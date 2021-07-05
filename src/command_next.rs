use std::{fmt, ops::Range, path::Path};

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

type CommandFn = fn(&mut CommandContext) -> Result<Option<CommandOperation>, CommandErrorKind>;

pub struct CommandArgsBuilder {
    value_stack_index: u16,
    flag_stack_index: u16,
    bang: bool,
}
impl CommandArgsBuilder {
    pub fn with<'a>(&self, commands: &'a CommandManager) -> CommandArgs<'a> {
        CommandArgs {
            virtual_machine: &commands.virtual_machine,
            flag_stack_index: self.flag_stack_index,
            value_stack_index: self.value_stack_index,
            bang: self.bang,
        }
    }
}

pub struct CommandArgs<'a> {
    virtual_machine: &'a VirtualMachine,
    value_stack_index: u16,
    flag_stack_index: u16,
    pub bang: bool,
}
impl<'a> CommandArgs<'a> {
    pub fn get_flags(&self, flags: &mut [&'a str]) {
        let texts = &self.virtual_machine.texts;
        for flag in &self.virtual_machine.flag_stack[self.flag_stack_index as usize..] {
            flags[flag.index as usize] = &texts[flag.start as usize..flag.end as usize];
        }
    }

    pub fn try_next(&mut self) -> Option<&'a str> {
        let value = self
            .virtual_machine
            .value_stack
            .get(self.value_stack_index as usize)?;
        let range = value.start as usize..value.end as usize;
        self.value_stack_index += 1;
        Some(&self.virtual_machine.texts[range])
    }

    pub fn next(&mut self) -> Result<&'a str, CommandErrorKind> {
        match self.try_next() {
            Some(text) => Ok(text),
            None => Err(CommandErrorKind::TooFewArguments),
        }
    }

    pub fn assert_empty(&mut self) -> Result<(), CommandErrorKind> {
        match self.try_next() {
            Some(_) => Err(CommandErrorKind::TooManyArguments),
            None => Ok(()),
        }
    }
}

pub struct CommandContext<'a> {
    pub editor: &'a mut Editor,
    pub platform: &'a mut Platform,
    pub clients: &'a mut ClientManager,
    pub client_handle: Option<ClientHandle>,
    pub args: CommandArgsBuilder,
}
impl<'a> CommandContext<'a> {
    /*
    pub fn current_buffer_view_handle(&self) -> Result<BufferViewHandle, CommandError> {
        match self
            .client_handle
            .and_then(|h| self.clients.get(h))
            .and_then(Client::buffer_view_handle)
        {
            Some(handle) => Ok(handle),
            None => Err(CommandError::NoBufferOpened),
        }
    }

    pub fn current_buffer_handle(&self) -> Result<BufferHandle, CommandError> {
        let buffer_view_handle = self.current_buffer_view_handle()?;
        match self
            .editor
            .buffer_views
            .get(buffer_view_handle)
            .map(|v| v.buffer_handle)
        {
            Some(handle) => Ok(handle),
            None => Err(CommandError::NoBufferOpened),
        }
    }

    pub fn assert_can_discard_all_buffers(&self) -> Result<(), CommandError> {
        if self.args.bang || !self.editor.buffers.iter().any(Buffer::needs_save) {
            Ok(())
        } else {
            Err(CommandError::UnsavedChanges)
        }
    }

    pub fn assert_can_discard_buffer(&self, handle: BufferHandle) -> Result<(), CommandError> {
        let buffer = self
            .editor
            .buffers
            .get(handle)
            .ok_or(CommandError::InvalidBufferHandle(handle))?;
        if self.args.bang || !buffer.needs_save() {
            Ok(())
        } else {
            Err(CommandError::UnsavedChanges)
        }
    }
    */
}

pub enum CommandOperation {
    Suspend,
    Quit,
    QuitAll,
}

pub struct BuiltinCommand {
    pub name_hash: u64,
    pub alias_hash: u64,
    pub hidden: bool,
    pub completions: &'static [CompletionSource],
    pub accepts_bang: bool,
    pub flags: &'static [&'static str],
    pub func: CommandFn,
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

#[derive(Debug, Default, Clone, Copy)]
pub struct SourcePathHandle(u32);

struct SourcePathCollection {
    buf: String,
    ranges: Vec<Range<u32>>,
}
impl SourcePathCollection {
    pub fn get(&self, handle: SourcePathHandle) -> &Path {
        let range = self.ranges[handle.0 as usize].clone();
        let range = range.start as usize..range.end as usize;
        Path::new(&self.buf[range])
    }

    pub fn add(&mut self, path: &str) -> SourcePathHandle {
        let start = self.buf.len() as _;
        self.buf.push_str(path);
        let end = self.buf.len() as _;
        let handle = SourcePathHandle(self.ranges.len() as _);
        self.ranges.push(start..end);
        handle
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
    virtual_machine: VirtualMachine,
    paths: SourcePathCollection,
}
impl CommandManager {
    pub fn write_output(&mut self, output: &str) {
        self.virtual_machine.texts.push_str(output);
    }

    pub fn fmt_output(&mut self, args: fmt::Arguments) {
        let _ = fmt::write(&mut self.virtual_machine.texts, args);
    }

    pub fn eval_expression(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        source: &str,
        output: &mut String,
    ) -> Result<(), CommandError> {
        output.clear();
        let commands = &mut editor.commands_next;

        let mut compiler = Compiler::new(
            source,
            SourcePathHandle::default(),
            &mut commands.commands,
            &mut commands.virtual_machine,
        );
        let init_block = compile_expression(&mut compiler)?;

        execute(
            editor,
            platform,
            clients,
            client_handle,
            init_block.ops_start,
        )?;

        let commands = &mut editor.commands_next;
        let value = commands.virtual_machine.value_stack.pop().unwrap();
        let text = &commands.virtual_machine.texts[value.start as usize..value.end as usize];
        output.push_str(text);

        let vm = &mut commands.virtual_machine;
        vm.texts.truncate(init_block.texts_start as _);
        vm.ops.truncate(init_block.ops_start as _);
        vm.op_locations.truncate(init_block.ops_start as _);

        Ok(())
    }
}

#[derive(Debug)]
pub enum CommandErrorKind {
    UnterminatedQuotedLiteral,
    InvalidFlagName,
    InvalidBindingName,

    AstTooLong,
    TooManyMacroCommands,
    TooManyLiterals,
    LiteralTooLong,
    ExpectedToken(CommandTokenKind),
    ExpectedMacroDefinition,
    ExpectedStatement,
    ExpectedExpression,
    NonTrailingInitBlock,
    InvalidMacroName,
    InvalidLiteralEscaping,
    TooManyBindings,
    UndeclaredBinding,
    NoSuchCommand,
    NoSuchFlag,
    WrongNumberOfArgs,
    TooManyFlags,
    CommandAlreadyExists,

    CommandDoesNotAcceptBang,
    TooFewArguments,
    TooManyArguments,
}

const _ASSERT_COMMAND_ERROR_SIZE: [(); 16] = [(); std::mem::size_of::<CommandError>()];

#[derive(Debug)]
pub struct CommandError {
    pub kind: CommandErrorKind,
    pub source: SourcePathHandle,
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
    EndOfLine,
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
            if self.index >= source_bytes.len() {
                return Ok(CommandToken {
                    kind: CommandTokenKind::EndOfSource,
                    range: source_bytes.len() as _..source_bytes.len() as _,
                    position: self.position,
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
                        kind: CommandTokenKind::EndOfLine,
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
                                source: SourcePathHandle::default(),
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
                            source: SourcePathHandle::default(),
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
                            source: SourcePathHandle::default(),
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

struct Binding {
    pub name_hash: u64,
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

struct Compiler<'data, 'source> {
    pub tokenizer: CommandTokenizer<'source>,
    pub source: SourcePathHandle,
    pub commands: &'data mut CommandCollection,
    pub virtual_machine: &'data mut VirtualMachine,
    pub previous_token: CommandToken,
    pub bindings: [Binding; u8::MAX as _],
    pub bindings_len: u8,
}
impl<'data, 'source> Compiler<'data, 'source> {
    pub fn new(
        source: &'source str,
        source_handle: SourcePathHandle,
        commands: &'data mut CommandCollection,
        virtual_machine: &'data mut VirtualMachine,
    ) -> Self {
        const DEFAULT_BINDING: Binding = Binding { name_hash: 0 };

        Self {
            tokenizer: CommandTokenizer::new(source),
            commands,
            virtual_machine,
            source: source_handle,
            previous_token: CommandToken::default(),
            bindings: [DEFAULT_BINDING; u8::MAX as _],
            bindings_len: 0,
        }
    }

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
                error.source = self.source;
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
                source: self.source,
                position: self.previous_token.position,
            })
        }
    }

    pub fn declare_binding_from_previous_token(&mut self) -> Result<(), CommandError> {
        if self.bindings_len < u8::MAX {
            let name = self.previous_token_str();
            let name_hash = hash_bytes(name.as_bytes());
            self.bindings[self.bindings_len as usize] = Binding { name_hash };
            self.bindings_len += 1;
            Ok(())
        } else {
            Err(CommandError {
                kind: CommandErrorKind::TooManyBindings,
                source: self.source,
                position: self.previous_token.position,
            })
        }
    }

    pub fn find_binding_stack_index_from_previous_token(&self) -> Option<u8> {
        let name = self.previous_token_str();
        let name_hash = hash_bytes(name.as_bytes());
        self.bindings[..self.bindings_len as usize]
            .iter()
            .rposition(|b| b.name_hash == name_hash)
            .map(|i| i as _)
    }

    pub fn emit(&mut self, op: Op, position: BufferPosition) {
        self.virtual_machine.ops.push(op);
        self.virtual_machine.op_locations.push(SourceLocation {
            source: self.source,
            position,
        });
    }

    pub fn emit_push_literal_from_previous_token(&mut self) -> Result<(), CommandError> {
        let source = self.tokenizer.source;
        let texts = &mut self.virtual_machine.texts;
        let start = texts.len();
        let position = self.previous_token.position;

        match self.previous_token.kind {
            CommandTokenKind::Literal => {
                let text = &source[self.previous_token.range()];
                texts.push_str(text);
            }
            CommandTokenKind::QuotedLiteral => {
                let mut range = self.previous_token.range();
                range.start += 1;
                range.end -= 1;
                let mut text = &source[range];
                while let Some(i) = text.find('\\') {
                    texts.push_str(&text[..i]);
                    text = &text[i + 1..];
                    match text.as_bytes() {
                        &[b'\\', ..] => texts.push('\\'),
                        &[b'\'', ..] => texts.push('\''),
                        &[b'\"', ..] => texts.push('\"'),
                        &[b'\n', ..] => texts.push('\n'),
                        &[b'\r', ..] => texts.push('\r'),
                        &[b'\t', ..] => texts.push('\t'),
                        &[b'\0', ..] => texts.push('\0'),
                        _ => {
                            return Err(CommandError {
                                kind: CommandErrorKind::InvalidLiteralEscaping,
                                source: self.source,
                                position,
                            })
                        }
                    }
                }
                texts.push_str(text);
            }
            _ => unreachable!(),
        };

        let len = texts.len() - start;
        if len > u8::MAX as _ {
            return Err(CommandError {
                kind: CommandErrorKind::LiteralTooLong,
                source: self.source,
                position,
            });
        }

        self.emit(
            Op::PushStringLiteral {
                start: start as _,
                len: len as _,
            },
            position,
        );

        Ok(())
    }
}

fn expression(compiler: &mut Compiler) -> Result<(), CommandError> {
    while let CommandTokenKind::EndOfLine = compiler.previous_token.kind {
        compiler.next_token()?;
    }

    match compiler.previous_token.kind {
        CommandTokenKind::Literal => {
            if matches!(compiler.previous_token_str(), "macro" | "return") {
                return Err(CommandError {
                    kind: CommandErrorKind::ExpectedExpression,
                    source: compiler.source,
                    position: compiler.previous_token.position,
                });
            }

            compiler.emit_push_literal_from_previous_token()?;
            compiler.next_token()?;
            Ok(())
        }
        CommandTokenKind::QuotedLiteral => {
            compiler.emit_push_literal_from_previous_token()?;
            compiler.next_token()?;
            Ok(())
        }
        CommandTokenKind::OpenParenthesis => {
            compiler.next_token()?;
            command_call(compiler, true)?;
            compiler.consume_token(CommandTokenKind::CloseParenthesis)?;
            Ok(())
        }
        CommandTokenKind::Binding => {
            let position = compiler.previous_token.position;
            match compiler.find_binding_stack_index_from_previous_token() {
                Some(index) => {
                    compiler.next_token()?;
                    compiler.emit(Op::DuplicateAt(index), position);
                    Ok(())
                }
                None => Err(CommandError {
                    kind: CommandErrorKind::UndeclaredBinding,
                    source: compiler.source,
                    position,
                }),
            }
        }
        _ => Err(CommandError {
            kind: CommandErrorKind::ExpectedExpression,
            source: compiler.source,
            position: compiler.previous_token.position,
        }),
    }
}

fn command_call(compiler: &mut Compiler, ignore_end_of_line: bool) -> Result<(), CommandError> {
    let position = compiler.previous_token.position;
    let command_name = compiler.previous_token_str();
    compiler.consume_token(CommandTokenKind::Literal)?;

    let (command_name, bang) = match command_name.strip_suffix('!') {
        Some(name) => (name, true),
        None => (command_name, false),
    };
    let command_name_hash = hash_bytes(command_name.as_bytes());
    let command_source = match find_command(compiler.commands, command_name_hash) {
        Some(source) => source,
        None => {
            return Err(CommandError {
                kind: CommandErrorKind::NoSuchCommand,
                source: compiler.source,
                position,
            })
        }
    };

    if bang {
        let accepts_bang = match command_source {
            CommandSource::Builtin(i) => compiler.commands.builtin_commands[i].accepts_bang,
            _ => false,
        };
        if !accepts_bang {
            return Err(CommandError {
                kind: CommandErrorKind::CommandDoesNotAcceptBang,
                source: compiler.source,
                position,
            });
        }
    }

    compiler.emit(Op::PushStackFrame, position);

    let mut arg_count = 0;
    let mut flag_count = 0;

    loop {
        match compiler.previous_token.kind {
            CommandTokenKind::Flag => {
                let position = compiler.previous_token.position;

                if flag_count == u8::MAX {
                    return Err(CommandError {
                        kind: CommandErrorKind::TooManyFlags,
                        source: compiler.source,
                        position,
                    });
                }
                flag_count += 1;

                let flag_name = &compiler.previous_token_str()[1..];
                compiler.next_token()?;

                let command_flags = match command_source {
                    CommandSource::Builtin(i) => compiler.commands.builtin_commands[i].flags,
                    _ => {
                        return Err(CommandError {
                            kind: CommandErrorKind::NoSuchFlag,
                            source: compiler.source,
                            position,
                        })
                    }
                };

                let mut index = None;
                for (i, &flag) in command_flags.iter().enumerate() {
                    if flag == flag_name {
                        index = Some(i as _);
                        break;
                    }
                }
                let index = match index {
                    Some(index) => index,
                    None => {
                        return Err(CommandError {
                            kind: CommandErrorKind::NoSuchFlag,
                            source: compiler.source,
                            position,
                        })
                    }
                };

                match compiler.previous_token.kind {
                    CommandTokenKind::Equals => {
                        compiler.next_token()?;
                        expression(compiler)?;
                        compiler.emit(Op::PopAsFlag(index), position);
                    }
                    _ => {
                        compiler.emit(Op::PushStringLiteral { start: 0, len: 1 }, position);
                        compiler.emit(Op::PopAsFlag(index), position);
                    }
                }
            }
            CommandTokenKind::EndOfLine => {
                compiler.next_token()?;
                if !ignore_end_of_line {
                    break;
                }
            }
            CommandTokenKind::CloseParenthesis
            | CommandTokenKind::CloseCurlyBrackets
            | CommandTokenKind::EndOfSource => break,
            _ => {
                if arg_count == u8::MAX {
                    return Err(CommandError {
                        kind: CommandErrorKind::WrongNumberOfArgs,
                        source: compiler.source,
                        position,
                    });
                }
                arg_count += 1;
                expression(compiler)?;
            }
        }
    }

    if let CommandSource::Macro(i) = command_source {
        let param_count = compiler.commands.macro_commands[i].param_count;
        if arg_count != param_count {
            return Err(CommandError {
                kind: CommandErrorKind::WrongNumberOfArgs,
                source: compiler.source,
                position,
            });
        }
    }

    let op = match command_source {
        CommandSource::Builtin(i) => Op::CallBuiltinCommand {
            index: i as _,
            bang,
        },
        CommandSource::Macro(i) => Op::CallMacroCommand(i as _),
        CommandSource::Request(i) => Op::CallRequestCommand(i as _),
    };
    compiler.emit(op, position);

    Ok(())
}

fn expression_or_command_call(compiler: &mut Compiler) -> Result<(), CommandError> {
    match compiler.previous_token.kind {
        CommandTokenKind::Literal => command_call(compiler, false),
        _ => expression(compiler),
    }
}

struct InitBlock {
    pub texts_start: u32,
    pub ops_start: u32,
}
impl InitBlock {
    pub fn from_virtual_machine(vm: &VirtualMachine) -> Self {
        Self {
            texts_start: vm.texts.len() as _,
            ops_start: vm.ops.len() as _,
        }
    }
}

fn compile_expression(compiler: &mut Compiler) -> Result<InitBlock, CommandError> {
    compiler.next_token()?;
    while let CommandTokenKind::EndOfLine = compiler.previous_token.kind {
        compiler.next_token()?;
    }

    let init_block = InitBlock::from_virtual_machine(&compiler.virtual_machine);

    if let CommandTokenKind::EndOfSource = compiler.previous_token.kind {
        compiler.emit(Op::PushStringLiteral { start: 0, len: 0 }, BufferPosition::zero());
        compiler.emit(Op::Return, BufferPosition::zero());
        return Ok(init_block);
    }

    if compiler.virtual_machine.texts.is_empty() {
        compiler.virtual_machine.texts.push('\0');
    }

    expression_or_command_call(compiler)?;
    compiler.emit(Op::Return, BufferPosition::zero());

    while let CommandTokenKind::EndOfLine = compiler.previous_token.kind {
        compiler.next_token()?;
    }
    compiler.consume_token(CommandTokenKind::EndOfSource)?;

    Ok(init_block)
}

fn compile_source(compiler: &mut Compiler) -> Result<InitBlock, CommandError> {
    fn definition_block(
        compiler: &mut Compiler,
        position: BufferPosition,
    ) -> Result<(), CommandError> {
        loop {
            if let CommandTokenKind::CloseCurlyBrackets | CommandTokenKind::EndOfSource =
                compiler.previous_token.kind
            {
                break;
            }
            statement(compiler)?;
        }
        compiler.consume_token(CommandTokenKind::CloseCurlyBrackets)?;

        if !matches!(compiler.virtual_machine.ops.last(), Some(Op::Return)) {
            compiler.emit(Op::PushStringLiteral { start: 0, len: 0 }, position);
            compiler.emit(Op::Return, position);
        }

        Ok(())
    }

    fn init_block_definition(compiler: &mut Compiler) -> Result<InitBlock, CommandError> {
        let position = compiler.previous_token.position;
        compiler.next_token()?;
        compiler.consume_token(CommandTokenKind::OpenCurlyBrackets)?;

        let init_block = InitBlock::from_virtual_machine(&compiler.virtual_machine);
        definition_block(compiler, position)?;
        Ok(init_block)
    }

    fn macro_definition(compiler: &mut Compiler) -> Result<(), CommandError> {
        let position = compiler.previous_token.position;
        compiler.next_token()?;

        let name = compiler.previous_token_str();
        if name
            .chars()
            .any(|c| !matches!(c, '_' | '-' | 'a'..='z' | 'A'..='Z' | '0'..='9'))
        {
            return Err(CommandError {
                kind: CommandErrorKind::InvalidMacroName,
                source: compiler.source,
                position,
            });
        }
        let name_hash = hash_bytes(name.as_bytes());
        if find_command(compiler.commands, name_hash).is_some() {
            return Err(CommandError {
                kind: CommandErrorKind::CommandAlreadyExists,
                source: compiler.source,
                position,
            });
        }
        compiler.consume_token(CommandTokenKind::Literal)?;

        loop {
            match compiler.previous_token.kind {
                CommandTokenKind::OpenCurlyBrackets => {
                    compiler.next_token()?;
                    break;
                }
                CommandTokenKind::Binding => {
                    compiler.declare_binding_from_previous_token()?;
                    compiler.next_token()?;
                }
                _ => {
                    return Err(CommandError {
                        kind: CommandErrorKind::ExpectedToken(CommandTokenKind::OpenCurlyBrackets),
                        source: compiler.source,
                        position: compiler.previous_token.position,
                    })
                }
            }
        }

        let param_count = compiler.bindings_len;
        let op_start_index = compiler.virtual_machine.ops.len() as _;

        definition_block(compiler, position)?;

        compiler.commands.macro_commands.push(MacroCommand {
            name_hash,
            op_start_index,
            param_count,
        });

        compiler.bindings_len = 0;

        Ok(())
    }

    fn statement(compiler: &mut Compiler) -> Result<(), CommandError> {
        match compiler.previous_token.kind {
            CommandTokenKind::Literal => match compiler.previous_token_str() {
                "macro" => Err(CommandError {
                    kind: CommandErrorKind::ExpectedStatement,
                    source: compiler.source,
                    position: compiler.previous_token.position,
                }),
                "return" => {
                    let position = compiler.previous_token.position;
                    compiler.next_token()?;
                    expression_or_command_call(compiler)?;
                    compiler.emit(Op::Return, position);
                    Ok(())
                }
                _ => {
                    command_call(compiler, false)?;
                    compiler.emit(Op::Pop, compiler.previous_token.position);
                    Ok(())
                }
            },
            CommandTokenKind::OpenParenthesis => {
                expression(compiler)?;
                compiler.emit(Op::Pop, compiler.previous_token.position);
                Ok(())
            }
            CommandTokenKind::Binding => {
                compiler.declare_binding_from_previous_token()?;
                compiler.next_token()?;
                compiler.consume_token(CommandTokenKind::Equals)?;
                expression_or_command_call(compiler)?;
                Ok(())
            }
            CommandTokenKind::EndOfLine => compiler.next_token(),
            CommandTokenKind::EndOfSource => Ok(()),
            _ => Err(CommandError {
                kind: CommandErrorKind::ExpectedStatement,
                source: compiler.source,
                position: compiler.previous_token.position,
            }),
        }
    }

    compiler.next_token()?;
    if compiler.virtual_machine.texts.is_empty() {
        compiler.virtual_machine.texts.push('\0');
    }

    let mut init_block = None;
    loop {
        match compiler.previous_token.kind {
            CommandTokenKind::Literal => match compiler.previous_token_str() {
                "macro" => macro_definition(compiler)?,
                "init" => {
                    init_block = Some(init_block_definition(compiler)?);
                    while let CommandTokenKind::EndOfLine = compiler.previous_token.kind {
                        compiler.next_token()?;
                    }
                    match compiler.previous_token.kind {
                        CommandTokenKind::EndOfSource => (),
                        _ => {
                            return Err(CommandError {
                                kind: CommandErrorKind::NonTrailingInitBlock,
                                source: compiler.source,
                                position: compiler.previous_token.position,
                            })
                        }
                    }

                    break;
                }
                _ => {
                    return Err(CommandError {
                        kind: CommandErrorKind::ExpectedMacroDefinition,
                        source: compiler.source,
                        position: compiler.previous_token.position,
                    })
                }
            },
            CommandTokenKind::EndOfLine => compiler.next_token()?,
            CommandTokenKind::EndOfSource => break,
            _ => {
                return Err(CommandError {
                    kind: CommandErrorKind::ExpectedMacroDefinition,
                    source: compiler.source,
                    position: compiler.previous_token.position,
                })
            }
        }
    }

    let init_block = match init_block {
        Some(block) => block,
        None => {
            let block = InitBlock::from_virtual_machine(&compiler.virtual_machine);
            compiler.emit(
                Op::PushStringLiteral { start: 0, len: 0 },
                BufferPosition::zero(),
            );
            compiler.emit(Op::Return, BufferPosition::zero());
            block
        }
    };

    Ok(init_block)
}

const _ASSERT_OP_SIZE: [(); 4] = [(); std::mem::size_of::<Op>()];

#[derive(Debug, PartialEq, Eq)]
enum Op {
    Return,
    Pop,
    PushStringLiteral { start: u16, len: u8 },
    DuplicateAt(u8),
    PopAsFlag(u8),
    PushStackFrame,
    CallBuiltinCommand { index: u8, bang: bool },
    CallMacroCommand(u16),
    CallRequestCommand(u16),
}

#[derive(Clone, Copy)]
struct StackValue {
    pub start: u32,
    pub end: u32,
}

struct StackFlag {
    pub index: u8,
    pub start: u32,
    pub end: u32,
}

struct StackFrame {
    op_index: u32,
    texts_start: u32,
    flag_stack_start: u16,
    value_stack_start: u16,
}

struct SourceLocation {
    source: SourcePathHandle,
    position: BufferPosition,
}

#[derive(Default)]
struct VirtualMachine {
    ops: Vec<Op>,
    texts: String,
    flag_stack: Vec<StackFlag>,
    value_stack: Vec<StackValue>,
    frames: Vec<StackFrame>,
    pending_frames: Vec<StackFrame>,
    op_locations: Vec<SourceLocation>,
}

fn execute(
    editor: &mut Editor,
    platform: &mut Platform,
    clients: &mut ClientManager,
    client_handle: Option<ClientHandle>,
    mut op_index: u32,
) -> Result<Option<CommandOperation>, CommandError> {
    let mut vm = &mut editor.commands_next.virtual_machine;
    let mut start_stack_index = 0;

    vm.flag_stack.clear();
    vm.value_stack.clear();
    vm.frames.clear();
    vm.pending_frames.clear();

    vm.frames.push(StackFrame {
        op_index,
        texts_start: vm.texts.len() as _,
        flag_stack_start: 0,
        value_stack_start: 0,
    });

    loop {
        //*
        eprint!("\nstack: ");
        for value in &vm.value_stack {
            let range = value.start as usize..value.end as usize;
            eprint!("{}:{}={}, ", value.start, value.end, &vm.texts[range]);
        }
        eprintln!("\ntexts: '{}'", &vm.texts);
        eprintln!(
            "[{}] {:?} (stack_start: {})",
            op_index, &vm.ops[op_index as usize], start_stack_index
        );
        //*/
        match vm.ops[op_index as usize] {
            Op::Return => {
                let frame = vm.frames.pop().unwrap();

                let value = vm.value_stack.last().unwrap();
                let value = if value.start >= frame.texts_start {
                    let return_text_start = frame.texts_start as usize;
                    let return_text_range = value.start as usize..value.end as usize;
                    let return_text_len = return_text_range.end - return_text_range.start;
                    unsafe {
                        let bytes = vm.texts.as_mut_vec();
                        bytes.copy_within(return_text_range, return_text_start);
                        bytes.truncate(return_text_start + return_text_len);
                    }
                    StackValue {
                        start: frame.texts_start,
                        end: vm.texts.len() as _,
                    }
                } else {
                    vm.texts.truncate(frame.texts_start as _);
                    value.clone()
                };

                vm.value_stack.truncate(frame.value_stack_start as _);
                vm.value_stack.push(value);

                op_index = frame.op_index;
                start_stack_index = frame.value_stack_start as _;

                if vm.frames.is_empty() {
                    return Ok(None);
                }
            }
            Op::Pop => {
                let texts_start = vm.frames.last().unwrap().texts_start;
                let value = vm.value_stack.pop().unwrap();
                if value.start == texts_start && value.end == vm.texts.len() as _ {
                    vm.texts.truncate(texts_start as _);
                }
            }
            Op::PushStringLiteral { start, len } => {
                let start = start as usize;
                let end = start + len as usize;
                vm.value_stack.push(StackValue {
                    start: start as _,
                    end: end as _,
                });
            }
            Op::DuplicateAt(stack_index) => {
                let value = vm.value_stack[start_stack_index + stack_index as usize];
                vm.value_stack.push(value);
            }
            Op::PopAsFlag(index) => {
                let value = vm.value_stack.pop().unwrap();
                vm.flag_stack.push(StackFlag {
                    index,
                    start: value.start,
                    end: value.end,
                });
            }
            Op::PushStackFrame => {
                let frame = StackFrame {
                    op_index: 0,
                    texts_start: vm.texts.len() as _,
                    flag_stack_start: vm.flag_stack.len() as _,
                    value_stack_start: vm.value_stack.len() as _,
                };
                vm.pending_frames.push(frame);
            }
            Op::CallBuiltinCommand { index, bang } => {
                let mut frame = vm.pending_frames.pop().unwrap();
                let command = &editor.commands_next.commands.builtin_commands[index as usize];
                let command_fn = command.func;

                let return_text_start = vm.texts.len();

                let mut ctx = CommandContext {
                    editor,
                    platform,
                    clients,
                    client_handle,
                    args: CommandArgsBuilder {
                        flag_stack_index: frame.flag_stack_start,
                        value_stack_index: frame.value_stack_start,
                        bang,
                    },
                };
                match command_fn(&mut ctx) {
                    Ok(Some(op)) => return Ok(Some(op)),
                    Ok(None) => (),
                    Err(kind) => {
                        frame.op_index = op_index;
                        vm = &mut editor.commands_next.virtual_machine;
                        vm.frames.push(frame);
                        let location = &vm.op_locations[op_index as usize];
                        return Err(CommandError {
                            kind,
                            source: location.source,
                            position: location.position,
                        });
                    }
                }

                vm = &mut editor.commands_next.virtual_machine;
                vm.texts
                    .drain(return_text_start..frame.texts_start as usize);
                vm.flag_stack.truncate(frame.flag_stack_start as _);
                vm.value_stack.truncate(frame.value_stack_start as _);
                vm.value_stack.push(StackValue {
                    start: frame.texts_start as _,
                    end: vm.texts.len() as _,
                });
            }
            Op::CallMacroCommand(index) => {
                let mut frame = vm.pending_frames.pop().unwrap();
                start_stack_index = frame.value_stack_start as _;

                let command = &editor.commands_next.commands.macro_commands[index as usize];
                frame.op_index = op_index;
                op_index = command.op_start_index as _;

                vm.frames.push(frame);
                continue;
            }
            Op::CallRequestCommand(index) => {
                let mut frame = vm.pending_frames.pop().unwrap();
                frame.op_index = op_index as _;
                // TODO: send request
                vm.texts.truncate(frame.texts_start as _);
                vm.value_stack.truncate(frame.value_stack_start as _);
                vm.value_stack.push(StackValue { start: 0, end: 0 });

                todo!();
            }
        }
        op_index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

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

        assert_eq!(0, collect("").len());
        assert_eq!(0, collect("  ").len());
        assert_eq!(vec![(Literal, "command", pos(0, 0)),], collect("command"),);
        assert_eq!(
            vec![(QuotedLiteral, "'text'", pos(0, 0)),],
            collect("'text'"),
        );
        assert_eq!(
            vec![
                (Literal, "cmd", pos(0, 0)),
                (OpenParenthesis, "(", pos(0, 4)),
                (Literal, "subcmd", pos(0, 5)),
                (CloseParenthesis, ")", pos(0, 11)),
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
            ],
            collect("cmd $binding -flag=value = not-flag"),
        );
        assert_eq!(
            vec![
                (Literal, "cmd0", pos(0, 0)),
                (Literal, "cmd1", pos(0, 5)),
                (EndOfLine, "\n\n \t \n  ", pos(0, 12)),
                (Literal, "cmd2", pos(3, 2)),
            ],
            collect("cmd0 cmd1 \t\r\n\n \t \n  cmd2"),
        );
    }

    enum CompilationMode {
        Expression,
        Source,
    }

    fn compile_into(
        commands: &mut CommandManager,
        source: &str,
        mode: CompilationMode,
    ) -> InitBlock {
        static BUILTIN_COMMANDS: &[BuiltinCommand] = &[
            BuiltinCommand {
                name_hash: hash_bytes(b"cmd"),
                alias_hash: hash_bytes(b""),
                hidden: false,
                completions: &[],
                accepts_bang: true,
                flags: &["switch", "option"],
                func: |_| Ok(None),
            },
            BuiltinCommand {
                name_hash: hash_bytes(b"append"),
                alias_hash: hash_bytes(b""),
                hidden: false,
                completions: &[],
                accepts_bang: false,
                flags: &[],
                func: |ctx| {
                    let mut args = ctx.args.with(&ctx.editor.commands_next);
                    let mut output = String::new();
                    while let Some(arg) = args.try_next() {
                        output.push_str(arg);
                    }
                    ctx.editor.commands_next.write_output(&output);
                    Ok(None)
                },
            },
        ];
        commands.commands.builtin_commands = BUILTIN_COMMANDS;

        let mut compiler = Compiler::new(
            source,
            SourcePathHandle::default(),
            &mut commands.commands,
            &mut commands.virtual_machine,
        );
        let init_block = match mode {
            CompilationMode::Expression => compile_expression(&mut compiler).unwrap(),
            CompilationMode::Source => compile_source(&mut compiler).unwrap(),
        };

        assert_eq!(
            commands.virtual_machine.ops.len(),
            commands.virtual_machine.op_locations.len()
        );

        init_block
    }

    fn compile_into_ops(source: &str, mode: CompilationMode) -> Vec<Op> {
        let mut commands = CommandManager::default();
        compile_into(&mut commands, source, mode);
        commands.virtual_machine.ops
    }

    #[test]
    fn command_compiler() {
        use Op::*;

        assert_eq!(
            vec![PushStringLiteral { start: 0, len: 0 }, Return],
            compile_into_ops("", CompilationMode::Expression),
        );

        assert_eq!(
            vec![
                PushStackFrame,
                CallBuiltinCommand {
                    index: 0,
                    bang: false,
                },
                Return,
            ],
            compile_into_ops("cmd", CompilationMode::Expression),
        );

        assert_eq!(
            vec![
                PushStackFrame,
                PushStringLiteral {
                    start: 1,
                    len: "arg0".len() as _,
                },
                PushStringLiteral {
                    start: (1 + "arg0".len()) as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand {
                    index: 0,
                    bang: true,
                },
                Return,
            ],
            compile_into_ops("cmd! arg0 arg1", CompilationMode::Expression),
        );

        assert_eq!(
            vec![
                PushStackFrame,
                PushStringLiteral {
                    start: 0,
                    len: 1,
                },
                PopAsFlag(0),
                PushStringLiteral {
                    start: 1,
                    len: "arg".len() as _,
                },
                PushStringLiteral {
                    start: (1 + "arg".len()) as _,
                    len: "opt".len() as _,
                },
                PopAsFlag(1),
                CallBuiltinCommand {
                    index: 0,
                    bang: false,
                },
                Return,
            ],
            compile_into_ops("cmd -switch arg -option=opt", CompilationMode::Expression),
        );

        assert_eq!(
            vec![
                PushStackFrame,
                PushStringLiteral { start: 1, len: "arg0".len() as _ },
                // begin nested call
                PushStackFrame,
                PushStringLiteral {
                    start: (1 + "arg0".len()) as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand {
                    index: 0,
                    bang: false,
                },
                PopAsFlag(1),
                // end nested call
                PushStringLiteral {
                    start: (1 + "arg0arg1".len()) as _,
                    len: "arg2".len() as _,
                },
                CallBuiltinCommand {
                    index: 0,
                    bang: false,
                },
                Return,
            ],
            compile_into_ops(
                "cmd arg0 -option=(cmd arg1) arg2",
                CompilationMode::Expression
            ),
        );

        assert_eq!(
            vec![
                PushStackFrame,
                PushStringLiteral {
                    start: 1,
                    len: "arg0".len() as _,
                },
                PushStringLiteral {
                    start: (1 + "arg0".len()) as _,
                    len: "arg1".len() as _,
                },
                CallBuiltinCommand {
                    index: 0,
                    bang: false,
                },
                Return,
            ],
            compile_into_ops("(cmd \n arg0 \n arg1)", CompilationMode::Expression),
        );

        assert_eq!(
            vec![
                PushStackFrame,
                DuplicateAt(0),
                DuplicateAt(1),
                PopAsFlag(1),
                CallBuiltinCommand {
                    index: 0,
                    bang: false,
                },
                Return,
                PushStringLiteral { start: 0, len: 0 },
                Return,
            ],
            compile_into_ops(
                "macro c $a $b {\n\t return cmd $a -option=$b\n}",
                CompilationMode::Source
            ),
        );

        assert_eq!(
            vec![
                // begin macro
                PushStackFrame,
                DuplicateAt(0),
                CallBuiltinCommand {
                    index: 0,
                    bang: false,
                },
                Pop,
                PushStringLiteral { start: 0, len: 0 },
                Return,
                // end macro
                PushStackFrame,
                PushStringLiteral {
                    start: 1,
                    len: "0".len() as _
                },
                CallBuiltinCommand {
                    index: 0,
                    bang: false,
                },
                Pop,
                PushStackFrame,
                PushStringLiteral {
                    start: (1 + "0".len()) as _,
                    len: "1".len() as _
                },
                CallBuiltinCommand {
                    index: 0,
                    bang: false,
                },
                Pop,
                PushStringLiteral { start: 0, len: 0 },
                Return,
            ],
            compile_into_ops(
                "macro c $p { cmd $p } init { cmd '0' \n cmd '1' }",
                CompilationMode::Source
            ),
        );
    }

    #[test]
    fn command_execution() {
        fn eval(source: &str) -> String {
            eval_debug(source, false)
        }

        fn eval_debug(source: &str, debug: bool) -> String {
            let mut editor = Editor::new(PathBuf::new());
            let (request_sender, _) = std::sync::mpsc::channel();
            let mut platform = Platform::new(|| (), request_sender);
            let mut clients = ClientManager::default();

            let init_block =
                compile_into(&mut editor.commands_next, source, CompilationMode::Source);

            if debug {
                eprintln!("==================================================================");
                let c = &editor.commands_next;
                dbg!(init_block.ops_start, &c.virtual_machine.ops);
            }

            execute(
                &mut editor,
                &mut platform,
                &mut clients,
                None,
                init_block.ops_start,
            )
            .unwrap();

            let vm = &editor.commands_next.virtual_machine;

            if debug {
                eprintln!("\nstack:");
                for value in &vm.value_stack {
                    let range = value.start as usize..value.end as usize;
                    eprintln!("\t{}..{} = '{}'", value.start, value.end, &vm.texts[range]);
                }
                eprintln!("texts: '{}'", &vm.texts);
                eprintln!();
            }

            assert_eq!(1, vm.value_stack.len());
            assert_eq!(0, vm.frames.len());
            assert_eq!(0, vm.pending_frames.len());

            let value = vm.value_stack.last().unwrap();
            vm.texts[value.start as usize..value.end as usize].into()
        }

        assert_eq!("", eval(""));
        assert_eq!("abc", eval("init { return 'abc' }"));
        assert_eq!("", eval("macro c { }"));
        assert_eq!("", eval("macro c $a { return $a }"));
        assert_eq!(
            "",
            eval("macro c $a { return $a }\n init { c 'abc' \n c 'def' }")
        );
        assert_eq!(
            "abc",
            eval("macro c $a { return $a }\n init { return c 'abc' }")
        );
        assert_eq!(
            "a",
            eval("macro first $a $b { return $a }\n init { return first a b }")
        );
        assert_eq!(
            "b",
            eval("macro second $a $b { return $b }\n init { return second a b }")
        );
        assert_eq!(
            "ab",
            eval(concat!(
                "macro first $a $b { return $a }\n",
                "macro second $a $b { return first $b x }\n",
                "init { return append (first a y) (second a b) }",
            ))
        );
    }
}

