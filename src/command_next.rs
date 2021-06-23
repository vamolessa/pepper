use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

use crate::{
    client::{ClientHandle, ClientManager},
    editor::Editor,
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
    pub name: &'static str,
    pub alias: &'static str,
    pub hidden: bool,
    pub completions: &'static [CompletionSource],
    pub flags: &'static [&'static str],
    pub func: fn(),
}

struct MacroCommand {
    name_range: Range<u32>,
    op_start_index: u32,
    param_count: u8,
}

struct RequestCommand {
    name_range: Range<u32>,
}

struct CommandCollection {
    builtin_commands: &'static [BuiltinCommand],
    custom_command_names: String,
    macro_commands: Vec<MacroCommand>,
    request_commands: Vec<RequestCommand>,
}
impl CommandCollection {
    pub fn add_custom_command_name(&mut self, name: &str) -> Range<u32> {
        let start = self.custom_command_names.len();
        self.custom_command_names.push_str(name);
        let end = self.custom_command_names.len();
        start as _..end as _
    }
}
impl Default for CommandCollection {
    fn default() -> Self {
        Self {
            builtin_commands: &[], // TODO: reference builtin commands
            custom_command_names: String::new(),
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
    temp_ast: Vec<CommandAstNode>,
    temp_bindings: Vec<Binding>,
    paths: SourcePathCollection,
    virtual_machine: VirtualMachine,
}
impl CommandManager {
    pub fn eval() -> Result<(), CommandError> {
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
    pub source: u8,
    pub range: Range<u32>,
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
}
impl CommandToken {
    pub fn range(&self) -> Range<usize> {
        self.range.start as _..self.range.end as _
    }
}

pub struct CommandTokenizer<'a> {
    pub source: &'a str,
    index: usize,
}
impl<'a> CommandTokenizer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source, index: 0 }
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
        }
        fn single_byte_token(iter: &mut CommandTokenizer, kind: CommandTokenKind) -> CommandToken {
            let from = iter.index;
            iter.index += 1;
            CommandToken {
                kind,
                range: from as _..iter.index as _,
            }
        }

        let source_bytes = self.source.as_bytes();

        loop {
            loop {
                if self.index == source_bytes.len() {
                    self.index += 1;
                    return Ok(CommandToken {
                        kind: CommandTokenKind::EndOfCommand,
                        range: source_bytes.len() as _..source_bytes.len() as _,
                    });
                }
                if self.index > source_bytes.len() {
                    return Ok(CommandToken {
                        kind: CommandTokenKind::EndOfSource,
                        range: source_bytes.len() as _..source_bytes.len() as _,
                    });
                }
                if matches!(source_bytes[self.index], b' ' | b'\t') {
                    self.index += 1;
                } else {
                    break;
                }
            }

            match source_bytes[self.index] {
                delim @ (b'"' | b'\'') => {
                    let from = self.index;
                    self.index += 1;
                    loop {
                        if self.index >= source_bytes.len() {
                            return Err(CommandError {
                                kind: CommandErrorKind::UnterminatedQuotedLiteral,
                                source: 0,
                                range: from as _..source_bytes.len() as _,
                            });
                        }

                        let byte = source_bytes[self.index];
                        if byte == b'\\' {
                            self.index += 2;
                        } else {
                            self.index += 1;
                            if byte == delim {
                                break;
                            }
                        }
                    }
                    return Ok(CommandToken {
                        kind: CommandTokenKind::QuotedLiteral,
                        range: from as _..self.index as _,
                    });
                }
                b'-' => {
                    let from = self.index;
                    self.index += 1;
                    consume_identifier(self);
                    let range = from as _..self.index as _;
                    if range.start + 1 == range.end {
                        return Err(CommandError {
                            kind: CommandErrorKind::InvalidFlagName,
                            source: 0,
                            range,
                        });
                    } else {
                        return Ok(CommandToken {
                            kind: CommandTokenKind::Flag,
                            range,
                        });
                    }
                }
                b'$' => {
                    let from = self.index;
                    self.index += 1;
                    consume_identifier(self);
                    let range = from as _..self.index as _;
                    if range.start + 1 == range.end {
                        return Err(CommandError {
                            kind: CommandErrorKind::InvalidBindingName,
                            source: 0,
                            range,
                        });
                    } else {
                        return Ok(CommandToken {
                            kind: CommandTokenKind::Binding,
                            range,
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
                b'\r' | b'\n' => {
                    let from = self.index;
                    while self.index < source_bytes.len()
                        && matches!(source_bytes[self.index], b' ' | b'\t' | b'\r' | b'\n')
                    {
                        self.index += 1;
                    }
                    return Ok(CommandToken {
                        kind: CommandTokenKind::EndOfCommand,
                        range: from as _..self.index as _,
                    });
                }
                _ => {
                    let from = self.index;
                    self.index += 1;
                    while self.index < source_bytes.len() {
                        match source_bytes[self.index] {
                            b'{' | b'}' | b'(' | b')' | b' ' | b'\t' | b'\r' | b'\n' => break,
                            _ => self.index += 1,
                        }
                    }
                    return Ok(CommandToken {
                        kind: CommandTokenKind::Literal,
                        range: from as _..self.index as _,
                    });
                }
            }
        }
    }
}

const _ASSERT_AST_NODE_SIZE: [(); 16] = [(); std::mem::size_of::<CommandAstNode>()];

enum CommandAstNode {
    Literal(Range<u32>),
    QuotedLiteral(Range<u32>),
    Binding(u16),
    CommandCall {
        source: u8,
        name: Range<u32>,
        first_arg: u16,
        first_flag: u16,
        next: u16,
    },
    CommandCallArg {
        next: u16,
    },
    CommandCallFlag {
        name: Range<u32>,
        next: u16,
    },
    BindingDeclaration {
        source: u8,
        name: Range<u32>,
        next: u16,
    },
    MacroDeclaration {
        source: u8,
        name: Range<u32>,
        param_count: u8,
        next: u16,
    },
    Return {
        source: u8,
        next: u16,
    },
}

struct Binding {
    pub range: Range<u32>,
}

struct Parser<'source, 'data> {
    tokenizer: CommandTokenizer<'source>,
    pub path: Option<&'data Path>,
    pub paths: &'data mut SourcePathCollection,
    pub source_index: u8,
    pub ast: &'data mut Vec<CommandAstNode>,
    pub bindings: &'data mut Vec<Binding>,
    pub previous_token: CommandToken,
}
impl<'source, 'data> Parser<'source, 'data> {
    pub fn new(
        source: &'source str,
        path: Option<&'data Path>,
        paths: &'data mut SourcePathCollection,
        ast: &'data mut Vec<CommandAstNode>,
        bindings: &'data mut Vec<Binding>,
        source_index: u8,
    ) -> Result<Self, CommandError> {
        let mut tokenizer = CommandTokenizer::new(source);
        let previous_token = tokenizer.next()?;
        Ok(Self {
            tokenizer,
            path,
            paths,
            source_index: source_index as _,
            ast,
            bindings,
            previous_token,
        })
    }

    pub fn previous_token_str(&self) -> &'source str {
        &self.tokenizer.source[self.previous_token.range()]
    }

    pub fn next_token(&mut self) -> Result<(), CommandError> {
        let token = self.tokenizer.next()?;
        self.previous_token = token;
        Ok(())
    }

    pub fn consume_token(&mut self, kind: CommandTokenKind) -> Result<(), CommandError> {
        if self.previous_token.kind == kind {
            self.next_token()
        } else {
            Err(CommandError {
                kind: CommandErrorKind::ExpectedToken(kind),
                source: self.source_index,
                range: self.previous_token.range.clone(),
            })
        }
    }

    pub fn declare_binding_from_previous_token(&mut self) -> Result<&Binding, CommandError> {
        if self.bindings.len() >= u16::MAX as _ {
            Err(CommandError {
                kind: CommandErrorKind::TooManyBindings,
                source: self.source_index,
                range: self.previous_token.range.clone(),
            })
        } else {
            let range = self.previous_token.range.clone();
            self.bindings.push(Binding { range });
            Ok(&self.bindings[self.bindings.len()])
        }
    }

    pub fn find_binding_stack_index(&self, name: &str) -> Option<u16> {
        let source = self.tokenizer.source;
        self.bindings
            .iter()
            .rposition(|b| &source[b.range.start as usize..b.range.end as usize] == name)
            .map(|i| i as _)
    }

    pub fn patch_statement(&mut self, node_index: usize, next_index: usize) {
        match &mut self.ast[node_index] {
            CommandAstNode::CommandCall { next, .. }
            | CommandAstNode::BindingDeclaration { next, .. }
            | CommandAstNode::MacroDeclaration { next, .. }
            | CommandAstNode::Return { next, .. } => *next = next_index as _,
            _ => unreachable!(),
        }
    }

    pub fn parse(&mut self) -> Result<(), CommandError> {
        self.ast.clear();
        self.bindings.clear();
        self.ast.push(CommandAstNode::Return {
            source: self.source_index,
            next: 0,
        });
        parse(self)?;
        Ok(())
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
            if let Some(path) = parser.path {
                buf.push(path);
            }
            buf.push(path);
            buf
        };

        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(_) => {
                return Err(CommandError {
                    kind: CommandErrorKind::CouldNotSourceFile,
                    source: parser.source_index,
                    range: parser.previous_token.range.clone(),
                })
            }
        };

        let source_index = parser.paths.index_of(&path);
        if source_index > u8::MAX as _ {
            return Err(CommandError {
                kind: CommandErrorKind::TooManySources,
                source: parser.source_index,
                range: parser.previous_token.range.clone(),
            });
        }

        parser.next_token()?;

        let previous_bindings_len = parser.bindings.len();
        let mut parser = Parser::new(
            &source,
            Some(&path),
            parser.paths,
            parser.ast,
            parser.bindings,
            source_index as _,
        )?;
        parse(&mut parser)?;
        parser.bindings.truncate(previous_bindings_len);
        Ok(())
    }

    fn parse_macro(parser: &mut Parser) -> Result<(), CommandError> {
        parser.next_token()?;
        parser.consume_token(CommandTokenKind::Literal)?;

        let index = parser.ast.len();
        parser.ast.push(CommandAstNode::MacroDeclaration {
            source: parser.source_index,
            name: parser.previous_token.range.clone(),
            param_count: 0,
            next: 0,
        });
        parser.next_token()?;

        let previous_bindings_len = parser.bindings.len();
        loop {
            match parser.previous_token.kind {
                CommandTokenKind::OpenCurlyBrackets => {
                    match &mut parser.ast[index] {
                        CommandAstNode::MacroDeclaration { param_count, .. } => {
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
                        source: parser.source_index,
                        range: parser.previous_token.range.clone(),
                    })
                }
            }
        }

        let mut previous_statement = 0;
        while parser.previous_token.kind != CommandTokenKind::CloseCurlyBrackets {
            let next_statement = parse_statement(parser, false)?;
            parser.patch_statement(previous_statement, next_statement);
            previous_statement = next_statement;
        }
        parser.next_token()?;

        parser.bindings.clear();
        Ok(())
    }

    fn parse_return(parser: &mut Parser, is_top_level: bool) -> Result<(), CommandError> {
        parser.ast.push(CommandAstNode::Return {
            source: parser.source_index,
            next: 0,
        });
        parser.next_token()?;
        parse_expression(parser, is_top_level)?;
        Ok(())
    }

    fn parse_statement(parser: &mut Parser, is_top_level: bool) -> Result<usize, CommandError> {
        let index = parser.ast.len();
        loop {
            match parser.previous_token.kind {
                CommandTokenKind::Literal | CommandTokenKind::OpenParenthesis => {
                    parse_command_call(parser, is_top_level)?;
                    break;
                }
                CommandTokenKind::Binding => {
                    if is_top_level {
                        return Err(CommandError {
                            kind: CommandErrorKind::InvalidBindingDeclarationAtTopLevel,
                            source: parser.source_index,
                            range: parser.previous_token.range.clone(),
                        });
                    }

                    let binding = parser.declare_binding_from_previous_token()?;
                    let name = binding.range.clone();
                    parser.ast.push(CommandAstNode::BindingDeclaration {
                        source: parser.source_index,
                        name,
                        next: 0,
                    });

                    parser.next_token()?;
                    parser.consume_token(CommandTokenKind::Equals)?;

                    parse_expression(parser, is_top_level)?;
                    break;
                }
                CommandTokenKind::EndOfCommand => {
                    parser.next_token()?;
                }
                _ => {
                    return Err(CommandError {
                        kind: CommandErrorKind::ExpectedStatement,
                        source: parser.source_index,
                        range: parser.previous_token.range.clone(),
                    })
                }
            }
        }
        Ok(index)
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
                source: parser.source_index,
                range: parser.previous_token.range.clone(),
            })
        }

        let end_token_kind = find_end_token_kind(parser)?;

        let index = parser.ast.len();
        parser.ast.push(CommandAstNode::CommandCall {
            source: parser.source_index,
            name: parser.previous_token.range.clone(),
            first_arg: 0,
            first_flag: 0,
            next: 0,
        });

        parser.next_token()?;

        let mut arg_count = 0;
        let mut last_arg = 0;
        let mut flag_count = 0;
        let mut last_flag = 0;
        loop {
            if parser.previous_token.kind == CommandTokenKind::Flag {
                let range_start = parser.previous_token.range.start;

                let len = parser.ast.len() as _;
                if flag_count == 0 {
                    match &mut parser.ast[index] {
                        CommandAstNode::CommandCall { first_flag, .. } => *first_flag = len,
                        _ => unreachable!(),
                    }
                }
                if let CommandAstNode::CommandCallFlag { next, .. } = &mut parser.ast[last_flag] {
                    *next = len;
                }
                last_flag = parser.ast.len();
                parser.ast.push(CommandAstNode::CommandCallFlag {
                    name: parser.previous_token.range.clone(),
                    next: 0,
                });

                parser.next_token()?;
                match parser.previous_token.kind {
                    CommandTokenKind::Equals => {
                        parser.next_token()?;
                        parse_expression(parser, is_top_level)?;
                    }
                    _ => parser.ast.push(CommandAstNode::Literal(0..0)),
                }

                if flag_count == u8::MAX {
                    let range_end = parser.previous_token.range.start;
                    return Err(CommandError {
                        kind: CommandErrorKind::TooManyFlags,
                        source: parser.source_index,
                        range: range_start..range_end,
                    });
                }
                flag_count += 1;
            } else if parser.previous_token.kind == end_token_kind {
                parser.next_token()?;
                break;
            } else {
                let range_start = parser.previous_token.range.start;

                let len = parser.ast.len() as _;
                if arg_count == 0 {
                    match &mut parser.ast[index] {
                        CommandAstNode::CommandCall { first_arg, .. } => *first_arg = len,
                        _ => unreachable!(),
                    }
                }
                if let CommandAstNode::CommandCallArg { next, .. } = &mut parser.ast[last_arg] {
                    *next = len;
                }
                last_arg = parser.ast.len();
                parser.ast.push(CommandAstNode::CommandCallArg { next: 0 });

                parse_expression(parser, is_top_level)?;

                if arg_count == u8::MAX {
                    let range_end = parser.previous_token.range.start;
                    return Err(CommandError {
                        kind: CommandErrorKind::WrongNumberOfArgs,
                        source: parser.source_index,
                        range: range_start..range_end,
                    });
                }
                arg_count += 1;
            }
        }

        Ok(())
    }

    fn parse_expression(parser: &mut Parser, is_top_level: bool) -> Result<(), CommandError> {
        fn consume_literal_range(parser: &mut Parser) -> Result<Range<u32>, CommandError> {
            let range = parser.previous_token.range.clone();
            if range.end - range.start <= u8::MAX as _ {
                parser.next_token()?;
                Ok(range.start as _..range.end as _)
            } else {
                Err(CommandError {
                    kind: CommandErrorKind::LiteralTooBig,
                    source: parser.source_index,
                    range,
                })
            }
        }

        while let CommandTokenKind::EndOfCommand = parser.previous_token.kind {
            parser.next_token()?;
        }

        match parser.previous_token.kind {
            CommandTokenKind::Literal => {
                let range = consume_literal_range(parser)?;
                parser.ast.push(CommandAstNode::Literal(range));
                Ok(())
            }
            CommandTokenKind::QuotedLiteral => {
                let range = consume_literal_range(parser)?;
                parser.ast.push(CommandAstNode::QuotedLiteral(range));
                Ok(())
            }
            CommandTokenKind::OpenParenthesis => parse_command_call(parser, is_top_level),
            CommandTokenKind::Binding => {
                let binding_name = parser.previous_token_str();
                match parser.find_binding_stack_index(binding_name) {
                    Some(index) => {
                        parser.next_token()?;
                        parser.ast.push(CommandAstNode::Binding(index));
                        Ok(())
                    }
                    None => Err(CommandError {
                        kind: CommandErrorKind::UndeclaredBinding,
                        source: parser.source_index,
                        range: parser.previous_token.range.clone(),
                    }),
                }
            }
            _ => Err(CommandError {
                kind: CommandErrorKind::ExpectedExpression,
                source: parser.source_index,
                range: parser.previous_token.range.clone(),
            }),
        }
    }

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

fn find_command(commands: &CommandCollection, name: &str) -> Option<CommandSource> {
    if let Some(i) = commands.macro_commands.iter().position(|c| {
        let range = c.name_range.start as usize..c.name_range.end as usize;
        &commands.custom_command_names[range] == name
    }) {
        return Some(CommandSource::Macro(i));
    }

    if let Some(i) = commands.request_commands.iter().position(|c| {
        let range = c.name_range.start as usize..c.name_range.end as usize;
        &commands.custom_command_names[range] == name
    }) {
        return Some(CommandSource::Request(i));
    }

    if let Some(i) = commands
        .builtin_commands
        .iter()
        .position(|c| c.name == name || c.alias == name)
    {
        return Some(CommandSource::Builtin(i));
    }

    None
}

struct Compiler<'source, 'data> {
    pub sources: &'source [&'source str],
    pub ast: &'data [CommandAstNode],
    pub commands: &'data mut CommandCollection,
    pub virtual_machine: &'data mut VirtualMachine,
}
impl<'source, 'data> Compiler<'source, 'data> {
    pub fn emit_push_literal(&mut self, source_index: u8, range: Range<u32>) {
        let literal =
            &self.sources[source_index as usize][range.start as usize..range.end as usize];
        let start = self.virtual_machine.texts.len();
        self.virtual_machine.texts.push_str(literal);
        let len = self.virtual_machine.texts.len() - start;
        self.virtual_machine.ops.push(Op::PushLiteral {
            start: start as _,
            len: len as _,
        });
    }

    pub fn emit_push_escaped_literal(
        &mut self,
        source_index: u8,
        range: Range<u32>,
    ) -> Result<(), CommandError> {
        let start = self.virtual_machine.texts.len();

        let mut literal =
            &self.sources[source_index as usize][range.start as usize..range.end as usize];
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
                        source: source_index,
                        range,
                    })
                }
            }
        }
        self.virtual_machine.texts.push_str(literal);

        let len = self.virtual_machine.texts.len() - start;
        self.virtual_machine.ops.push(Op::PushLiteral {
            start: start as _,
            len: len as _,
        });
        Ok(())
    }
}

fn compile(compiler: &mut Compiler) -> Result<usize, CommandError> {
    fn emit_expression(
        compiler: &mut Compiler,
        source_index: u8,
        index: usize,
    ) -> Result<(), CommandError> {
        match compiler.ast[index] {
            CommandAstNode::Literal(ref range) => {
                compiler.emit_push_literal(source_index, range.clone());
            }
            CommandAstNode::QuotedLiteral(ref range) => {
                let mut range = range.clone();
                range.start += 1;
                range.end -= 1;
                compiler.emit_push_escaped_literal(source_index, range)?;
            }
            CommandAstNode::Binding(index) => {
                compiler.virtual_machine.ops.push(Op::DuplicateAt(index));
            }
            CommandAstNode::CommandCall {
                source,
                ref name,
                first_arg,
                first_flag,
                next,
            } => {
                let command_name = &compiler.sources[source_index as usize]
                    [name.start as usize..name.end as usize];
                let command_source = match find_command(compiler.commands, command_name) {
                    Some(source) => source,
                    None => {
                        return Err(CommandError {
                            kind: CommandErrorKind::NoSuchCommand,
                            source,
                            range: name.clone(),
                        });
                    }
                };

                compiler.virtual_machine.ops.push(Op::PrepareStackFrame);

                let mut arg = first_arg as usize;
                let mut flag = first_flag as usize;

                match command_source {
                    CommandSource::Builtin(i) => {
                        fn find_flag_index(
                            flags: &[&str],
                            source: &str,
                            source_index: u8,
                            range: Range<u32>,
                        ) -> Result<usize, CommandError> {
                            let name = &source[range.start as usize..range.end as usize];
                            for (i, &flag) in flags.iter().enumerate() {
                                if flag == name {
                                    return Ok(i);
                                }
                            }
                            Err(CommandError {
                                kind: CommandErrorKind::NoSuchFlag,
                                source: source_index,
                                range,
                            })
                        }

                        let mut flag_expressions = [0; u8::MAX as _];
                        let flags = compiler.commands.builtin_commands[i].flags;
                        while let CommandAstNode::CommandCallFlag { ref name, next } =
                            compiler.ast[flag]
                        {
                            let name_range = name.start as _..name.end as _;
                            let flag_index = find_flag_index(
                                flags,
                                compiler.sources[source_index as usize],
                                source,
                                name_range,
                            )?;
                            flag_expressions[flag_index] = flag + 1;
                            flag = next as _;
                        }

                        for &expression in &flag_expressions[..flags.len()] {
                            if expression == 0 {
                                compiler
                                    .virtual_machine
                                    .ops
                                    .push(Op::PushLiteral { start: 0, len: 0 });
                            } else {
                                emit_expression(compiler, source, expression)?;
                            }
                        }
                    }
                    _ => match compiler.ast[flag] {
                        CommandAstNode::CommandCallFlag { ref name, .. } => {
                            return Err(CommandError {
                                kind: CommandErrorKind::NoSuchFlag,
                                source,
                                range: name.clone(),
                            });
                        }
                        _ => (),
                    },
                }

                while let CommandAstNode::CommandCallArg { next } = compiler.ast[arg] {
                    emit_expression(compiler, source, arg + 1)?;
                    arg = next as _;
                }

                match command_source {
                    CommandSource::Builtin(i) => compiler
                        .virtual_machine
                        .ops
                        .push(Op::CallBuiltinCommand(i as _)),
                    CommandSource::Macro(i) => compiler
                        .virtual_machine
                        .ops
                        .push(Op::CallMacroCommand(i as _)),
                    CommandSource::Request(i) => compiler
                        .virtual_machine
                        .ops
                        .push(Op::CallRequestCommand(i as _)),
                }

                debug_assert_eq!(0, next);
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    fn emit_statement(compiler: &mut Compiler, index: usize) -> Result<usize, CommandError> {
        match compiler.ast[index] {
            CommandAstNode::CommandCall { source, next, .. } => {
                emit_expression(compiler, source, index)?;
                compiler.virtual_machine.ops.push(Op::Pop);
                Ok(next as _)
            }
            CommandAstNode::Return { source, next, .. } => {
                emit_expression(compiler, source, index + 1)?;
                compiler.virtual_machine.ops.push(Op::Return);
                Ok(next as _)
            }
            _ => unreachable!(),
        }
    }

    if compiler.ast.len() == 1 {
        return Ok(compiler.virtual_machine.ops.len());
    }

    let mut index = 1;
    while index != 0 {
        match compiler.ast[index] {
            CommandAstNode::CommandCall { next, .. } | CommandAstNode::Return { next, .. } => {
                index = next as _;
            }
            CommandAstNode::MacroDeclaration {
                source,
                ref name,
                param_count,
                next,
            } => {
                let command_name =
                    &compiler.sources[source as usize][name.start as usize..name.end as usize];
                if find_command(compiler.commands, command_name).is_some() {
                    return Err(CommandError {
                        kind: CommandErrorKind::CommandAlreadyExists,
                        source,
                        range: name.clone(),
                    });
                }

                let op_start_index = compiler.virtual_machine.ops.len() as _;
                index += 1;
                while index != 0 {
                    index = emit_statement(compiler, index)?;
                }

                let name_range = compiler.commands.add_custom_command_name(command_name);
                compiler.commands.macro_commands.push(MacroCommand {
                    name_range,
                    op_start_index,
                    param_count,
                });

                index = next as _;
            }
            _ => unreachable!(),
        }
    }

    let definitions_end = compiler.virtual_machine.ops.len();

    index = 1;
    while index != 0 {
        match compiler.ast[index] {
            CommandAstNode::MacroDeclaration { next, .. } => index = next as _,
            _ => index = emit_statement(compiler, index)?,
        }
    }

    compiler
        .virtual_machine
        .ops
        .push(Op::PushLiteral { start: 0, len: 0 });
    compiler.virtual_machine.ops.push(Op::Return);

    Ok(definitions_end)
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
    source: u8,
    start: u32,
    len: u16,
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
                        source: location.source,
                        range: location.start..location.start + location.len as u32,
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
        fn collect<'a>(source: &'a str) -> Vec<(CommandTokenKind, &'a str)> {
            let mut tokenizer = CommandTokenizer::new(source);
            let mut tokens = Vec::new();
            loop {
                let token = tokenizer.next().unwrap();
                match token.kind {
                    CommandTokenKind::EndOfSource => break,
                    _ => tokens.push((token.kind, &source[token.range()])),
                }
            }
            tokens
        }

        use CommandTokenKind::*;

        assert_eq!(vec![(EndOfCommand, "")], collect(""));
        assert_eq!(vec![(EndOfCommand, "")], collect("  "));
        assert_eq!(
            vec![(Literal, "command"), (EndOfCommand, "")],
            collect("command"),
        );
        assert_eq!(
            vec![(QuotedLiteral, "'text'"), (EndOfCommand, "")],
            collect("'text'"),
        );
        assert_eq!(
            vec![
                (Literal, "cmd"),
                (OpenParenthesis, "("),
                (Literal, "subcmd"),
                (CloseParenthesis, ")"),
                (EndOfCommand, ""),
            ],
            collect("cmd (subcmd)"),
        );
        assert_eq!(
            vec![
                (Literal, "cmd"),
                (Binding, "$binding"),
                (Flag, "-flag"),
                (Equals, "="),
                (Literal, "value"),
                (Equals, "="),
                (Literal, "not-flag"),
                (EndOfCommand, ""),
            ],
            collect("cmd $binding -flag=value = not-flag"),
        );
        assert_eq!(
            vec![
                (Literal, "cmd0"),
                (Literal, "cmd1"),
                (EndOfCommand, "\r\n\n \t \n  "),
                (Literal, "cmd2"),
                (EndOfCommand, ""),
            ],
            collect("cmd0 cmd1 \t\r\n\n \t \n  cmd2"),
        );
    }

    #[test]
    fn command_compiler() {
        fn compile(source: &str) -> Vec<Op> {
            let mut bindings = Vec::new();
            let builtin_commands = &[BuiltinCommand {
                name: "cmd",
                alias: "",
                hidden: false,
                completions: &[],
                flags: &["-switch", "-option"],
                func: || (),
            }];
            let mut macro_commands = MacroCommandCollection::default();
            let mut compiler = Compiler::new(
                source,
                None,
                &mut bindings,
                builtin_commands,
                &mut macro_commands,
            )
            .unwrap();
            let mut chunk = ByteCodeChunk::default();
            compiler.compile(&mut chunk).unwrap();
            chunk.ops
        }

        use Op::*;

        assert_eq!(
            vec![
                PrepareStackFrame { is_macro_chunk },
                PushLiteralReference { start: 0, len: 0 },
                Return
            ],
            compile("")
        );

        assert_eq!(
            vec![
                CallBuiltinCommand(0),
                Pop,
                PushLiteralReference { start: 0, len: 0 },
                Return
            ],
            compile("cmd"),
        );

        assert_eq!(
            vec![
                PushLiteralReference {
                    start: 0,
                    len: "arg0".len() as _,
                },
                PushLiteralReference {
                    start: "arg0".len() as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand(0),
                Pop,
                PushLiteralReference { start: 0, len: 0 },
                Return
            ],
            compile("cmd arg0 arg1"),
        );

        assert_eq!(
            vec![
                PushLiteralReference { start: 0, len: 0 },
                PopAsFlag(0),
                PushLiteralReference {
                    start: 0,
                    len: "arg".len() as _,
                },
                PushLiteralReference {
                    start: "arg".len() as _,
                    len: "opt".len() as _,
                },
                PopAsFlag(1),
                CallBuiltinCommand(0),
                Pop,
                PushLiteralReference { start: 0, len: 0 },
                Return
            ],
            compile("cmd -switch arg -option=opt"),
        );

        assert_eq!(
            vec![
                PushLiteralReference {
                    start: 0,
                    len: "arg0".len() as _,
                },
                PushLiteralReference {
                    start: "arg0".len() as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand(0),
                PopAsFlag(1),
                PushLiteralReference {
                    start: "arg0arg1".len() as _,
                    len: "arg2".len() as _,
                },
                CallBuiltinCommand(0),
                Pop,
                PushLiteralReference { start: 0, len: 0 },
                Return
            ],
            compile("cmd arg0 -option=(cmd arg1) arg2"),
        );

        assert_eq!(
            vec![
                PushLiteralReference {
                    start: 0,
                    len: "arg0".len() as _,
                },
                PushLiteralReference {
                    start: "arg0".len() as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand(0),
                Pop,
                PushLiteralReference { start: 0, len: 0 },
                Return
            ],
            compile("(cmd \n arg0 \n arg2)"),
        );
    }
}

