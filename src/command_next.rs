use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

use crate::{
    buffer_position::{BufferPosition, BufferPositionIndex},
    client::{ClientHandle, ClientManager},
    editor::Editor,
    editor_utils::hash_bytes,
    platform::Platform,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    Commands,
    Buffers,
    Files,
    Custom(&'static [&'static str]),
}

pub struct BuiltinCommand {
    pub name_hash: u64,
    pub alias_hash: u64,
    pub hidden: bool,
    pub completions: &'static [CompletionSource],
    pub flags_hashes: &'static [u64],
    pub func: fn(),
}

struct MacroCommand {
    name_hash: u64,
    op_start_index: u32,
    param_count: u8,
}

struct RequestCommand {
    name_hash: u64,
}

struct CommandCollection {
    builtin_commands: &'static [BuiltinCommand],
    macro_commands: Vec<MacroCommand>,
    request_commands: Vec<RequestCommand>,
}
impl Default for CommandCollection {
    fn default() -> Self {
        Self {
            builtin_commands: &[], // TODO: reference builtin commands
            macro_commands: Vec::new(),
            request_commands: Vec::new(),
        }
    }
}

struct SourcePathCollection {
    buf: String,
    ranges: Vec<Range<u16>>,
}
impl SourcePathCollection {
    pub fn get(&self, index: usize) -> &Path {
        let range = self.ranges[index].clone();
        let range = range.start as usize..range.end as usize;
        Path::new(&self.buf[range])
    }

    pub fn index_of(&mut self, path: &Path) -> usize {
        let path = match path.to_str() {
            Some(path) => path,
            None => return 0,
        };

        for (i, range) in self.ranges.iter().enumerate() {
            let range = range.start as usize..range.end as usize;
            if &self.buf[range] == path {
                return i;
            }
        }

        let start = self.buf.len();
        self.buf.push_str(path);
        let end = self.buf.len();
        let index = self.ranges.len();
        self.ranges.push(start as _..end as _);
        index
    }
}
impl Default for SourcePathCollection {
    fn default() -> Self {
        Self {
            buf: String::new(),
            ranges: vec![0..0],
        }
    }
}

#[derive(Default)]
pub struct CommandManager {
    commands: CommandCollection,
    ast: Ast,
    bindings: Vec<Binding>,
    virtual_machine: VirtualMachine,
}
impl CommandManager {
    pub fn eval(editor: &mut Editor, source: &str) -> Result<(), CommandError> {
        let commands = &mut editor.commands_next;

        commands.ast.clear();
        commands.bindings.clear();

        let mut parser = Parser {
            tokenizer: CommandTokenizer::new(source),
            source_index: 0,
            paths: &mut commands.virtual_machine.paths,
            ast: &mut commands.ast,
            bindings: &mut commands.bindings,
            previous_token: CommandToken::default(),
            previous_statement_index: 0,
        };
        parse(&mut parser)?;

        let mut compiler = Compiler {
            ast: &commands.ast,
            commands: &mut commands.commands,
            virtual_machine: &mut commands.virtual_machine,
        };
        let definitions_len = compile(&mut compiler)?;

        commands
            .virtual_machine
            .ops
            .truncate(definitions_len.ops as _);
        commands
            .virtual_machine
            .texts
            .truncate(definitions_len.texts as _);
        commands
            .virtual_machine
            .op_locations
            .truncate(definitions_len.op_locations as _);

        Ok(())
    }
}

#[derive(Debug)]
pub enum CommandErrorKind {
    UnterminatedQuotedLiteral,
    InvalidFlagName,
    InvalidBindingName,

    TooManySources,
    ExpectedToken(CommandTokenKind),
    ExpectedStatement,
    ExpectedExpression,
    InvalidBindingDeclarationAtTopLevel,
    LiteralTooBig,
    InvalidLiteralEscaping,
    TooManyBindings,
    UndeclaredBinding,
    NoSuchCommand,
    NoSuchFlag,
    WrongNumberOfArgs,
    TooManyFlags,
    CouldNotSourceFile,
    CommandAlreadyExists,
}

const _ASSERT_COMMAND_ERROR_SIZE: [(); 12] = [(); std::mem::size_of::<CommandError>()];

#[derive(Debug)]
pub struct CommandError {
    pub kind: CommandErrorKind,
    pub source_index: u8,
    pub position: BufferPosition,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandTokenKind {
    Literal,
    QuotedLiteral,
    Flag,
    Equals,
    Binding,
    OpenCurlyBrackets,
    CloseCurlyBrackets,
    OpenParenthesis,
    CloseParenthesis,
    EndOfCommand,
    EndOfSource,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandToken {
    pub kind: CommandTokenKind,
    pub range: Range<u32>,
    pub position: BufferPosition,
}
impl CommandToken {
    pub fn range(&self) -> Range<usize> {
        self.range.start as _..self.range.end as _
    }
}
impl Default for CommandToken {
    fn default() -> Self {
        Self {
            kind: CommandTokenKind::EndOfSource,
            range: 0..0,
            position: BufferPosition::zero(),
        }
    }
}

pub struct CommandTokenizer<'a> {
    source: &'a str,
    index: usize,
    position: BufferPosition,
}
impl<'a> CommandTokenizer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            index: 0,
            position: BufferPosition::zero(),
        }
    }

    pub fn next(&mut self) -> Result<CommandToken, CommandError> {
        fn consume_identifier(iter: &mut CommandTokenizer) {
            let source = &iter.source[iter.index..];
            let len = match source
                .find(|c| !matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-'))
            {
                Some(len) => len,
                None => source.len(),
            };
            iter.index += len;
            iter.position.column_byte_index += len as BufferPositionIndex;
        }
        fn single_byte_token(iter: &mut CommandTokenizer, kind: CommandTokenKind) -> CommandToken {
            let from = iter.index;
            let position = iter.position;
            iter.index += 1;
            iter.position.column_byte_index += 1;
            CommandToken {
                kind,
                range: from as _..iter.index as _,
                position,
            }
        }

        let source_bytes = self.source.as_bytes();

        loop {
            if self.index == source_bytes.len() {
                let position = self.position;
                self.index += 1;
                self.position.column_byte_index += 1;
                return Ok(CommandToken {
                    kind: CommandTokenKind::EndOfCommand,
                    range: source_bytes.len() as _..source_bytes.len() as _,
                    position,
                });
            }
            if self.index > source_bytes.len() {
                let position = self.position;
                return Ok(CommandToken {
                    kind: CommandTokenKind::EndOfSource,
                    range: source_bytes.len() as _..source_bytes.len() as _,
                    position,
                });
            }

            match source_bytes[self.index] {
                b' ' | b'\t' | b'\r' => {
                    self.index += 1;
                    self.position.column_byte_index += 1;
                }
                b'\n' => {
                    let from = self.index;
                    let position = self.position;
                    while self.index < source_bytes.len() {
                        match source_bytes[self.index] {
                            b' ' | b'\t' | b'\r' => {
                                self.index += 1;
                                self.position.column_byte_index += 1;
                            }
                            b'\n' => {
                                self.index += 1;
                                self.position.line_index += 1;
                                self.position.column_byte_index = 0;
                            }
                            _ => break,
                        }
                    }
                    return Ok(CommandToken {
                        kind: CommandTokenKind::EndOfCommand,
                        range: from as _..self.index as _,
                        position,
                    });
                }
                delim @ (b'"' | b'\'') => {
                    let position = self.position;
                    let from = self.index;
                    self.index += 1;
                    self.position.column_byte_index += 1;
                    loop {
                        if self.index >= source_bytes.len() {
                            return Err(CommandError {
                                kind: CommandErrorKind::UnterminatedQuotedLiteral,
                                source_index: 0,
                                position,
                            });
                        }

                        let byte = source_bytes[self.index];
                        match byte {
                            b'\\' => {
                                self.index += 2;
                                self.position.column_byte_index += 2;
                            }
                            b'\n' => {
                                self.index += 1;
                                self.position.line_index += 1;
                                self.position.column_byte_index = 0;
                            }
                            _ => {
                                self.index += 1;
                                self.position.column_byte_index += 1;
                                if byte == delim {
                                    break;
                                }
                            }
                        }
                    }
                    return Ok(CommandToken {
                        kind: CommandTokenKind::QuotedLiteral,
                        range: from as _..self.index as _,
                        position,
                    });
                }
                b'-' => {
                    let from = self.index;
                    let position = self.position;
                    self.index += 1;
                    self.position.column_byte_index += 1;
                    consume_identifier(self);
                    let range = from as _..self.index as _;
                    if range.start + 1 == range.end {
                        return Err(CommandError {
                            kind: CommandErrorKind::InvalidFlagName,
                            source_index: 0,
                            position,
                        });
                    } else {
                        return Ok(CommandToken {
                            kind: CommandTokenKind::Flag,
                            range,
                            position,
                        });
                    }
                }
                b'$' => {
                    let from = self.index;
                    let position = self.position;
                    self.index += 1;
                    self.position.column_byte_index += 1;
                    consume_identifier(self);
                    let range = from as _..self.index as _;
                    if range.start + 1 == range.end {
                        return Err(CommandError {
                            kind: CommandErrorKind::InvalidBindingName,
                            source_index: 0,
                            position,
                        });
                    } else {
                        return Ok(CommandToken {
                            kind: CommandTokenKind::Binding,
                            range,
                            position,
                        });
                    }
                }
                b'=' => return Ok(single_byte_token(self, CommandTokenKind::Equals)),
                b'{' => return Ok(single_byte_token(self, CommandTokenKind::OpenCurlyBrackets)),
                b'}' => {
                    return Ok(single_byte_token(
                        self,
                        CommandTokenKind::CloseCurlyBrackets,
                    ))
                }
                b'(' => return Ok(single_byte_token(self, CommandTokenKind::OpenParenthesis)),
                b')' => return Ok(single_byte_token(self, CommandTokenKind::CloseParenthesis)),
                _ => {
                    let from = self.index;
                    let position = self.position;
                    self.index += 1;
                    self.position.column_byte_index += 1;
                    while self.index < source_bytes.len() {
                        match source_bytes[self.index] {
                            b'{' | b'}' | b'(' | b')' | b' ' | b'\t' | b'\r' | b'\n' => break,
                            _ => {
                                self.index += 1;
                                self.position.column_byte_index += 1;
                            }
                        }
                    }
                    return Ok(CommandToken {
                        kind: CommandTokenKind::Literal,
                        range: from as _..self.index as _,
                        position,
                    });
                }
            }
        }
    }
}

const _ASSERT_AST_NODE_SIZE: [(); 24] = [(); std::mem::size_of::<AstNode>()];

#[derive(Debug)]
enum AstNode {
    Literal {
        range: Range<u32>,
        position: BufferPosition,
    },
    QuotedLiteral {
        range: Range<u32>,
        position: BufferPosition,
    },
    Binding {
        index: u16,
        position: BufferPosition,
    },
    Statement {
        source: u8,
        next: u16,
    },
    CommandCall {
        name_hash: u64,
        position: BufferPosition,
        first_arg: u16,
        first_flag: u16,
    },
    CommandCallArg {
        next: u16,
    },
    CommandCallFlag {
        name_hash: u64,
        next: u16,
    },
    BindingDeclaration,
    MacroDeclaration {
        name_hash: u64,
        position: BufferPosition,
        param_count: u8,
    },
    Return {
        position: BufferPosition,
    },
}

#[derive(Default)]
struct Ast {
    pub nodes: Vec<AstNode>,
    pub texts: String,
}
impl Ast {
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.texts.clear();
    }
}

struct Binding {
    pub range: Range<u32>,
}

struct Parser<'source, 'data> {
    tokenizer: CommandTokenizer<'source>,
    pub source_index: u8,
    pub paths: &'data mut SourcePathCollection,
    pub ast: &'data mut Ast,
    pub bindings: &'data mut Vec<Binding>,
    pub previous_token: CommandToken,
    pub previous_statement_index: u16,
}
impl<'source, 'data> Parser<'source, 'data> {
    pub fn previous_token_str(&self) -> &'source str {
        &self.tokenizer.source[self.previous_token.range()]
    }

    pub fn next_token(&mut self) -> Result<(), CommandError> {
        match self.tokenizer.next() {
            Ok(token) => {
                self.previous_token = token;
                Ok(())
            }
            Err(mut error) => {
                error.source_index = self.source_index;
                Err(error)
            }
        }
    }

    pub fn consume_token(&mut self, kind: CommandTokenKind) -> Result<(), CommandError> {
        if self.previous_token.kind == kind {
            self.next_token()
        } else {
            Err(CommandError {
                kind: CommandErrorKind::ExpectedToken(kind),
                source_index: self.source_index,
                position: self.previous_token.position,
            })
        }
    }

    pub fn declare_binding_from_previous_token(&mut self) -> Result<(), CommandError> {
        if self.bindings.len() >= 0 as _ {
            Err(CommandError {
                kind: CommandErrorKind::TooManyBindings,
                source_index: self.source_index,
                position: self.previous_token.position,
            })
        } else {
            let range = self.previous_token.range.clone();
            self.bindings.push(Binding { range });
            Ok(())
        }
    }

    pub fn find_binding_stack_index(&self, name: &str) -> Option<u16> {
        let source = self.tokenizer.source;
        self.bindings
            .iter()
            .rposition(|b| &source[b.range.start as usize..b.range.end as usize] == name)
            .map(|i| i as _)
    }

    pub fn add_statement(&mut self) {
        let index = self.ast.nodes.len() as _;
        if self.previous_statement_index > 0 {
            match &mut self.ast.nodes[self.previous_statement_index as usize] {
                AstNode::Statement { next, .. } => *next = index,
                _ => unreachable!(),
            }
        }
        self.previous_statement_index = index;
        self.ast.nodes.push(AstNode::Statement {
            source: self.source_index,
            next: 0,
        });
    }

    pub fn hash_from_previous_token(&self) -> u64 {
        let range = &self.previous_token.range;
        let text = &self.tokenizer.source[range.start as usize..range.end as usize];
        hash_bytes(text.as_bytes())
    }
}

fn parse(parser: &mut Parser) -> Result<(), CommandError> {
    fn parse_top_level(parser: &mut Parser) -> Result<(), CommandError> {
        if let CommandTokenKind::Literal = parser.previous_token.kind {
            match parser.previous_token_str() {
                "source" => return parse_source(parser),
                "macro" => return parse_macro(parser),
                "return" => return parse_return(parser, true),
                _ => (),
            }
        }

        parse_statement(parser, true)?;
        Ok(())
    }

    fn parse_source(parser: &mut Parser) -> Result<(), CommandError> {
        parser.next_token()?;
        parser.consume_token(CommandTokenKind::QuotedLiteral)?;

        let path = Path::new(parser.previous_token_str());
        let path = if path.is_absolute() {
            path.into()
        } else {
            let mut buf = PathBuf::new();
            let current_path = parser.paths.get(parser.source_index as _);
            if !current_path.as_os_str().is_empty() {
                buf.push(current_path);
            }
            buf.push(path);
            buf
        };

        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(_) => {
                return Err(CommandError {
                    kind: CommandErrorKind::CouldNotSourceFile,
                    source_index: parser.source_index,
                    position: parser.previous_token.position,
                })
            }
        };

        let source_index = parser.paths.index_of(&path);
        if source_index > u8::MAX as _ {
            return Err(CommandError {
                kind: CommandErrorKind::TooManySources,
                source_index: parser.source_index,
                position: parser.previous_token.position,
            });
        }

        parser.next_token()?;

        let mut source_parser = Parser {
            tokenizer: CommandTokenizer::new(&source),
            source_index: source_index as _,
            paths: parser.paths,
            ast: parser.ast,
            bindings: parser.bindings,
            previous_token: CommandToken::default(),
            previous_statement_index: parser.previous_statement_index,
        };
        parse(&mut source_parser)?;

        parser.previous_statement_index = source_parser.previous_statement_index;

        Ok(())
    }

    fn parse_macro(parser: &mut Parser) -> Result<(), CommandError> {
        parser.next_token()?;
        parser.consume_token(CommandTokenKind::Literal)?;

        let index = parser.ast.nodes.len();
        parser.add_statement();
        parser.ast.nodes.push(AstNode::MacroDeclaration {
            name_hash: parser.hash_from_previous_token(),
            position: parser.previous_token.position,
            param_count: 0,
        });
        parser.next_token()?;

        let previous_bindings_len = parser.bindings.len();
        loop {
            match parser.previous_token.kind {
                CommandTokenKind::OpenCurlyBrackets => {
                    match &mut parser.ast.nodes[index] {
                        AstNode::MacroDeclaration { param_count, .. } => {
                            *param_count = (parser.bindings.len() - previous_bindings_len) as _;
                        }
                        _ => unreachable!(),
                    }
                    parser.next_token()?;
                    break;
                }
                CommandTokenKind::Binding => {
                    parser.declare_binding_from_previous_token()?;
                    parser.next_token()?;
                }
                _ => {
                    return Err(CommandError {
                        kind: CommandErrorKind::ExpectedToken(CommandTokenKind::OpenCurlyBrackets),
                        source_index: parser.source_index,
                        position: parser.previous_token.position,
                    })
                }
            }
        }

        while parser.previous_token.kind != CommandTokenKind::CloseCurlyBrackets {
            parse_statement(parser, false)?;
        }
        parser.next_token()?;

        parser.bindings.clear();
        Ok(())
    }

    fn parse_return(parser: &mut Parser, is_top_level: bool) -> Result<(), CommandError> {
        parser.add_statement();
        parser.ast.nodes.push(AstNode::Return {
            position: parser.previous_token.position,
        });
        parser.next_token()?;
        parse_expression(parser, is_top_level)?;
        Ok(())
    }

    fn parse_statement(parser: &mut Parser, is_top_level: bool) -> Result<(), CommandError> {
        loop {
            match parser.previous_token.kind {
                CommandTokenKind::Literal | CommandTokenKind::OpenParenthesis => {
                    parser.add_statement();
                    return parse_command_call(parser, is_top_level);
                }
                CommandTokenKind::Binding => {
                    if is_top_level {
                        return Err(CommandError {
                            kind: CommandErrorKind::InvalidBindingDeclarationAtTopLevel,
                            source_index: parser.source_index,
                            position: parser.previous_token.position,
                        });
                    }

                    parser.declare_binding_from_previous_token()?;
                    parser.add_statement();
                    parser.ast.nodes.push(AstNode::BindingDeclaration);

                    parser.next_token()?;
                    parser.consume_token(CommandTokenKind::Equals)?;

                    parse_expression(parser, is_top_level)?;
                    return Ok(());
                }
                CommandTokenKind::EndOfCommand => parser.next_token()?,
                CommandTokenKind::EndOfSource => return Ok(()),
                _ => {
                    return Err(CommandError {
                        kind: CommandErrorKind::ExpectedStatement,
                        source_index: parser.source_index,
                        position: parser.previous_token.position,
                    });
                }
            }
        }
    }

    fn parse_command_call(parser: &mut Parser, is_top_level: bool) -> Result<(), CommandError> {
        fn find_end_token_kind(parser: &mut Parser) -> Result<CommandTokenKind, CommandError> {
            match parser.previous_token.kind {
                CommandTokenKind::Literal => return Ok(CommandTokenKind::EndOfCommand),
                CommandTokenKind::OpenParenthesis => {
                    parser.next_token()?;
                    if let CommandTokenKind::Literal = parser.previous_token.kind {
                        return Ok(CommandTokenKind::CloseParenthesis);
                    }
                }
                _ => (),
            }

            Err(CommandError {
                kind: CommandErrorKind::ExpectedToken(CommandTokenKind::Literal),
                source_index: parser.source_index,
                position: parser.previous_token.position,
            })
        }

        let end_token_kind = find_end_token_kind(parser)?;
        let index = parser.ast.nodes.len();

        parser.ast.nodes.push(AstNode::CommandCall {
            name_hash: parser.hash_from_previous_token(),
            position: parser.previous_token.position,
            first_arg: 0,
            first_flag: 0,
        });

        parser.next_token()?;

        let mut arg_count = 0;
        let mut last_arg = 0;
        let mut flag_count = 0;
        let mut last_flag = 0;

        loop {
            if parser.previous_token.kind == CommandTokenKind::Flag {
                let len = parser.ast.nodes.len() as _;

                match flag_count {
                    0 => match &mut parser.ast.nodes[index] {
                        AstNode::CommandCall { first_flag, .. } => *first_flag = len,
                        _ => unreachable!(),
                    },
                    u8::MAX => {
                        return Err(CommandError {
                            kind: CommandErrorKind::TooManyFlags,
                            source_index: parser.source_index,
                            position: parser.previous_token.position,
                        });
                    }
                    _ => (),
                }

                if let AstNode::CommandCallFlag { next, .. } = &mut parser.ast.nodes[last_flag] {
                    *next = len;
                }
                last_flag = parser.ast.nodes.len();
                parser.ast.nodes.push(AstNode::CommandCallFlag {
                    name_hash: parser.hash_from_previous_token(),
                    next: 0,
                });

                let position = parser.previous_token.position;
                parser.next_token()?;
                match parser.previous_token.kind {
                    CommandTokenKind::Equals => {
                        parser.next_token()?;
                        parse_expression(parser, is_top_level)?;
                    }
                    _ => parser.ast.nodes.push(AstNode::Literal {
                        range: 0..0,
                        position,
                    }),
                }

                flag_count += 1;
            } else if parser.previous_token.kind == end_token_kind {
                parser.next_token()?;
                break;
            } else {
                let len = parser.ast.nodes.len() as _;

                if arg_count == 0 {
                    match &mut parser.ast.nodes[index] {
                        AstNode::CommandCall { first_arg, .. } => *first_arg = len,
                        _ => unreachable!(),
                    }
                }
                if let AstNode::CommandCallArg { next, .. } = &mut parser.ast.nodes[last_arg] {
                    *next = len;
                }
                last_arg = parser.ast.nodes.len();
                parser.ast.nodes.push(AstNode::CommandCallArg { next: 0 });

                let expression_position = parse_expression(parser, is_top_level)?;
                if arg_count == u8::MAX {
                    return Err(CommandError {
                        kind: CommandErrorKind::WrongNumberOfArgs,
                        source_index: parser.source_index,
                        position: expression_position,
                    });
                }

                arg_count += 1;
            }
        }

        Ok(())
    }

    fn parse_expression(
        parser: &mut Parser,
        is_top_level: bool,
    ) -> Result<BufferPosition, CommandError> {
        fn consume_literal_range(parser: &mut Parser) -> Result<Range<u32>, CommandError> {
            let range = parser.previous_token.range();
            if range.end - range.start <= u8::MAX as _ {
                let start = parser.ast.texts.len();
                parser.ast.texts.push_str(&parser.tokenizer.source[range]);
                let end = parser.ast.texts.len();
                parser.next_token()?;
                Ok(start as _..end as _)
            } else {
                Err(CommandError {
                    kind: CommandErrorKind::LiteralTooBig,
                    source_index: parser.source_index,
                    position: parser.previous_token.position,
                })
            }
        }

        while let CommandTokenKind::EndOfCommand = parser.previous_token.kind {
            parser.next_token()?;
        }

        match parser.previous_token.kind {
            CommandTokenKind::Literal => {
                let position = parser.previous_token.position;
                let range = consume_literal_range(parser)?;
                parser.ast.nodes.push(AstNode::Literal { range, position });
                Ok(position)
            }
            CommandTokenKind::QuotedLiteral => {
                let position = parser.previous_token.position;
                let range = consume_literal_range(parser)?;
                parser
                    .ast
                    .nodes
                    .push(AstNode::QuotedLiteral { range, position });
                Ok(position)
            }
            CommandTokenKind::OpenParenthesis => {
                let position = parser.previous_token.position;
                parse_command_call(parser, is_top_level)?;
                Ok(position)
            }
            CommandTokenKind::Binding => {
                let position = parser.previous_token.position;
                let binding_name = parser.previous_token_str();
                match parser.find_binding_stack_index(binding_name) {
                    Some(index) => {
                        parser.next_token()?;
                        parser.ast.nodes.push(AstNode::Binding { index, position });
                        Ok(position)
                    }
                    None => Err(CommandError {
                        kind: CommandErrorKind::UndeclaredBinding,
                        source_index: parser.source_index,
                        position,
                    }),
                }
            }
            _ => Err(CommandError {
                kind: CommandErrorKind::ExpectedExpression,
                source_index: parser.source_index,
                position: parser.previous_token.position,
            }),
        }
    }

    parser.next_token()?;

    while parser.previous_token.kind != CommandTokenKind::EndOfSource {
        parse_top_level(parser)?;
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum CommandSource {
    Builtin(usize),
    Macro(usize),
    Request(usize),
}

fn find_command(commands: &CommandCollection, name_hash: u64) -> Option<CommandSource> {
    if let Some(i) = commands
        .macro_commands
        .iter()
        .position(|c| c.name_hash == name_hash)
    {
        return Some(CommandSource::Macro(i));
    }

    if let Some(i) = commands
        .request_commands
        .iter()
        .position(|c| c.name_hash == name_hash)
    {
        return Some(CommandSource::Request(i));
    }

    if let Some(i) = commands
        .builtin_commands
        .iter()
        .position(|c| c.name_hash == name_hash || c.alias_hash == name_hash)
    {
        return Some(CommandSource::Builtin(i));
    }

    None
}

struct Compiler<'data> {
    pub ast: &'data Ast,
    pub commands: &'data mut CommandCollection,
    pub virtual_machine: &'data mut VirtualMachine,
}
impl<'data> Compiler<'data> {
    pub fn emit(&mut self, op: Op, location: SourceLocation) {
        self.virtual_machine.ops.push(op);
        self.virtual_machine.op_locations.push(location);
    }

    pub fn emit_push_literal(&mut self, range: Range<u32>, location: SourceLocation) {
        let literal = &self.ast.texts[range.start as usize..range.end as usize];
        let start = self.virtual_machine.texts.len();
        self.virtual_machine.texts.push_str(literal);
        let len = self.virtual_machine.texts.len() - start;
        self.emit(
            Op::PushLiteral {
                start: start as _,
                len: len as _,
            },
            location,
        );
    }

    pub fn emit_push_escaped_literal(
        &mut self,
        range: Range<u32>,
        location: SourceLocation,
    ) -> Result<(), CommandError> {
        let start = self.virtual_machine.texts.len();

        let mut literal = &self.ast.texts[range.start as usize..range.end as usize];
        while let Some(i) = literal.find('\\') {
            self.virtual_machine.texts.push_str(&literal[..i]);
            literal = &literal[i + 1..];
            match literal.as_bytes() {
                &[b'\\', ..] => self.virtual_machine.texts.push('\\'),
                &[b'\'', ..] => self.virtual_machine.texts.push('\''),
                &[b'\"', ..] => self.virtual_machine.texts.push('\"'),
                &[b'\n', ..] => self.virtual_machine.texts.push('\n'),
                &[b'\r', ..] => self.virtual_machine.texts.push('\r'),
                &[b'\t', ..] => self.virtual_machine.texts.push('\t'),
                &[b'\0', ..] => self.virtual_machine.texts.push('\0'),
                _ => {
                    return Err(CommandError {
                        kind: CommandErrorKind::InvalidLiteralEscaping,
                        source_index: location.source_index,
                        position: location.position,
                    })
                }
            }
        }
        self.virtual_machine.texts.push_str(literal);

        let len = self.virtual_machine.texts.len() - start;
        self.emit(
            Op::PushLiteral {
                start: start as _,
                len: len as _,
            },
            location,
        );
        Ok(())
    }
}

struct DefinitionsLen {
    pub ops: u32,
    pub texts: u32,
    pub op_locations: u32,
}
impl DefinitionsLen {
    pub fn from_virtual_machine(vm: &VirtualMachine) -> Self {
        Self {
            ops: vm.ops.len() as _,
            texts: vm.texts.len() as _,
            op_locations: vm.op_locations.len() as _,
        }
    }
}

fn compile(compiler: &mut Compiler) -> Result<DefinitionsLen, CommandError> {
    fn emit_final_return(compiler: &mut Compiler) {
        compiler.emit(
            Op::PushLiteral { start: 0, len: 0 },
            SourceLocation {
                source_index: 0,
                position: BufferPosition::zero(),
            },
        );
        compiler.emit(
            Op::Return,
            SourceLocation {
                source_index: 0,
                position: BufferPosition::zero(),
            },
        );
    }

    fn emit_expression(
        compiler: &mut Compiler,
        source_index: u8,
        index: usize,
    ) -> Result<(), CommandError> {
        match compiler.ast.nodes[index] {
            AstNode::Literal {
                ref range,
                position,
            } => compiler.emit_push_literal(
                range.clone(),
                SourceLocation {
                    source_index,
                    position,
                },
            ),
            AstNode::QuotedLiteral {
                ref range,
                position,
            } => {
                let mut range = range.clone();
                range.start += 1;
                range.end -= 1;
                compiler.emit_push_escaped_literal(
                    range,
                    SourceLocation {
                        source_index,
                        position,
                    },
                )?;
            }
            AstNode::Binding { index, position } => compiler.emit(
                Op::DuplicateAt(index),
                SourceLocation {
                    source_index,
                    position,
                },
            ),
            AstNode::CommandCall {
                name_hash,
                position,
                first_arg,
                first_flag,
                ..
            } => {
                let command_source = match find_command(compiler.commands, name_hash) {
                    Some(source) => source,
                    None => {
                        return Err(CommandError {
                            kind: CommandErrorKind::NoSuchCommand,
                            source_index,
                            position,
                        });
                    }
                };

                compiler.emit(
                    Op::PrepareStackFrame,
                    SourceLocation {
                        source_index,
                        position,
                    },
                );

                let mut arg = first_arg as usize;
                let mut flag = first_flag as usize;

                match command_source {
                    CommandSource::Builtin(i) => {
                        fn find_flag_index(
                            flags: &[u64],
                            name_hash: u64,
                            source_index: u8,
                            position: BufferPosition,
                        ) -> Result<usize, CommandError> {
                            for (i, &flag) in flags.iter().enumerate() {
                                if flag == name_hash {
                                    return Ok(i);
                                }
                            }
                            Err(CommandError {
                                kind: CommandErrorKind::NoSuchFlag,
                                source_index,
                                position,
                            })
                        }

                        let mut flag_expressions = [0; u8::MAX as _];
                        let flags = compiler.commands.builtin_commands[i].flags_hashes;
                        while let AstNode::CommandCallFlag { name_hash, next } =
                            compiler.ast.nodes[flag]
                        {
                            let flag_index =
                                find_flag_index(flags, name_hash, source_index, position)?;
                            flag_expressions[flag_index] = flag + 1;
                            flag = next as _;
                        }

                        for &expression in &flag_expressions[..flags.len()] {
                            if expression == 0 {
                                compiler.emit(
                                    Op::PushLiteral { start: 0, len: 0 },
                                    SourceLocation {
                                        source_index,
                                        position,
                                    },
                                );
                            } else {
                                emit_expression(compiler, source_index, expression)?;
                            }
                        }
                    }
                    _ => match compiler.ast.nodes[flag] {
                        AstNode::CommandCallFlag { .. } => {
                            return Err(CommandError {
                                kind: CommandErrorKind::NoSuchFlag,
                                source_index,
                                position,
                            });
                        }
                        _ => (),
                    },
                }

                while let AstNode::CommandCallArg { next } = compiler.ast.nodes[arg] {
                    emit_expression(compiler, source_index, arg + 1)?;
                    arg = next as _;
                }

                match command_source {
                    CommandSource::Builtin(i) => compiler.emit(
                        Op::CallBuiltinCommand(i as _),
                        SourceLocation {
                            source_index,
                            position,
                        },
                    ),
                    CommandSource::Macro(i) => compiler.emit(
                        Op::CallMacroCommand(i as _),
                        SourceLocation {
                            source_index,
                            position,
                        },
                    ),
                    CommandSource::Request(i) => compiler.emit(
                        Op::CallRequestCommand(i as _),
                        SourceLocation {
                            source_index,
                            position,
                        },
                    ),
                }
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn emit_statement(compiler: &mut Compiler, index: usize) -> Result<usize, CommandError> {
        let (source_index, next) = match compiler.ast.nodes[index] {
            AstNode::Statement { source, next } => (source, next),
            _ => unreachable!(),
        };

        let index = index + 1;
        match compiler.ast.nodes[index] {
            AstNode::CommandCall { position, .. } => {
                emit_expression(compiler, source_index, index)?;
                compiler.emit(
                    Op::Pop,
                    SourceLocation {
                        source_index,
                        position,
                    },
                );
            }
            AstNode::Return { position } => {
                emit_expression(compiler, source_index, index + 1)?;
                compiler.emit(
                    Op::Return,
                    SourceLocation {
                        source_index,
                        position,
                    },
                );
            }
            _ => unreachable!(),
        }

        Ok(next as _)
    }

    if compiler.ast.nodes.is_empty() {
        emit_final_return(compiler);
        return Ok(DefinitionsLen::from_virtual_machine(
            &compiler.virtual_machine,
        ));
    }

    let mut index = 0;

    loop {
        let (source_index, next) = match compiler.ast.nodes[index] {
            AstNode::Statement { source, next } => (source, next),
            _ => unreachable!(),
        };

        index += 1;
        if let AstNode::MacroDeclaration {
            name_hash,
            position,
            param_count,
        } = compiler.ast.nodes[index]
        {
            if find_command(compiler.commands, name_hash).is_some() {
                return Err(CommandError {
                    kind: CommandErrorKind::CommandAlreadyExists,
                    source_index,
                    position,
                });
            }

            let op_start_index = compiler.virtual_machine.ops.len() as _;
            index += 1;
            while index != 0 {
                index = emit_statement(compiler, index)?;
            }

            compiler.commands.macro_commands.push(MacroCommand {
                name_hash,
                op_start_index,
                param_count,
            });
        }

        if next == 0 {
            break;
        }
        index = next as _;
    }

    let definitions_len = DefinitionsLen::from_virtual_machine(&compiler.virtual_machine);

    index = 0;

    loop {
        let next = match compiler.ast.nodes[index] {
            AstNode::Statement { next, .. } => next,
            _ => unreachable!(),
        };

        if !matches!(
            compiler.ast.nodes[index + 1],
            AstNode::MacroDeclaration { .. }
        ) {
            emit_statement(compiler, index)?;
        }

        if next == 0 {
            break;
        }
        index = next as _;
    }

    emit_final_return(compiler);
    Ok(definitions_len)
}

const _ASSERT_OP_SIZE: [(); 4] = [(); std::mem::size_of::<Op>()];

#[derive(Debug, PartialEq, Eq)]
enum Op {
    Return,
    Pop,
    PushLiteral { start: u16, len: u8 },
    DuplicateAt(u16),
    PrepareStackFrame,
    CallBuiltinCommand(u8),
    CallMacroCommand(u16),
    CallRequestCommand(u16),
}

#[derive(Copy, Clone)]
struct StackValue {
    pub start: u32,
    pub end: u32,
}

#[derive(Copy, Clone)]
struct StackFlag {
    pub start: u32,
    pub end: u32,
    pub flag_index: u8,
}

struct StackFrame {
    op_index: u32,
    texts_len: u32,
    stack_len: u16,
}

struct SourceLocation {
    source_index: u8,
    position: BufferPosition,
}

#[derive(Default)]
struct VirtualMachine {
    ops: Vec<Op>,
    texts: String,
    stack: Vec<StackValue>,
    frames: Vec<StackFrame>,
    prepared_frames: Vec<StackFrame>,

    op_locations: Vec<SourceLocation>,
    paths: SourcePathCollection,
}

fn execute(
    editor: &mut Editor,
    platform: &mut Platform,
    clients: &mut ClientManager,
    client_handle: Option<ClientHandle>,
) -> Result<(), CommandError> {
    let mut vm = &mut editor.commands_next.virtual_machine;
    let mut op_index = 0;

    loop {
        match vm.ops[op_index] {
            Op::Return => {
                let frame = vm.frames.pop().unwrap();
                let return_start = vm.stack.last().unwrap().start as usize;
                vm.texts.drain(frame.texts_len as usize..return_start);
                vm.stack.truncate(frame.stack_len as _);
                vm.stack.push(StackValue {
                    start: frame.texts_len,
                    end: vm.texts.len() as _,
                });

                op_index = frame.op_index as _;
            }
            Op::Pop => drop(vm.stack.pop()),
            Op::PushLiteral { start, len } => {
                let start = start as usize;
                let end = start + len as usize;
                vm.stack.push(StackValue {
                    start: start as _,
                    end: end as _,
                });
            }
            Op::DuplicateAt(stack_index) => {
                let value = vm.stack[stack_index as usize];
                vm.stack.push(value);
            }
            Op::PrepareStackFrame => {
                let frame = StackFrame {
                    op_index: (op_index + 2) as _,
                    texts_len: vm.texts.len() as _,
                    stack_len: vm.stack.len() as _,
                };
                vm.prepared_frames.push(frame);
            }
            Op::CallBuiltinCommand(index) => {
                let frame = vm.prepared_frames.pop().unwrap();
                let return_start = vm.texts.len();

                let command_fn =
                    &editor.commands_next.commands.builtin_commands[index as usize].func;
                command_fn();

                vm = &mut editor.commands_next.virtual_machine;
                vm.texts.drain(frame.texts_len as usize..return_start);
                vm.stack.truncate(frame.stack_len as _);
                vm.stack.push(StackValue {
                    start: frame.texts_len as _,
                    end: vm.texts.len() as _,
                });
            }
            Op::CallMacroCommand(index) => {
                let frame = vm.prepared_frames.pop().unwrap();
                let arg_count = vm.stack.len() - frame.stack_len as usize;

                let command = &editor.commands_next.commands.macro_commands[index as usize];
                if arg_count != command.param_count as _ {
                    let location = &vm.op_locations[op_index];
                    return Err(CommandError {
                        kind: CommandErrorKind::WrongNumberOfArgs,
                        source_index: location.source_index,
                        position: location.position,
                    });
                }

                op_index = command.op_start_index as _;
                vm.frames.push(frame);
            }
            Op::CallRequestCommand(index) => {
                let frame = vm.prepared_frames.pop().unwrap();
                // TODO: send request
                vm.texts.truncate(frame.texts_len as _);
                vm.stack.truncate(frame.stack_len as _);
                vm.stack.push(StackValue { start: 0, end: 0 });
            }
        }
        op_index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_tokenizer() {
        fn pos(line: usize, column: usize) -> BufferPosition {
            BufferPosition::line_col(line as _, column as _)
        }

        fn collect<'a>(source: &'a str) -> Vec<(CommandTokenKind, &'a str, BufferPosition)> {
            let mut tokenizer = CommandTokenizer::new(source);
            let mut tokens = Vec::new();
            loop {
                let token = tokenizer.next().unwrap();
                match token.kind {
                    CommandTokenKind::EndOfSource => break,
                    _ => {
                        let text = &source[token.range()];
                        tokens.push((token.kind, text, token.position))
                    }
                }
            }
            tokens
        }

        use CommandTokenKind::*;

        assert_eq!(vec![(EndOfCommand, "", pos(0, 0))], collect(""));
        assert_eq!(vec![(EndOfCommand, "", pos(0, 2))], collect("  "));
        assert_eq!(
            vec![
                (Literal, "command", pos(0, 0)),
                (EndOfCommand, "", pos(0, 7))
            ],
            collect("command"),
        );
        assert_eq!(
            vec![
                (QuotedLiteral, "'text'", pos(0, 0)),
                (EndOfCommand, "", pos(0, 6))
            ],
            collect("'text'"),
        );
        assert_eq!(
            vec![
                (Literal, "cmd", pos(0, 0)),
                (OpenParenthesis, "(", pos(0, 4)),
                (Literal, "subcmd", pos(0, 5)),
                (CloseParenthesis, ")", pos(0, 11)),
                (EndOfCommand, "", pos(0, 12)),
            ],
            collect("cmd (subcmd)"),
        );
        assert_eq!(
            vec![
                (Literal, "cmd", pos(0, 0)),
                (Binding, "$binding", pos(0, 4)),
                (Flag, "-flag", pos(0, 13)),
                (Equals, "=", pos(0, 18)),
                (Literal, "value", pos(0, 19)),
                (Equals, "=", pos(0, 25)),
                (Literal, "not-flag", pos(0, 27)),
                (EndOfCommand, "", pos(0, 35)),
            ],
            collect("cmd $binding -flag=value = not-flag"),
        );
        assert_eq!(
            vec![
                (Literal, "cmd0", pos(0, 0)),
                (Literal, "cmd1", pos(0, 5)),
                (EndOfCommand, "\n\n \t \n  ", pos(0, 12)),
                (Literal, "cmd2", pos(3, 2)),
                (EndOfCommand, "", pos(3, 6)),
            ],
            collect("cmd0 cmd1 \t\r\n\n \t \n  cmd2"),
        );
    }

    #[test]
    fn command_compiler() {
        fn compile_source(source: &str) -> VirtualMachine {
            let mut paths = SourcePathCollection::default();
            let mut texts = String::new();
            let mut ast = Vec::new();
            let mut bindings = Vec::new();

            let mut parser = Parser {
                tokenizer: CommandTokenizer::new(source),
                source_index: 0,
                paths: &mut paths,
                texts: &mut texts,
                ast: &mut ast,
                bindings: &mut bindings,
                previous_token: CommandToken::default(),
                previous_statement_index: 0,
            };
            parse(&mut parser).unwrap();

            dbg!(&ast);

            let mut commands = CommandCollection::default();
            commands.builtin_commands = &[BuiltinCommand {
                name: "cmd",
                alias: "",
                hidden: false,
                completions: &[],
                flags: &["-switch", "-option"],
                func: || (),
            }];
            let mut virtual_machine = VirtualMachine::default();

            let mut compiler = Compiler {
                texts: &texts,
                ast: &ast,
                commands: &mut commands,
                virtual_machine: &mut virtual_machine,
            };
            compile(&mut compiler).unwrap();

            virtual_machine
        }

        use Op::*;

        assert_eq!(
            vec![PushLiteral { start: 0, len: 0 }, Return],
            compile_source("").ops,
        );

        assert_eq!(
            vec![
                PrepareStackFrame,
                PushLiteral { start: 0, len: 0 },
                PushLiteral { start: 0, len: 0 },
                CallBuiltinCommand(0),
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile_source("cmd").ops,
        );

        assert_eq!(
            vec![
                PrepareStackFrame,
                PushLiteral { start: 0, len: 0 },
                PushLiteral { start: 0, len: 0 },
                PushLiteral {
                    start: 0,
                    len: "arg0".len() as _,
                },
                PushLiteral {
                    start: "arg0".len() as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand(0),
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile_source("cmd arg0 arg1").ops,
        );

        assert_eq!(
            vec![
                PrepareStackFrame,
                PushLiteral { start: 0, len: 0 },
                PushLiteral {
                    start: "arg".len() as _,
                    len: "opt".len() as _,
                },
                PushLiteral {
                    start: 0,
                    len: "arg".len() as _,
                },
                CallBuiltinCommand(0),
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile_source("cmd -switch arg -option=opt").ops,
        );

        assert_eq!(
            vec![
                PrepareStackFrame,
                PushLiteral { start: 0, len: 0 },
                // begin nested call
                PrepareStackFrame,
                PushLiteral { start: 0, len: 0 },
                PushLiteral { start: 0, len: 0 },
                PushLiteral {
                    start: 0,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand(0),
                // end nested call
                PushLiteral {
                    start: "arg1".len() as _,
                    len: "arg0".len() as _,
                },
                PushLiteral {
                    start: "arg1arg0".len() as _,
                    len: "arg2".len() as _,
                },
                CallBuiltinCommand(0),
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile_source("cmd arg0 -option=(cmd arg1) arg2").ops,
        );

        assert_eq!(
            vec![
                PrepareStackFrame,
                PushLiteral { start: 0, len: 0 },
                PushLiteral { start: 0, len: 0 },
                PushLiteral {
                    start: 0,
                    len: "arg0".len() as _,
                },
                PushLiteral {
                    start: "arg0".len() as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand(0),
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile_source("(cmd \n arg0 \n arg2)").ops,
        );
    }
}

