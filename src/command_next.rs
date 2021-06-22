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

#[derive(Debug, PartialEq, Eq)]
struct LiteralValue {
    pub start: u16,
    pub len: u8,
}

const _ASSERT_OP_SIZE: [(); 4] = [(); std::mem::size_of::<Op>()];

#[derive(Debug, PartialEq, Eq)]
enum Op {
    Return,
    Pop,
    PushLiteral { start: u16, len: u8 },
    PushFromStack(u16),
    PopAsFlag(u8),
    PrepareStackFrame { is_macro_chunk: bool },
    CallBuiltinCommand(u8),
    CallMacroCommand(u16),
    CallRequestCommand(u16),
}

#[derive(Clone, Copy)]
pub enum CommandSource {
    Builtin(usize),
    Macro(usize),
    Request(usize),
}

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
    params_len: u8,
}

#[derive(Default)]
struct MacroCommandCollection {
    names: String,
    chunk: ByteCodeChunk,
    commands: Vec<MacroCommand>,
}

pub struct CommandManager {
    builtin_commands: &'static [BuiltinCommand],
    macro_commands: MacroCommandCollection,

    temp_chunk: ByteCodeChunk,
    virtual_machine: VirtualMachine,
}
impl CommandManager {
    pub fn new() -> Self {
        Self {
            builtin_commands: &[],
            macro_commands: MacroCommandCollection::default(),

            temp_chunk: ByteCodeChunk::default(),
            virtual_machine: VirtualMachine::default(),
        }
    }
}

#[derive(Default)]
struct ByteCodeChunk {
    ops: Vec<Op>,
    texts: String,
}
impl ByteCodeChunk {
    pub fn clear(&mut self) {
        self.ops.clear();
        self.texts.clear();
    }

    pub fn emit(&mut self, op: Op) {
        self.ops.push(op);
    }

    pub fn add_literal(
        &mut self,
        text: &str,
        range: Range<usize>,
    ) -> Result<LiteralValue, CommandCompileError> {
        if text.len() > u8::MAX as _ {
            return Err(CommandCompileError {
                kind: CommandCompileErrorKind::LiteralTooBig,
                range,
            });
        }

        let start = self.texts.len() as _;
        let len = text.len() as _;
        self.texts.push_str(text);
        Ok(LiteralValue { start, len })
    }

    pub fn add_escaped_literal(
        &mut self,
        mut text: &str,
        range: Range<usize>,
    ) -> Result<LiteralValue, CommandCompileError> {
        if text.len() > u8::MAX as _ {
            return Err(CommandCompileError {
                kind: CommandCompileErrorKind::LiteralTooBig,
                range,
            });
        }

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

        let len = (self.texts.len() - start) as _;
        let start = start as _;
        Ok(LiteralValue { start, len })
    }
}

struct Binding {
    range: Range<u32>,
}

struct Compiler<'source, 'state> {
    tokenizer: CommandTokenizer<'source>,
    path: Option<&'state Path>,
    pub previous_token: CommandToken,
    pub bindings: &'state mut Vec<Binding>,
    bindings_previous_len: usize,
    builtin_commands: &'static [BuiltinCommand],
    macro_commands: &'state mut MacroCommandCollection,
}
impl<'source, 'state> Compiler<'source, 'state> {
    pub fn new(
        source: &'source str,
        path: Option<&'state Path>,
        bindings: &'state mut Vec<Binding>,
        builtin_commands: &'static [BuiltinCommand],
        macro_commands: &'state mut MacroCommandCollection,
    ) -> Result<Self, CommandCompileError> {
        let mut tokenizer = CommandTokenizer::new(source);
        let previous_token = tokenizer.next()?;
        let bindings_previous_len = bindings.len();
        Ok(Self {
            tokenizer,
            path,
            previous_token,
            bindings,
            bindings_previous_len,
            builtin_commands,
            macro_commands,
        })
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

    pub fn declare_binding(&mut self, range: Range<usize>) -> Result<(), CommandCompileError> {
        if self.bindings.len() >= u16::MAX as _ {
            Err(CommandCompileError {
                kind: CommandCompileErrorKind::TooManyBindings,
                range,
            })
        } else {
            let range = range.start as _..range.end as _;
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

    pub fn compile(&mut self, chunk: &mut ByteCodeChunk) -> Result<(), CommandCompileError> {
        chunk.clear();
        match compile(self, chunk) {
            Ok(()) => {
                chunk.emit(Op::PushLiteral { start: 0, len: 0 });
                chunk.emit(Op::Return);
            }
            Err(error) => {
                chunk.clear();
                return Err(error);
            }
        }
        Ok(())
    }
}
impl<'source, 'state> Drop for Compiler<'source, 'state> {
    fn drop(&mut self) {
        self.bindings.truncate(self.bindings_previous_len);
    }
}

fn compile(compiler: &mut Compiler, chunk: &mut ByteCodeChunk) -> Result<(), CommandCompileError> {
    fn parse_top_level(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        if let CommandTokenKind::Literal = compiler.previous_token.kind {
            match compiler.previous_token_str() {
                "source" => return parse_source(compiler, chunk),
                "macro" => return parse_macro(compiler, chunk),
                "return" => return parse_return(compiler, chunk),
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

        compiler.bindings.clear();
        if chunk.ops.last() != Some(&Op::Return) {
            chunk.emit(Op::PushLiteral { start: 0, len: 0 });
            chunk.emit(Op::Return);
        }
        Ok(())
    }

    fn parse_return(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        compiler.next_token()?;
        parse_expression(compiler, chunk)?;
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
                parse_command_call(compiler, chunk)?;
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

                parse_expression(compiler, chunk)?;
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

            /*
            if let Some(i) = compiler.request_commands.iter().position(|c| c.name == command_name) {
                return Ok(CommandSource::Request(i));
            }
            */

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
                        parse_expression(compiler, chunk)?;
                    }
                    _ => chunk.emit(Op::PushLiteral { start: 0, len: 0 }),
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
                parse_expression(compiler, chunk)?;

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
    ) -> Result<(), CommandCompileError> {
        while let CommandTokenKind::EndOfCommand = compiler.previous_token.kind {
            compiler.next_token()?;
        }

        let range_start = compiler.previous_token.range.start;
        match compiler.previous_token.kind {
            CommandTokenKind::Literal => {
                let literal = compiler.previous_token_str();
                let literal = chunk.add_literal(literal, compiler.previous_token.range.clone())?;
                chunk.emit(Op::PushLiteral {
                    start: literal.start,
                    len: literal.len,
                });
                compiler.next_token()?;
                Ok(())
            }
            CommandTokenKind::QuotedLiteral => {
                let literal = compiler.previous_token_str();
                let literal = &literal[1..];
                let literal = &literal[..literal.len() - 1];
                let literal =
                    chunk.add_escaped_literal(literal, compiler.previous_token.range.clone())?;
                chunk.emit(Op::PushLiteral {
                    start: literal.start,
                    len: literal.len,
                });
                compiler.next_token()?;
                Ok(())
            }
            CommandTokenKind::OpenParenthesis => parse_command_call(compiler, chunk),
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
    is_macro_chunk: bool,
    op_index: u32,
    texts_len: u32,
    value_stack_len: u16,
    flag_stack_len: u16,
}

#[derive(Default)]
struct VirtualMachine {
    texts: String,
    flag_stack: Vec<StackFlag>,
    value_stack: Vec<StackValue>,
    stack_frames: Vec<StackFrame>,
    prepared_stack_frames: Vec<StackFrame>,
}

fn execute(
    editor: &mut Editor,
    platform: &mut Platform,
    clients: &mut ClientManager,
    client_handle: Option<ClientHandle>,
) {
    let mut vm = &mut editor.commands_next.virtual_machine;
    let mut chunk = &editor.commands_next.temp_chunk;
    let mut op_index = 0;

    loop {
        match &chunk.ops[op_index] {
            Op::Return => {
                todo!();
            }
            Op::Pop => drop(vm.value_stack.pop()),
            &Op::PushLiteral { start, len } => {
                let start = start as usize;
                let end = start + len as usize;
                vm.texts.push_str(&chunk.texts[start..end]);
                vm.value_stack.push(StackValue {
                    start: start as _,
                    end: end as _,
                });
            }
            &Op::PushFromStack(stack_index) => {
                // TODO: is this ok?
                let value = vm.value_stack[stack_index as usize];
                //let range = value.start as usize..value.end as usize;
                //let start = vm.texts.len();
                //unsafe {
                //    vm.texts.as_mut_vec().extend_from_within(range);
                //}
                vm.value_stack.push(value);
            }
            &Op::PopAsFlag(flag_index) => {
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
            &Op::PrepareStackFrame { is_macro_chunk } => {
                let frame = StackFrame {
                    is_macro_chunk,
                    op_index: op_index as _,
                    texts_len: vm.texts.len() as _,
                    value_stack_len: vm.value_stack.len() as _,
                    flag_stack_len: vm.flag_stack.len() as _,
                };
                vm.prepared_stack_frames.push(frame);
            }
            &Op::CallBuiltinCommand(index) => {
                let frame = vm.prepared_stack_frames.pop().unwrap();
                let command_fn = &editor.commands_next.builtin_commands[index as usize].func;
                command_fn();

                chunk = if frame.is_macro_chunk {
                    &editor.commands_next.macro_commands.chunk
                } else {
                    &editor.commands_next.temp_chunk
                };
                vm = &mut editor.commands_next.virtual_machine;

                vm.value_stack.push(StackValue {
                    start: 0,
                    end: chunk.texts.len() as _,
                });

                vm.stack_frames.push(frame);
            }
            &Op::CallMacroCommand(index) => {
                todo!();
            }
            &Op::CallRequestCommand(index) => {
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

        assert_eq!(vec![PushLiteral { start: 0, len: 0 }, Return], compile(""));

        assert_eq!(
            vec![
                CallBuiltinCommand {
                    index: 0,
                    arg_count: 0,
                    flag_count: 0,
                },
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile("cmd"),
        );

        assert_eq!(
            vec![
                PushLiteral {
                    start: 0,
                    len: "arg0".len() as _,
                },
                PushLiteral {
                    start: "arg0".len() as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand {
                    index: 0,
                    arg_count: 2,
                    flag_count: 0,
                },
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile("cmd arg0 arg1"),
        );

        assert_eq!(
            vec![
                PushLiteral { start: 0, len: 0 },
                PopAsFlag(0),
                PushLiteral {
                    start: 0,
                    len: "arg".len() as _,
                },
                PushLiteral {
                    start: "arg".len() as _,
                    len: "opt".len() as _,
                },
                PopAsFlag(1),
                CallBuiltinCommand {
                    index: 0,
                    arg_count: 1,
                    flag_count: 2,
                },
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile("cmd -switch arg -option=opt"),
        );

        assert_eq!(
            vec![
                PushLiteral {
                    start: 0,
                    len: "arg0".len() as _,
                },
                PushLiteral {
                    start: "arg0".len() as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand {
                    index: 0,
                    arg_count: 1,
                    flag_count: 0,
                },
                PopAsFlag(1),
                PushLiteral {
                    start: "arg0arg1".len() as _,
                    len: "arg2".len() as _,
                },
                CallBuiltinCommand {
                    index: 0,
                    arg_count: 2,
                    flag_count: 1,
                },
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile("cmd arg0 -option=(cmd arg1) arg2"),
        );

        assert_eq!(
            vec![
                PushLiteral {
                    start: 0,
                    len: "arg0".len() as _,
                },
                PushLiteral {
                    start: "arg0".len() as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand {
                    index: 0,
                    arg_count: 2,
                    flag_count: 0,
                },
                Pop,
                PushLiteral { start: 0, len: 0 },
                Return
            ],
            compile("(cmd \n arg0 \n arg2)"),
        );
    }
}

