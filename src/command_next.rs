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

pub struct CommandCollection {
    builtin_commands: &'static [BuiltinCommand],
    custom_command_names: String,
    macro_commands: Vec<MacroCommand>,
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
        }
    }
}

#[derive(Default)]
pub struct CommandManager {
    commands: CommandCollection,
    temp_ast: Vec<CommandAstNode>,
    temp_bindings: Vec<Binding>,
    virtual_machine: VirtualMachine,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandCompileErrorKind {
    UnterminatedQuotedLiteral,
    InvalidFlagName,
    InvalidBindingName,

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
    TooManyArgs,
    TooManyFlags,
    CouldNotSourceFile,
    CommandAlreadyExists,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandCompileError {
    pub kind: CommandCompileErrorKind,
    pub range: Range<usize>,
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
    pub range: Range<usize>,
}

pub struct CommandTokenizer<'a> {
    pub source: &'a str,
    index: usize,
}
impl<'a> CommandTokenizer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source, index: 0 }
    }

    pub fn next(&mut self) -> Result<CommandToken, CommandCompileError> {
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
                range: from..iter.index,
            }
        }

        let source_bytes = self.source.as_bytes();

        loop {
            loop {
                if self.index == source_bytes.len() {
                    self.index += 1;
                    return Ok(CommandToken {
                        kind: CommandTokenKind::EndOfCommand,
                        range: source_bytes.len()..source_bytes.len(),
                    });
                }
                if self.index > source_bytes.len() {
                    return Ok(CommandToken {
                        kind: CommandTokenKind::EndOfSource,
                        range: source_bytes.len()..source_bytes.len(),
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
                            return Err(CommandCompileError {
                                kind: CommandCompileErrorKind::UnterminatedQuotedLiteral,
                                range: from..source_bytes.len(),
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
                        range: from..self.index,
                    });
                }
                b'-' => {
                    let from = self.index;
                    self.index += 1;
                    consume_identifier(self);
                    let range = from..self.index;
                    if range.start + 1 == range.end {
                        return Err(CommandCompileError {
                            kind: CommandCompileErrorKind::InvalidFlagName,
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
                    let range = from..self.index;
                    if range.start + 1 == range.end {
                        return Err(CommandCompileError {
                            kind: CommandCompileErrorKind::InvalidBindingName,
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
                        range: from..self.index,
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
                        range: from..self.index,
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
        name: Range<u32>,
        next: u16,
    },
    MacroDeclaration {
        name: Range<u32>,
        param_count: u8,
        next: u16,
    },
    Return {
        next: u16,
    },
}

struct Binding {
    pub range: Range<u32>,
}

struct Parser<'source, 'data> {
    tokenizer: CommandTokenizer<'source>,
    pub path: Option<&'data Path>,
    pub ast: &'data mut Vec<CommandAstNode>,
    pub bindings: &'data mut Vec<Binding>,
    pub previous_token: CommandToken,
}
impl<'source, 'data> Parser<'source, 'data> {
    pub fn new(
        source: &'source str,
        path: Option<&'data Path>,
        ast: &'data mut Vec<CommandAstNode>,
        bindings: &'data mut Vec<Binding>,
    ) -> Result<Self, CommandCompileError> {
        let mut tokenizer = CommandTokenizer::new(source);
        let previous_token = tokenizer.next()?;
        Ok(Self {
            tokenizer,
            path,
            ast,
            bindings,
            previous_token,
        })
    }

    pub fn previous_token_range(&self) -> Range<u32> {
        self.previous_token.range.start as u32..self.previous_token.range.end as u32
    }

    pub fn previous_token_str(&self) -> &'source str {
        &self.tokenizer.source[self.previous_token.range.clone()]
    }

    pub fn next_token(&mut self) -> Result<(), CommandCompileError> {
        let token = self.tokenizer.next()?;
        self.previous_token = token;
        Ok(())
    }

    pub fn consume_token(&mut self, kind: CommandTokenKind) -> Result<(), CommandCompileError> {
        if self.previous_token.kind == kind {
            self.next_token()
        } else {
            Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedToken(kind),
                range: self.previous_token.range.clone(),
            })
        }
    }

    pub fn declare_binding_from_previous_token(&mut self) -> Result<&Binding, CommandCompileError> {
        if self.bindings.len() >= u16::MAX as _ {
            Err(CommandCompileError {
                kind: CommandCompileErrorKind::TooManyBindings,
                range: self.previous_token.range.clone(),
            })
        } else {
            let range = self.previous_token_range();
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
            | CommandAstNode::Return { next } => *next = next_index as _,
            _ => unreachable!(),
        }
    }

    pub fn parse(&mut self) -> Result<(), CommandCompileError> {
        self.ast.clear();
        self.ast.push(CommandAstNode::Return { next: 0 });
        self.bindings.clear();
        parse(self)
    }
}

fn parse(parser: &mut Parser) -> Result<(), CommandCompileError> {
    fn parse_top_level(parser: &mut Parser) -> Result<(), CommandCompileError> {
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

    fn parse_source(parser: &mut Parser) -> Result<(), CommandCompileError> {
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
                return Err(CommandCompileError {
                    kind: CommandCompileErrorKind::CouldNotSourceFile,
                    range: parser.previous_token.range.clone(),
                })
            }
        };

        let previous_bindings_len = parser.bindings.len();
        let mut parser = Parser::new(&source, Some(&path), parser.ast, parser.bindings)?;
        parse(&mut parser)?;
        parser.bindings.truncate(previous_bindings_len);
        Ok(())
    }

    fn parse_macro(parser: &mut Parser) -> Result<(), CommandCompileError> {
        parser.next_token()?;
        parser.consume_token(CommandTokenKind::Literal)?;

        let index = parser.ast.len();
        parser.ast.push(CommandAstNode::MacroDeclaration {
            name: parser.previous_token_range(),
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
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::ExpectedToken(
                            CommandTokenKind::OpenCurlyBrackets,
                        ),
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

    fn parse_return(parser: &mut Parser, is_top_level: bool) -> Result<(), CommandCompileError> {
        parser.ast.push(CommandAstNode::Return { next: 0 });
        parser.next_token()?;
        parse_expression(parser, is_top_level)?;
        Ok(())
    }

    fn parse_statement(
        parser: &mut Parser,
        is_top_level: bool,
    ) -> Result<usize, CommandCompileError> {
        let index = parser.ast.len();
        loop {
            match parser.previous_token.kind {
                CommandTokenKind::Literal | CommandTokenKind::OpenParenthesis => {
                    parse_command_call(parser, is_top_level)?;
                    break;
                }
                CommandTokenKind::Binding => {
                    if is_top_level {
                        return Err(CommandCompileError {
                            kind: CommandCompileErrorKind::InvalidBindingDeclarationAtTopLevel,
                            range: parser.previous_token.range.clone(),
                        });
                    }

                    let binding = parser.declare_binding_from_previous_token()?;
                    let name = binding.range.clone();
                    parser.ast.push(CommandAstNode::BindingDeclaration {
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
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::ExpectedStatement,
                        range: parser.previous_token.range.clone(),
                    })
                }
            }
        }
        Ok(index)
    }

    fn parse_command_call(
        parser: &mut Parser,
        is_top_level: bool,
    ) -> Result<(), CommandCompileError> {
        fn find_end_token_kind(
            parser: &mut Parser,
        ) -> Result<CommandTokenKind, CommandCompileError> {
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

            Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedToken(CommandTokenKind::Literal),
                range: parser.previous_token.range.clone(),
            })
        }

        let end_token_kind = find_end_token_kind(parser)?;

        let index = parser.ast.len();
        parser.ast.push(CommandAstNode::CommandCall {
            name: parser.previous_token_range(),
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
                    name: parser.previous_token_range(),
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
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::TooManyFlags,
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
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::TooManyArgs,
                        range: range_start..range_end,
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
    ) -> Result<(), CommandCompileError> {
        fn consume_literal_range(parser: &mut Parser) -> Result<Range<u32>, CommandCompileError> {
            let range = parser.previous_token.range.clone();
            if range.end - range.start <= u8::MAX as _ {
                parser.next_token()?;
                Ok(range.start as _..range.end as _)
            } else {
                Err(CommandCompileError {
                    kind: CommandCompileErrorKind::LiteralTooBig,
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
                    None => Err(CommandCompileError {
                        kind: CommandCompileErrorKind::UndeclaredBinding,
                        range: parser.previous_token.range.clone(),
                    }),
                }
            }
            _ => Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedExpression,
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

    // TODO: implement for request command
    /*
    if let Some(i) = commands.request_commands.iter().position(|c| c.name == name) {
        return Some(CommandSource::Request(i));
    }
    */

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
    source: &'source str,
    ast: &'data [CommandAstNode],
    commands: &'data mut CommandCollection,
    virtual_machine: &'data mut VirtualMachine,
}

fn compile(compiler: &mut Compiler) -> Result<(), CommandCompileError> {
    fn emit_expression(compiler: &mut Compiler) -> usize {
        0
    }

    fn emit_statement(compiler: &mut Compiler) -> usize {
        0
    }

    if compiler.ast.len() == 1 {
        return Ok(());
    }

    let mut index = 1;
    while index != 0 {
        match compiler.ast[index] {
            CommandAstNode::CommandCall { next, .. } | CommandAstNode::Return { next } => {
                index = next as _;
            }
            CommandAstNode::MacroDeclaration {
                ref name,
                param_count,
                next,
            } => {
                let name_range = name.start as usize..name.end as usize;
                let name = &compiler.source[name_range.clone()];
                if find_command(compiler.commands, name).is_some() {
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::CommandAlreadyExists,
                        range: name_range,
                    });
                }

                let op_start_index = compiler.virtual_machine.ops.len() as _;
                index += 1;
                while index != 0 {
                    index = emit_statement(compiler);
                }

                let name_range = compiler.commands.add_custom_command_name(name);
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

    index = 1;
    while index != 0 {
        match compiler.ast[index] {
            CommandAstNode::CommandCall {
                ref name,
                first_arg,
                first_flag,
                next,
            } => {
                let name_range = name.start as usize..name.end as usize;
                let name = &compiler.source[name_range.clone()];
                let command_source = match find_command(compiler.commands, name) {
                    Some(source) => source,
                    None => {
                        return Err(CommandCompileError {
                            kind: CommandCompileErrorKind::NoSuchCommand,
                            range: name_range,
                        });
                    }
                };

                let mut arg = first_arg as usize;
                let mut flag = first_flag as usize;

                if flag != 0 && !matches!(command_source, CommandSource::Builtin(_)) {
                    let name = match compiler.ast[flag] {
                        CommandAstNode::CommandCallFlag { ref name, .. } => name,
                        _ => unreachable!(),
                    };
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::NoSuchFlag,
                        range: name.start as usize..name.end as usize,
                    });
                }

                while flag != 0 {
                    flag = emit_statement(compiler);
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

                index = next as _;
            }
            CommandAstNode::Return { next } => {
                compiler.virtual_machine.ops.push(Op::Return);
                index = next as _;
            }
            CommandAstNode::MacroDeclaration { next, .. } => index = next as _,
            _ => unreachable!(),
        }
    }

    compiler.virtual_machine.ops.push(Op::PushLiteral { start: 0, len: 0 });
    compiler.virtual_machine.ops.push(Op::Return);
    Ok(())
}


/*
fn compile(compiler: &mut Compiler, chunk: &mut ByteCodeChunk) -> Result<(), CommandCompileError> {
    fn parse_top_level(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        if let CommandTokenKind::Literal = compiler.previous_token.kind {
            match compiler.previous_token_str() {
                "source" => return parse_source(compiler, chunk),
                "macro" => return parse_macro(compiler, chunk),
                "return" => return parse_return(compiler, chunk, true),
                _ => (),
            }
        }

        parse_statement(compiler, chunk, true)
    }

    fn parse_source(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        compiler.next_token()?;
        compiler.consume_token(CommandTokenKind::QuotedLiteral)?;

        let path = Path::new(compiler.previous_token_str());
        let path = if path.is_absolute() {
            path.into()
        } else {
            let mut buf = PathBuf::new();
            if let Some(path) = compiler.path {
                buf.push(path);
            }
            buf.push(path);
            buf
        };

        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(_) => {
                return Err(CommandCompileError {
                    kind: CommandCompileErrorKind::CouldNotSourceFile,
                    range: compiler.previous_token.range.clone(),
                })
            }
        };

        let mut compiler = Compiler::new(
            &source,
            Some(&path),
            compiler.bindings,
            compiler.builtin_commands,
            compiler.macro_commands,
        )?;
        compiler.compile(chunk)?;
        Ok(())
    }

    fn parse_macro(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        compiler.next_token()?;
        compiler.consume_token(CommandTokenKind::Literal)?;
        let name = compiler.previous_token_str();
        compiler.next_token()?;

        debug_assert!(compiler.bindings.is_empty());

        loop {
            match compiler.previous_token.kind {
                CommandTokenKind::OpenCurlyBrackets => {
                    compiler.next_token()?;
                    break;
                }
                CommandTokenKind::Binding => {
                    compiler.declare_binding(compiler.previous_token.range.clone())?;
                    compiler.next_token()?;
                }
                _ => {
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::ExpectedToken(
                            CommandTokenKind::OpenCurlyBrackets,
                        ),
                        range: compiler.previous_token.range.clone(),
                    })
                }
            }
        }

        while compiler.previous_token.kind != CommandTokenKind::CloseCurlyBrackets {
            parse_statement(compiler, chunk, false)?;
        }
        compiler.next_token()?;

        // TODO: register macro command

        compiler.bindings.clear();
        if chunk.ops.last() != Some(&Op::Return) {
            chunk.emit(Op::PushLiteralReference { start: 0, len: 0 });
            chunk.emit(Op::Return);
        }
        Ok(())
    }

    fn parse_return(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
        is_top_level: bool,
    ) -> Result<(), CommandCompileError> {
        compiler.next_token()?;
        parse_expression(compiler, chunk, is_top_level)?;
        chunk.emit(Op::Return);
        Ok(())
    }

    fn parse_statement(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
        is_top_level: bool,
    ) -> Result<(), CommandCompileError> {
        match compiler.previous_token.kind {
            CommandTokenKind::Literal | CommandTokenKind::OpenParenthesis => {
                parse_command_call(compiler, chunk, is_top_level)?;
                chunk.emit(Op::Pop);
                Ok(())
            }
            CommandTokenKind::Binding => {
                if is_top_level {
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::InvalidBindingDeclarationAtTopLevel,
                        range: compiler.previous_token.range.clone(),
                    });
                }

                compiler.declare_binding(compiler.previous_token.range.clone())?;
                compiler.next_token()?;
                compiler.consume_token(CommandTokenKind::Equals)?;

                parse_expression(compiler, chunk, is_top_level)?;
                Ok(())
            }
            CommandTokenKind::EndOfCommand => {
                compiler.next_token()?;
                Ok(())
            }
            _ => Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedStatement,
                range: compiler.previous_token.range.clone(),
            }),
        }
    }

    fn parse_command_call(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
        is_top_level: bool,
    ) -> Result<(), CommandCompileError> {
        fn find_command_from_previous_token(
            compiler: &Compiler,
        ) -> Result<CommandSource, CommandCompileError> {
            let command_name = compiler.previous_token_str();

            if let Some(i) = compiler.macro_commands.commands.iter().position(|c| {
                let range = c.name_range.start as usize..c.name_range.end as usize;
                &compiler.macro_commands.names[range] == command_name
            }) {
                return Ok(CommandSource::Macro(i));
            }


            if let Some(i) = compiler
                .builtin_commands
                .iter()
                .position(|c| c.alias == command_name || c.name == command_name)
            {
                return Ok(CommandSource::Builtin(i));
            }

            Err(CommandCompileError {
                kind: CommandCompileErrorKind::NoSuchCommand,
                range: compiler.previous_token.range.clone(),
            })
        }

        fn find_flag_index_from_previous_token(
            compiler: &Compiler,
            command_source: CommandSource,
        ) -> Result<usize, CommandCompileError> {
            if let CommandSource::Builtin(i) = command_source {
                let flag_name = compiler.previous_token_str();
                for (i, &flag) in compiler.builtin_commands[i].flags.iter().enumerate() {
                    if flag == flag_name {
                        return Ok(i);
                    }
                }
            }

            Err(CommandCompileError {
                kind: CommandCompileErrorKind::NoSuchFlag,
                range: compiler.previous_token.range.clone(),
            })
        }

        fn find_end_token_kind(
            compiler: &mut Compiler,
        ) -> Result<CommandTokenKind, CommandCompileError> {
            match compiler.previous_token.kind {
                CommandTokenKind::Literal => return Ok(CommandTokenKind::EndOfCommand),
                CommandTokenKind::OpenParenthesis => {
                    compiler.next_token()?;
                    if let CommandTokenKind::Literal = compiler.previous_token.kind {
                        return Ok(CommandTokenKind::CloseParenthesis);
                    }
                }
                _ => (),
            }

            Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedToken(CommandTokenKind::Literal),
                range: compiler.previous_token.range.clone(),
            })
        }

        let end_token_kind = find_end_token_kind(compiler)?;
        let command_source = find_command_from_previous_token(compiler)?;
        compiler.next_token()?;

        chunk.emit(Op::PrepareStackFrame {
            is_macro_chunk: matches!(command_source, CommandSource::Macro(_)),
        });

        let mut arg_count = 0;
        let mut flag_count = 0;
        loop {
            if compiler.previous_token.kind == CommandTokenKind::Flag {
                let range_start = compiler.previous_token.range.start;
                let flag_index =
                    find_flag_index_from_previous_token(compiler, command_source)? as _;
                compiler.next_token()?;

                match compiler.previous_token.kind {
                    CommandTokenKind::Equals => {
                        compiler.next_token()?;
                        parse_expression(compiler, chunk, is_top_level)?;
                    }
                    _ => chunk.emit(Op::PushLiteralReference { start: 0, len: 0 }),
                }

                if flag_count == u8::MAX {
                    let range_end = compiler.previous_token.range.start;
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::TooManyFlags,
                        range: range_start..range_end,
                    });
                }
                flag_count += 1;

                chunk.emit(Op::PopAsFlag(flag_index));
            } else if compiler.previous_token.kind == end_token_kind {
                compiler.next_token()?;
                break;
            } else {
                let range_start = compiler.previous_token.range.start;
                parse_expression(compiler, chunk, is_top_level)?;

                if arg_count == u8::MAX {
                    let range_end = compiler.previous_token.range.start;
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::TooManyArgs,
                        range: range_start..range_end,
                    });
                }
                arg_count += 1;
            }
        }

        match command_source {
            CommandSource::Builtin(i) => chunk.emit(Op::CallBuiltinCommand(i as _)),
            CommandSource::Macro(i) => chunk.emit(Op::CallMacroCommand(i as _)),
            CommandSource::Request(i) => chunk.emit(Op::CallRequestCommand(i as _)),
        };

        Ok(())
    }

    fn parse_expression(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
        is_top_level: bool,
    ) -> Result<(), CommandCompileError> {
        while let CommandTokenKind::EndOfCommand = compiler.previous_token.kind {
            compiler.next_token()?;
        }

        let range_start = compiler.previous_token.range.start;
        match compiler.previous_token.kind {
            CommandTokenKind::Literal => {
                let literal = compiler.previous_token_str();
                let literal = chunk.add_literal(literal, compiler.previous_token.range.clone())?;
                let op = match is_top_level {
                    true => Op::PushLiteralReference {
                        start: literal.start,
                        len: literal.len,
                    },
                    false => Op::PushLiteralCopy {
                        start: literal.start,
                        len: literal.len,
                    },
                };
                chunk.emit(op);
                compiler.next_token()?;
                Ok(())
            }
            CommandTokenKind::QuotedLiteral => {
                let literal = compiler.previous_token_str();
                let literal = &literal[1..];
                let literal = &literal[..literal.len() - 1];
                let literal =
                    chunk.add_escaped_literal(literal, compiler.previous_token.range.clone())?;
                chunk.emit(Op::PushLiteralReference {
                    start: literal.start,
                    len: literal.len,
                });
                compiler.next_token()?;
                Ok(())
            }
            CommandTokenKind::OpenParenthesis => parse_command_call(compiler, chunk, is_top_level),
            CommandTokenKind::Binding => {
                let binding_name = compiler.previous_token_str();
                match compiler.find_binding_stack_index(binding_name) {
                    Some(index) => {
                        compiler.next_token()?;
                        chunk.emit(Op::PushFromStack(index));
                        Ok(())
                    }
                    None => Err(CommandCompileError {
                        kind: CommandCompileErrorKind::UndeclaredBinding,
                        range: compiler.previous_token.range.clone(),
                    }),
                }
            }
            _ => Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedExpression,
                range: range_start..compiler.previous_token.range.end,
            }),
        }
    }

    while compiler.previous_token.kind != CommandTokenKind::EndOfSource {
        parse_top_level(compiler, chunk)?;
    }
    Ok(())
}
*/

const _ASSERT_OP_SIZE: [(); 4] = [(); std::mem::size_of::<Op>()];

#[derive(Debug, PartialEq, Eq)]
enum Op {
    Return,
    Pop,
    PushLiteral { start: u16, len: u8 },
    PushFromStack(u16),
    PopAsFlag(u8),
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
    value_stack_len: u16,
    flag_stack_len: u16,
}

#[derive(Default)]
struct VirtualMachine {
    ops: Vec<Op>,
    texts: String,
    flag_stack: Vec<StackFlag>,
    value_stack: Vec<StackValue>,
    stack_frames: Vec<StackFrame>,
    prepared_stack_frames: Vec<StackFrame>,
}
impl VirtualMachine {
    pub fn add_literal(&mut self, text: &str) -> Op {
        let start = self.texts.len();
        self.texts.push_str(text);
        let len = self.texts.len() - start;
        Op::PushLiteral {
            start: start as _,
            len: len as _,
        }
    }

    pub fn add_escaped_literal(
        &mut self,
        mut text: &str,
        range: Range<usize>,
    ) -> Result<Op, CommandCompileError> {
        let start = self.texts.len();
        while let Some(i) = text.find('\\') {
            self.texts.push_str(&text[..i]);
            text = &text[i + 1..];
            match text.as_bytes() {
                &[b'\\', ..] => self.texts.push('\\'),
                &[b'\'', ..] => self.texts.push('\''),
                &[b'\"', ..] => self.texts.push('\"'),
                &[b'\n', ..] => self.texts.push('\n'),
                &[b'\r', ..] => self.texts.push('\r'),
                &[b'\t', ..] => self.texts.push('\t'),
                &[b'\0', ..] => self.texts.push('\0'),
                _ => {
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::InvalidLiteralEscaping,
                        range,
                    })
                }
            }
        }

        self.texts.push_str(text);
        let len = self.texts.len() - start;
        Ok(Op::PushLiteral {
            start: start as _,
            len: len as _,
        })
    }
}

fn execute(
    editor: &mut Editor,
    platform: &mut Platform,
    clients: &mut ClientManager,
    client_handle: Option<ClientHandle>,
) {
    let mut vm = &mut editor.commands_next.virtual_machine;
    let mut op_index = 0;

    loop {
        match vm.ops[op_index] {
            Op::Return => {
                todo!();
            }
            Op::Pop => drop(vm.value_stack.pop()),
            Op::PushLiteral { start, len } => {
                let start = start as usize;
                let end = start + len as usize;
                vm.value_stack.push(StackValue {
                    start: start as _,
                    end: end as _,
                });
            }
            Op::PushFromStack(stack_index) => {
                let value = vm.value_stack[stack_index as usize];
                vm.value_stack.push(value);
            }
            Op::PopAsFlag(flag_index) => {
                let value = match vm.value_stack.pop() {
                    Some(value) => value,
                    None => unreachable!(),
                };
                vm.flag_stack.push(StackFlag {
                    start: value.start,
                    end: value.end,
                    flag_index,
                });
            }
            Op::PrepareStackFrame => {
                let frame = StackFrame {
                    op_index: op_index as _,
                    texts_len: vm.texts.len() as _,
                    value_stack_len: vm.value_stack.len() as _,
                    flag_stack_len: vm.flag_stack.len() as _,
                };
                vm.prepared_stack_frames.push(frame);
            }
            Op::CallBuiltinCommand(index) => {
                let frame = vm.prepared_stack_frames.pop().unwrap();
                let command_fn = &editor.commands_next.commands.builtin_commands[index as usize].func;
                command_fn();

                vm = &mut editor.commands_next.virtual_machine;

                vm.value_stack.push(StackValue {
                    start: 0,
                    end: vm.texts.len() as _,
                });

                vm.stack_frames.push(frame);
            }
            Op::CallMacroCommand(index) => {
                todo!();
            }
            Op::CallRequestCommand(index) => {
                todo!();
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
                    _ => tokens.push((token.kind, &source[token.range])),
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

