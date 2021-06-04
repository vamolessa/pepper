use std::{
    collections::VecDeque,
    fmt, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{
    buffer::{Buffer, BufferCollection, BufferError, BufferHandle},
    buffer_view::BufferViewHandle,
    client::{Client, ClientHandle, ClientManager},
    editor::Editor,
    editor_utils::MessageKind,
    events::{KeyParseError, ServerEvent},
    pattern::PatternErrorKind,
    platform::{Platform, PlatformRequest, ProcessHandle, ProcessTag, SharedBuf},
    register::{RegisterCollection, RegisterKey, RETURN_REGISTER},
    serialization::Serialize,
};

mod builtin;

pub const HISTORY_CAPACITY: usize = 10;

#[derive(Clone, Copy)]
pub enum CommandTokenKind {
    Identifier,
    String,
    Register,
    Flag,
    Equals,
    Unterminated,
}

#[derive(Clone, Copy)]
pub struct CommandToken {
    pub from: usize,
    pub to: usize,
}
impl CommandToken {
    pub fn as_str<'a>(&self, command: &'a str) -> &'a str {
        &command[self.from..self.to]
    }
}

pub enum RawCommandValue {
    Literal(CommandToken),
    Register(CommandToken, RegisterKey),
}

pub struct CommandValue<'a> {
    pub token: CommandToken,
    pub text: &'a str,
}
impl<'a> CommandValue<'a> {
    pub fn from_raw(
        raw: &'a str,
        value: RawCommandValue,
        registers: &'a RegisterCollection,
    ) -> Self {
        match value {
            RawCommandValue::Literal(token) => Self {
                token,
                text: token.as_str(raw),
            },
            RawCommandValue::Register(token, register) => Self {
                token,
                text: registers.get(register),
            },
        }
    }
}

pub enum CommandError {
    InvalidCommandName(CommandToken),
    CommandNotFound(CommandToken),
    CommandDoesNotAcceptBang,
    UnterminatedToken(CommandToken),
    InvalidToken(CommandToken),
    TooFewArguments(usize),
    TooManyArguments(CommandToken, usize),
    InvalidRegisterKey(CommandToken),
    UnknownFlag(CommandToken),
    UnsavedChanges,
    NoBufferOpened,
    InvalidBufferHandle(BufferHandle),
    InvalidPath(CommandToken),
    ParseCommandValueError {
        value: CommandToken,
        type_name: &'static str,
    },
    OpenFileError {
        path: CommandToken,
        error: io::Error,
    },
    BufferError(BufferHandle, BufferError),
    BufferedKeysParseError(CommandToken),
    ConfigNotFound(CommandToken),
    InvalidConfigValue {
        key: CommandToken,
        value: CommandToken,
    },
    ColorNotFound(CommandToken),
    InvalidColorValue {
        key: CommandToken,
        value: CommandToken,
    },
    InvalidGlob(CommandToken),
    SyntaxExpectedEquals(CommandToken),
    SyntaxExpectedPattern(CommandToken),
    PatternError(CommandToken, PatternErrorKind),
    KeyParseError(CommandToken, KeyParseError),
    LspServerNotRunning,
    EvalCommandError {
        command: String,
        error: Box<CommandError>,
    },
    MacroCommandError {
        index: usize,
        command: String,
        error: Box<CommandError>,
    },
}
impl CommandError {
    pub fn display<'command, 'error>(
        &'error self,
        command: &'command str,
        source_path: Option<&'command Path>,
        commands: &'error CommandManager,
        buffers: &'error BufferCollection,
    ) -> CommandErrorDisplay<'command, 'error> {
        CommandErrorDisplay {
            command,
            source_path,
            commands,
            buffers,
            error: self,
        }
    }
}

pub struct CommandErrorDisplay<'command, 'error> {
    command: &'command str,
    source_path: Option<&'command Path>,
    commands: &'error CommandManager,
    buffers: &'error BufferCollection,
    error: &'error CommandError,
}
impl<'command, 'error> fmt::Display for CommandErrorDisplay<'command, 'error> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn write(
            this: &CommandErrorDisplay,
            f: &mut fmt::Formatter,
            error_token: &CommandToken,
            message: fmt::Arguments,
        ) -> fmt::Result {
            let error_len = this.command[error_token.from..error_token.to]
                .chars()
                .count()
                .max(1);
            let error_offset = this
                .command
                .char_indices()
                .take_while(|&(i, _)| i < error_token.from)
                .count();

            if let Some(path) = this.source_path {
                writeln!(f, "@ {:?}", path)?;
            }

            write!(
                f,
                "{}\n{: >offset$}{:^<len$}\n{}",
                this.command,
                "",
                "",
                message,
                offset = error_offset,
                len = error_len
            )
        }

        let c = self.command;
        match self.error {
            CommandError::InvalidCommandName(token) => write(
                self,
                f,
                token,
                format_args!("invalid command name '{}'", token.as_str(c)),
            ),
            CommandError::CommandNotFound(command) => write(
                self,
                f,
                command,
                format_args!("no such command '{}'", command.as_str(c)),
            ),
            CommandError::CommandDoesNotAcceptBang => {
                let mut tokens = CommandTokenIter::new(c);
                let token = match tokens.next() {
                    Some((_, token)) => token,
                    None => tokens.end_token(),
                };

                write(
                    self,
                    f,
                    &token,
                    format_args!("command does not accept bang"),
                )
            }
            CommandError::UnterminatedToken(token) => {
                write(self, f, token, format_args!("unterminated token"))
            }
            CommandError::InvalidToken(token) => write(
                self,
                f,
                token,
                format_args!("invalid token '{}'", token.as_str(c)),
            ),
            CommandError::TooFewArguments(min) => write(
                self,
                f,
                &CommandTokenIter::new(c).end_token(),
                format_args!("command expects at least {} arguments", min),
            ),
            CommandError::TooManyArguments(token, max) => write(
                self,
                f,
                token,
                format_args!("command expects at most {} arguments", max),
            ),
            CommandError::InvalidRegisterKey(key) => write(
                self,
                f,
                key,
                format_args!("invalid register key '{}'", key.as_str(c)),
            ),
            CommandError::UnknownFlag(token) => write(
                self,
                f,
                token,
                format_args!("unknown flag '{}'", token.as_str(c)),
            ),
            CommandError::UnsavedChanges => f.write_str(
                "there are unsaved changes. try appending a '!' to command name to force execute",
            ),
            CommandError::NoBufferOpened => f.write_str("no buffer opened"),
            CommandError::InvalidBufferHandle(handle) => {
                write!(f, "invalid buffer handle {}", handle)
            }
            CommandError::InvalidPath(path) => write(
                self,
                f,
                path,
                format_args!("invalid path '{}'", path.as_str(c)),
            ),
            CommandError::ParseCommandValueError { value, type_name } => write(
                self,
                f,
                value,
                format_args!("could not parse '{}' as {}", value.as_str(c), type_name),
            ),
            CommandError::OpenFileError { path, error } => write(
                self,
                f,
                path,
                format_args!("could not open file '{}': {}", path.as_str(c), error),
            ),
            CommandError::BufferError(handle, error) => match self.buffers.get(*handle) {
                Some(buffer) => write!(f, "{}", error.display(buffer)),
                None => Ok(()),
            },
            CommandError::BufferedKeysParseError(token) => write(
                self,
                f,
                token,
                format_args!("could not parse keys '{}'", token.as_str(c)),
            ),
            CommandError::ConfigNotFound(key) => write(
                self,
                f,
                key,
                format_args!("no such config '{}'", key.as_str(c)),
            ),
            CommandError::InvalidConfigValue { key, value } => write(
                self,
                f,
                value,
                format_args!(
                    "invalid value '{}' for config '{}'",
                    value.as_str(c),
                    key.as_str(c)
                ),
            ),
            CommandError::ColorNotFound(key) => write(
                self,
                f,
                key,
                format_args!("no such theme color '{}'", key.as_str(c)),
            ),
            CommandError::InvalidColorValue { key, value } => write(
                self,
                f,
                value,
                format_args!(
                    "invalid value '{}' for theme color '{}'",
                    value.as_str(c),
                    key.as_str(c)
                ),
            ),
            CommandError::InvalidGlob(glob) => write(
                self,
                f,
                glob,
                format_args!("invalid glob '{}'", glob.as_str(c)),
            ),
            CommandError::SyntaxExpectedEquals(end) => write(
                self,
                f,
                end,
                format_args!("syntax definition expected '=' token here"),
            ),
            CommandError::SyntaxExpectedPattern(end) => write(
                self,
                f,
                end,
                format_args!("syntax definition expected a pattern here"),
            ),
            CommandError::PatternError(pattern, error) => {
                write(self, f, pattern, format_args!("{}", error))
            }
            CommandError::KeyParseError(keys, error) => {
                write(self, f, keys, format_args!("{}", error))
            }
            CommandError::LspServerNotRunning => f.write_str("lsp server not running"),
            CommandError::EvalCommandError { command, error } => {
                let error_display = CommandErrorDisplay {
                    command: &command,
                    source_path: self.source_path,
                    commands: self.commands,
                    buffers: self.buffers,
                    error: &error,
                };
                write(
                    self,
                    f,
                    &CommandToken {
                        from: 0,
                        to: c.len(),
                    },
                    format_args!("\n@ eval \n{}", error_display),
                )
            }
            CommandError::MacroCommandError {
                index,
                command,
                error,
            } => {
                let macro_command = &self.commands.macro_commands()[*index];
                let error_display = CommandErrorDisplay {
                    command: &command,
                    source_path: macro_command.source_path.as_ref().map(PathBuf::as_path),
                    commands: self.commands,
                    buffers: self.buffers,
                    error: &error,
                };
                write(
                    self,
                    f,
                    &CommandToken {
                        from: 0,
                        to: c.len(),
                    },
                    format_args!(
                        "\n@ command macro '{}':\n{}",
                        &macro_command.name, error_display
                    ),
                )
            }
        }
    }
}

type CommandFn = for<'state, 'command> fn(
    &mut CommandContext<'state, 'command>,
) -> Result<Option<CommandOperation>, CommandError>;

pub enum CommandOperation {
    Suspend,
    Quit,
    QuitAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    Commands,
    Buffers,
    Files,
    Custom(&'static [&'static str]),
}

pub struct CommandContext<'state, 'command> {
    pub editor: &'state mut Editor,
    pub platform: &'state mut Platform,
    pub clients: &'state mut ClientManager,
    pub client_handle: Option<ClientHandle>,
    pub source_path: Option<&'command Path>,
    pub args: CommandArgsBuilder<'command>,
    pub output: &'state mut String,
}
impl<'state, 'command> CommandContext<'state, 'command> {
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
}

fn find_balanced(bytes: &[u8], start: u8, end: u8) -> Option<usize> {
    let mut balance: usize = 1;
    let mut i = 0;

    loop {
        if i == bytes.len() {
            return None;
        }

        let b = bytes[i];
        if b == start {
            balance += 1;
        } else if b == end {
            balance -= 1;
            if balance == 0 {
                return Some(i);
            }
        }

        i += 1;
    }
}

struct CommandIter<'a>(pub &'a str);
impl<'a> Iterator for CommandIter<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.0 = self.0.trim_start();
            if self.0.is_empty() {
                return None;
            }

            let bytes = self.0.as_bytes();
            let mut i = 0;

            loop {
                if i == bytes.len() {
                    let command = self.0;
                    self.0 = "";
                    return Some(command);
                }

                match bytes[i] {
                    b'\n' | b';' => {
                        let command = &self.0[..i];
                        self.0 = &self.0[i + 1..];
                        if command.is_empty() {
                            break;
                        } else {
                            return Some(command);
                        }
                    }
                    b'{' => match find_balanced(&bytes[i + 1..], b'{', b'}') {
                        Some(len) => i += len + 1,
                        None => {
                            let command = self.0;
                            self.0 = "";
                            return Some(command);
                        }
                    },
                    b'#' => {
                        let command = &self.0[..i];
                        while i < bytes.len() && bytes[i] != b'\n' {
                            i += 1;
                        }
                        self.0 = &self.0[i..];
                        if command.is_empty() {
                            break;
                        } else {
                            return Some(command);
                        }
                    }
                    _ => (),
                }

                i += 1;
            }
        }
    }
}

#[derive(Clone)]
pub struct CommandTokenIter<'a> {
    raw: &'a str,
    rest: &'a str,
}
impl<'a> CommandTokenIter<'a> {
    pub fn new(command: &'a str) -> Self {
        Self {
            raw: command,
            rest: command,
        }
    }

    pub fn end_token(&self) -> CommandToken {
        let len = self.raw.len();
        CommandToken { from: len, to: len }
    }
}
impl<'a> Iterator for CommandTokenIter<'a> {
    type Item = (CommandTokenKind, CommandToken);
    fn next(&mut self) -> Option<Self::Item> {
        #[inline]
        fn current_from_index(this: &CommandTokenIter) -> usize {
            this.raw.len() - this.rest.len()
        }
        fn trim_until_boundary(s: &str) -> &str {
            match s.find(|c: char| c.is_ascii_whitespace() || matches!(c, '"' | '\'' | '{' | '=')) {
                Some(i) => &s[i..],
                None => "",
            }
        }

        self.rest = self
            .rest
            .trim_start_matches(|c: char| c.is_ascii_whitespace());
        if self.rest.is_empty() {
            return None;
        }

        match self.rest.as_bytes()[0] {
            delim @ b'"' | delim @ b'\'' => {
                self.rest = &self.rest[1..];
                match self.rest.find(delim as char) {
                    Some(i) => {
                        let from = current_from_index(self);
                        self.rest = &self.rest[i + 1..];
                        Some((
                            CommandTokenKind::String,
                            CommandToken { from, to: from + i },
                        ))
                    }
                    None => {
                        let from = current_from_index(self);
                        self.rest = "";
                        Some((
                            CommandTokenKind::Unterminated,
                            CommandToken {
                                from,
                                to: self.raw.len(),
                            },
                        ))
                    }
                }
            }
            b'{' => {
                self.rest = &self.rest[1..];
                match find_balanced(self.rest.as_bytes(), b'{', b'}') {
                    Some(i) => {
                        let from = current_from_index(self);
                        self.rest = &self.rest[i + 1..];
                        Some((
                            CommandTokenKind::String,
                            CommandToken { from, to: from + i },
                        ))
                    }
                    None => {
                        let from = current_from_index(self);
                        self.rest = "";
                        Some((
                            CommandTokenKind::Unterminated,
                            CommandToken {
                                from,
                                to: self.raw.len(),
                            },
                        ))
                    }
                }
            }
            b'%' => {
                let from = current_from_index(self);
                self.rest = trim_until_boundary(&self.rest);
                let to = current_from_index(self);
                Some((CommandTokenKind::Register, CommandToken { from, to }))
            }
            b'-' => {
                let from = current_from_index(self);
                self.rest = trim_until_boundary(&self.rest);
                let to = current_from_index(self);
                Some((CommandTokenKind::Flag, CommandToken { from, to }))
            }
            b'=' => {
                let from = current_from_index(self);
                self.rest = &self.rest[1..];
                Some((
                    CommandTokenKind::Equals,
                    CommandToken { from, to: from + 1 },
                ))
            }
            _ => {
                let from = current_from_index(self);
                self.rest = trim_until_boundary(&self.rest);
                let to = current_from_index(self);
                Some((CommandTokenKind::Identifier, CommandToken { from, to }))
            }
        }
    }
}

fn parse_register_key(raw: &str, token: CommandToken) -> Result<RegisterKey, CommandError> {
    let name_token = CommandToken {
        from: token.from + 1,
        to: token.to,
    };
    let register = name_token.as_str(raw);
    match RegisterKey::from_str(register) {
        Some(key) => Ok(key),
        None => Err(CommandError::InvalidRegisterKey(token)),
    }
}

fn assert_no_bang(bang: bool) -> Result<(), CommandError> {
    if bang {
        Err(CommandError::CommandDoesNotAcceptBang)
    } else {
        Ok(())
    }
}

fn get_flags<'a>(
    mut tokens: CommandTokenIter<'a>,
    registers: &'a RegisterCollection,
    flags: &mut [(&'static str, Option<CommandValue<'a>>)],
) -> Result<(), CommandError> {
    let raw = tokens.raw;
    loop {
        match tokens.next() {
            Some((CommandTokenKind::Identifier, _))
            | Some((CommandTokenKind::String, _))
            | Some((CommandTokenKind::Register, _)) => (),
            Some((CommandTokenKind::Flag, token)) => {
                let key = &token.as_str(raw)[1..];
                let value = match flags.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, value)) => value,
                    None => break Err(CommandError::UnknownFlag(token)),
                };

                let previous_state = tokens.rest;
                let raw_value = match tokens.next() {
                    Some((CommandTokenKind::Identifier, _))
                    | Some((CommandTokenKind::String, _)) => RawCommandValue::Literal(token),
                    Some((CommandTokenKind::Register, token)) => {
                        let register = parse_register_key(raw, token)?;
                        RawCommandValue::Register(token, register)
                    }
                    Some((CommandTokenKind::Flag, _)) => {
                        tokens.rest = previous_state;
                        RawCommandValue::Literal(token)
                    }
                    Some((CommandTokenKind::Equals, _)) => match tokens.next() {
                        Some((CommandTokenKind::Identifier, token))
                        | Some((CommandTokenKind::String, token)) => {
                            RawCommandValue::Literal(token)
                        }
                        Some((CommandTokenKind::Register, token)) => {
                            let register = parse_register_key(raw, token)?;
                            RawCommandValue::Register(token, register)
                        }
                        Some((CommandTokenKind::Flag, token))
                        | Some((CommandTokenKind::Equals, token)) => {
                            break Err(CommandError::InvalidToken(token))
                        }
                        Some((CommandTokenKind::Unterminated, token)) => {
                            break Err(CommandError::UnterminatedToken(token))
                        }
                        None => break Err(CommandError::InvalidToken(token)),
                    },
                    Some((CommandTokenKind::Unterminated, token)) => {
                        break Err(CommandError::UnterminatedToken(token))
                    }
                    None => RawCommandValue::Literal(token),
                };

                *value = Some(CommandValue::from_raw(raw, raw_value, registers));
            }
            Some((CommandTokenKind::Equals, token)) => {
                break Err(CommandError::InvalidToken(token))
            }
            Some((CommandTokenKind::Unterminated, token)) => {
                break Err(CommandError::UnterminatedToken(token))
            }
            None => break Ok(()),
        }
    }
}

fn try_next_raw_value(
    tokens: &mut CommandTokenIter,
) -> Result<Option<RawCommandValue>, CommandError> {
    let raw = tokens.raw;
    loop {
        match tokens.next() {
            Some((CommandTokenKind::Identifier, token))
            | Some((CommandTokenKind::String, token)) => {
                break Ok(Some(RawCommandValue::Literal(token)))
            }
            Some((CommandTokenKind::Register, token)) => {
                let register = parse_register_key(raw, token)?;
                break Ok(Some(RawCommandValue::Register(token, register)));
            }
            Some((CommandTokenKind::Flag, _)) => {
                let previous_state = tokens.rest;
                match tokens.next() {
                    Some((CommandTokenKind::Identifier, token))
                    | Some((CommandTokenKind::String, token)) => {
                        break Ok(Some(RawCommandValue::Literal(token)))
                    }
                    Some((CommandTokenKind::Register, token)) => {
                        let register = parse_register_key(raw, token)?;
                        break Ok(Some(RawCommandValue::Register(token, register)));
                    }
                    Some((CommandTokenKind::Flag, _)) => tokens.rest = previous_state,
                    Some((CommandTokenKind::Equals, _)) => {
                        tokens.next();
                    }
                    Some((CommandTokenKind::Unterminated, token)) => {
                        break Err(CommandError::UnterminatedToken(token))
                    }
                    None => break Ok(None),
                }
            }
            Some((CommandTokenKind::Equals, token)) => {
                break Err(CommandError::InvalidToken(token))
            }
            Some((CommandTokenKind::Unterminated, token)) => {
                break Err(CommandError::UnterminatedToken(token))
            }
            None => break Ok(None),
        }
    }
}

fn assert_empty(tokens: &mut CommandTokenIter, max: usize) -> Result<(), CommandError> {
    loop {
        match tokens.next() {
            Some((CommandTokenKind::Identifier, token))
            | Some((CommandTokenKind::String, token))
            | Some((CommandTokenKind::Register, token)) => {
                break Err(CommandError::TooManyArguments(token, max))
            }
            Some((CommandTokenKind::Flag, _)) => match tokens.next() {
                Some((CommandTokenKind::Identifier, token))
                | Some((CommandTokenKind::String, token))
                | Some((CommandTokenKind::Register, token)) => {
                    break Err(CommandError::TooManyArguments(token, max))
                }
                Some((CommandTokenKind::Flag, _)) => (),
                Some((CommandTokenKind::Equals, _)) => {
                    tokens.next();
                }
                Some((CommandTokenKind::Unterminated, token)) => {
                    break Err(CommandError::UnterminatedToken(token))
                }
                None => (),
            },
            Some((CommandTokenKind::Equals, token)) => {
                break Err(CommandError::InvalidToken(token))
            }
            Some((CommandTokenKind::Unterminated, token)) => {
                break Err(CommandError::UnterminatedToken(token))
            }
            None => break Ok(()),
        }
    }
}

pub struct CommandArgsBuilder<'a> {
    tokens: CommandTokenIter<'a>,
    bang: bool,
}
impl<'a> CommandArgsBuilder<'a> {
    pub fn with(&self, registers: &'a RegisterCollection) -> CommandArgs<'a> {
        CommandArgs {
            tokens: self.tokens.clone(),
            bang: self.bang,
            len: 0,
            registers,
        }
    }
}

pub struct CommandArgs<'a> {
    tokens: CommandTokenIter<'a>,
    bang: bool,
    len: usize,
    registers: &'a RegisterCollection,
}
impl<'a> CommandArgs<'a> {
    pub fn assert_no_bang(&self) -> Result<(), CommandError> {
        assert_no_bang(self.bang)
    }

    pub fn get_flags(
        &self,
        flags: &mut [(&'static str, Option<CommandValue<'a>>)],
    ) -> Result<(), CommandError> {
        get_flags(self.tokens.clone(), self.registers, flags)
    }

    pub fn try_next(&mut self) -> Result<Option<CommandValue<'a>>, CommandError> {
        self.len += 1;
        match try_next_raw_value(&mut self.tokens)? {
            Some(value) => Ok(Some(CommandValue::from_raw(
                self.tokens.raw,
                value,
                self.registers,
            ))),
            None => Ok(None),
        }
    }

    pub fn next(&mut self) -> Result<CommandValue<'a>, CommandError> {
        match self.try_next()? {
            Some(value) => Ok(value),
            None => Err(CommandError::TooFewArguments(self.len)),
        }
    }

    pub fn assert_empty(&mut self) -> Result<(), CommandError> {
        assert_empty(&mut self.tokens, self.len)
    }
}

#[derive(Clone, Copy)]
pub enum CommandSource {
    Builtin(usize),
    Macro(usize),
    Request(usize),
}

pub struct BuiltinCommand {
    pub name: &'static str,
    pub alias: &'static str,
    pub hidden: bool,
    pub completions: &'static [CompletionSource],
    pub func: CommandFn,
}

pub struct MacroCommand {
    pub name: String,
    pub hidden: bool,
    pub params: Vec<RegisterKey>,
    pub body: String,
    pub source_path: Option<PathBuf>,
}

pub struct RequestCommand {
    pub name: String,
    pub hidden: bool,
    pub client_handle: ClientHandle,
}

enum ParsedExpression<'command> {
    Literal(&'command str),
    Register(RegisterKey),
    Command {
        source: CommandSource,
        args: CommandArgsBuilder<'command>,
    },
}

struct ParsedStatement<'command> {
    pub target_register: Option<RegisterKey>,
    pub expression: ParsedExpression<'command>,
}

#[derive(Default)]
struct Process {
    pub alive: bool,
    pub client_handle: Option<ClientHandle>,
    pub input: Option<SharedBuf>,
    pub output: Vec<u8>,
    pub split_on_byte: Option<u8>,
    pub on_output: String,
}

pub struct CommandManager {
    builtin_commands: &'static [BuiltinCommand],
    macro_commands: Vec<MacroCommand>,
    request_commands: Vec<RequestCommand>,
    history: VecDeque<String>,
    spawned_processes: Vec<Process>,
}

impl CommandManager {
    pub fn new() -> Self {
        Self {
            builtin_commands: builtin::COMMANDS,
            macro_commands: Vec::new(),
            request_commands: Vec::new(),
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
            spawned_processes: Vec::new(),
        }
    }

    pub fn find_command(&self, name: &str) -> Option<CommandSource> {
        if let Some(i) = self.macro_commands.iter().position(|c| c.name == name) {
            return Some(CommandSource::Macro(i));
        }

        if let Some(i) = self.request_commands.iter().position(|c| c.name == name) {
            return Some(CommandSource::Request(i));
        }

        if let Some(i) = self
            .builtin_commands
            .iter()
            .position(|c| c.alias == name || c.name == name)
        {
            return Some(CommandSource::Builtin(i));
        }

        None
    }

    pub fn builtin_commands(&self) -> &[BuiltinCommand] {
        &self.builtin_commands
    }

    pub fn macro_commands(&self) -> &[MacroCommand] {
        &self.macro_commands
    }

    pub fn request_commands(&self) -> &[RequestCommand] {
        &self.request_commands
    }

    pub fn register_macro(&mut self, command: MacroCommand) {
        for m in &mut self.macro_commands {
            if m.name == command.name {
                *m = command;
                return;
            }
        }
        self.macro_commands.push(command);
    }

    pub fn register_request(&mut self, command: RequestCommand) {
        for m in &mut self.request_commands {
            if m.name == command.name {
                *m = command;
                return;
            }
        }
        self.request_commands.push(command);
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn history_entry(&self, index: usize) -> &str {
        match self.history.get(index) {
            Some(e) => &e[..],
            None => "",
        }
    }

    pub fn add_to_history(&mut self, entry: &str) {
        if entry.is_empty() || entry.starts_with(|c: char| c.is_ascii_whitespace()) {
            return;
        }
        if let Some(back) = self.history.back() {
            if back == entry {
                return;
            }
        }

        let mut s = if self.history.len() == self.history.capacity() {
            self.history.pop_front().unwrap()
        } else {
            String::new()
        };

        s.clear();
        s.push_str(entry);
        self.history.push_back(s);
    }

    pub fn eval_and_then_output<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        commands: &'command str,
        source_path: Option<&'command Path>,
    ) -> Option<CommandOperation> {
        let mut output = editor.string_pool.acquire();

        let operation = match Self::eval(
            editor,
            platform,
            clients,
            client_handle,
            commands,
            source_path,
            &mut output,
        ) {
            Ok(op) => op,
            Err((command, error)) => {
                output.clear();
                let error = error.display(command, source_path, &editor.commands, &editor.buffers);
                editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error));
                None
            }
        };

        match client_handle
            .and_then(|h| clients.get(h))
            .filter(|c| !c.has_ui())
            .map(Client::handle)
        {
            Some(handle) => {
                let mut buf = platform.buf_pool.acquire();
                ServerEvent::CommandOutput(&output).serialize(buf.write());
                let buf = buf.share();

                platform.buf_pool.release(buf.clone());
                platform.enqueue_request(PlatformRequest::WriteToClient { handle, buf });
            }
            None => {
                if !output.is_empty() {
                    editor.status_bar.write(MessageKind::Info).str(&output)
                }
            }
        }

        editor.string_pool.release(output);
        operation
    }

    pub fn eval<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        commands: &'command str,
        source_path: Option<&'command Path>,
        output: &mut String,
    ) -> Result<Option<CommandOperation>, (&'command str, CommandError)> {
        for command in CommandIter(commands) {
            let op = Self::eval_single_command(
                editor,
                platform,
                clients,
                client_handle,
                command,
                source_path,
                output,
            );
            editor.trigger_event_handlers(platform, clients);
            match op {
                Ok(Some(op)) => return Ok(Some(op)),
                Ok(None) => (),
                Err(error) => return Err((command, error)),
            }
        }
        Ok(None)
    }

    fn eval_single_command<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: &'command str,
        source_path: Option<&'command Path>,
        output: &mut String,
    ) -> Result<Option<CommandOperation>, CommandError> {
        output.clear();
        let ParsedStatement {
            target_register,
            expression,
        } = editor.commands.parse(command)?;

        let result = match expression {
            ParsedExpression::Literal(value) => {
                output.push_str(value);
                Ok(None)
            }
            ParsedExpression::Register(register) => {
                output.push_str(editor.registers.get(register));
                Ok(None)
            }
            ParsedExpression::Command {
                source: CommandSource::Builtin(i),
                args,
            } => {
                let command = editor.commands.builtin_commands[i].func;
                let mut ctx = CommandContext {
                    editor,
                    platform,
                    clients,
                    client_handle,
                    source_path,
                    args,
                    output,
                };
                command(&mut ctx)
            }
            ParsedExpression::Command {
                source: CommandSource::Macro(i),
                args,
            } => {
                assert_no_bang(args.bang)?;
                let mut tokens = args.tokens.clone();
                get_flags(tokens.clone(), &editor.registers, &mut [])?;

                let macro_command = &editor.commands.macro_commands[i];
                let body = editor.string_pool.acquire_with(&macro_command.body);

                let mut arg_count = 0;
                for &key in &macro_command.params {
                    arg_count += 1;
                    match try_next_raw_value(&mut tokens)? {
                        Some(RawCommandValue::Literal(token)) => {
                            editor.registers.set(key, token.as_str(tokens.raw))
                        }
                        Some(RawCommandValue::Register(_, register)) => {
                            editor.registers.copy(register, key)
                        }
                        None => return Err(CommandError::TooFewArguments(arg_count)),
                    }
                }
                assert_empty(&mut tokens, macro_command.params.len() as _)?;

                let result = match Self::eval(
                    editor,
                    platform,
                    clients,
                    client_handle,
                    &body,
                    source_path,
                    output,
                ) {
                    Ok(op) => Ok(op),
                    Err((command, error)) => Err(CommandError::MacroCommandError {
                        index: i,
                        command: command.into(),
                        error: Box::new(error),
                    }),
                };

                editor.string_pool.release(body);
                result
            }
            ParsedExpression::Command {
                source: CommandSource::Request(i),
                args,
            } => {
                let args = args.with(&editor.registers);
                args.assert_no_bang()?;

                let handle = editor.commands.request_commands[i].client_handle;

                let mut buf = platform.buf_pool.acquire();
                let write = buf.write();
                ServerEvent::Request(command).serialize(write);
                let buf = buf.share();
                platform.enqueue_request(PlatformRequest::WriteToClient { handle, buf });

                Ok(None)
            }
        };

        if let Some(register) = target_register {
            editor.registers.set(register, output);
            output.clear();
        }
        result
    }

    pub fn spawn_process(
        &mut self,
        platform: &mut Platform,
        client_handle: Option<ClientHandle>,
        mut command: Command,
        stdin: Option<&str>,
        on_output: Option<&str>,
        split_on_byte: Option<u8>,
    ) {
        let mut index = None;
        for (i, process) in self.spawned_processes.iter().enumerate() {
            if !process.alive {
                index = Some(i);
                break;
            }
        }
        let index = match index {
            Some(index) => index,
            None => {
                let index = self.spawned_processes.len();
                self.spawned_processes.push(Default::default());
                index
            }
        };

        let process = &mut self.spawned_processes[index];
        process.alive = true;
        process.client_handle = client_handle;
        process.output.clear();
        process.split_on_byte = split_on_byte;
        process.on_output.clear();

        match stdin {
            Some(stdin) => {
                let mut buf = platform.buf_pool.acquire();
                let writer = buf.write();
                writer.extend_from_slice(stdin.as_bytes());
                let buf = buf.share();
                platform.buf_pool.release(buf.clone());

                command.stdin(Stdio::piped());
                process.input = Some(buf);
            }
            None => {
                command.stdin(Stdio::null());
                process.input = None;
            }
        }
        match on_output {
            Some(on_output) => {
                command.stdout(Stdio::piped());
                process.on_output.push_str(on_output);
            }
            None => {
                command.stdout(Stdio::null());
            }
        }
        command.stderr(Stdio::null());

        platform.enqueue_request(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Command(index),
            command,
            buf_len: if on_output.is_some() { 4 * 1024 } else { 0 },
        });
    }

    pub fn on_process_spawned(
        &mut self,
        platform: &mut Platform,
        index: usize,
        handle: ProcessHandle,
    ) {
        if let Some(buf) = self.spawned_processes[index].input.take() {
            platform.enqueue_request(PlatformRequest::WriteToProcess { handle, buf });
            platform.enqueue_request(PlatformRequest::CloseProcessInput { handle });
        }
    }

    pub fn on_process_output(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        index: usize,
        bytes: &[u8],
    ) {
        let process = &mut editor.commands.spawned_processes[index];
        if process.on_output.is_empty() {
            return;
        }
        process.output.extend_from_slice(bytes);
        let split_on_byte = match process.split_on_byte {
            Some(b) => b,
            None => return,
        };

        let client_handle = process.client_handle;
        let commands = editor.string_pool.acquire_with(&process.on_output);
        let mut output_index = 0;

        loop {
            let process = &editor.commands.spawned_processes[index];
            let stdout = &process.output[output_index..];
            let slice = match stdout.iter().position(|&b| b == split_on_byte) {
                Some(i) => {
                    output_index += i + 1;
                    &stdout[..i]
                }
                None => break,
            };

            if slice.is_empty() {
                continue;
            }

            match std::str::from_utf8(slice) {
                Ok(slice) => {
                    editor.registers.set(RETURN_REGISTER, slice);
                    Self::eval_and_then_output(
                        editor,
                        platform,
                        clients,
                        client_handle,
                        &commands,
                        None,
                    );
                }
                Err(error) => {
                    editor
                        .status_bar
                        .write(MessageKind::Error)
                        .fmt(format_args!("{}", error));
                }
            }
        }

        editor.string_pool.release(commands);
        editor.commands.spawned_processes[index]
            .output
            .drain(..output_index);
    }

    pub fn on_process_exit(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        index: usize,
    ) {
        let process = &mut editor.commands.spawned_processes[index];
        process.alive = false;
        if process.on_output.is_empty() {
            return;
        }
        if process.output.is_empty() && process.split_on_byte.is_some() {
            return;
        }

        match std::str::from_utf8(&process.output) {
            Ok(stdout) => editor.registers.set(RETURN_REGISTER, stdout),
            Err(error) => {
                editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error));
                return;
            }
        }

        let client_handle = process.client_handle;
        let commands = editor.string_pool.acquire_with(&process.on_output);
        Self::eval_and_then_output(editor, platform, clients, client_handle, &commands, None);
        editor.string_pool.release(commands);
    }

    fn parse<'a>(&self, raw: &'a str) -> Result<ParsedStatement<'a>, CommandError> {
        let mut tokens = CommandTokenIter::new(raw);

        let mut target_register = None;
        let (command_token, command_name) = loop {
            match tokens.next() {
                Some((CommandTokenKind::Identifier, token)) => break (token, token.as_str(raw)),
                Some((CommandTokenKind::String, token)) => match tokens.next() {
                    Some((_, token)) => return Err(CommandError::InvalidToken(token)),
                    None => {
                        return Ok(ParsedStatement {
                            target_register,
                            expression: ParsedExpression::Literal(token.as_str(raw)),
                        })
                    }
                },
                Some((CommandTokenKind::Register, token)) => {
                    let register = parse_register_key(raw, token)?;
                    match target_register {
                        Some(_) => match tokens.next() {
                            Some((_, token)) => return Err(CommandError::InvalidToken(token)),
                            None => {
                                return Ok(ParsedStatement {
                                    target_register,
                                    expression: ParsedExpression::Register(register),
                                })
                            }
                        },
                        None => match tokens.next() {
                            Some((CommandTokenKind::Equals, _)) => target_register = Some(register),
                            Some((_, token)) => return Err(CommandError::InvalidToken(token)),
                            None => {
                                return Ok(ParsedStatement {
                                    target_register,
                                    expression: ParsedExpression::Register(register),
                                })
                            }
                        },
                    }
                }
                Some((_, token)) => return Err(CommandError::InvalidCommandName(token)),
                None => return Err(CommandError::InvalidCommandName(tokens.end_token())),
            }
        };

        let (command_name, bang) = match command_name.strip_suffix('!') {
            Some(command_name) => (command_name, true),
            None => (command_name, false),
        };
        if command_name.is_empty() {
            return Err(CommandError::InvalidCommandName(command_token));
        }

        let source = match self.find_command(command_name) {
            Some(source) => source,
            None => return Err(CommandError::CommandNotFound(command_token)),
        };
        Ok(ParsedStatement {
            target_register,
            expression: ParsedExpression::Command {
                source,
                args: CommandArgsBuilder { tokens, bang },
            },
        })
    }
}

pub fn parse_process_command(
    registers: &RegisterCollection,
    command: &str,
    environment: &str,
) -> Result<Command, CommandError> {
    let mut command_tokens = CommandTokenIter::new(command);
    let command_name = match command_tokens.next() {
        Some((CommandTokenKind::Identifier, token))
        | Some((CommandTokenKind::String, token))
        | Some((CommandTokenKind::Flag, token))
        | Some((CommandTokenKind::Equals, token)) => token.as_str(command),
        Some((CommandTokenKind::Register, token)) => {
            let register = parse_register_key(command, token)?;
            registers.get(register)
        }
        Some((CommandTokenKind::Unterminated, token)) => {
            return Err(CommandError::UnterminatedToken(token))
        }
        None => return Err(CommandError::InvalidToken(command_tokens.end_token())),
    };

    let mut process_command = Command::new(command_name);
    while let Some((kind, token)) = command_tokens.next() {
        let arg = match kind {
            CommandTokenKind::Identifier
            | CommandTokenKind::String
            | CommandTokenKind::Flag
            | CommandTokenKind::Equals => token.as_str(command),
            CommandTokenKind::Register => {
                let register = parse_register_key(command, token)?;
                registers.get(register)
            }
            CommandTokenKind::Unterminated => return Err(CommandError::InvalidToken(token)),
        };
        process_command.arg(arg);
    }

    let mut environment_tokens = CommandTokenIter::new(environment);
    loop {
        let key = match environment_tokens.next() {
            Some((CommandTokenKind::Identifier, token))
            | Some((CommandTokenKind::String, token)) => token.as_str(environment),
            Some((CommandTokenKind::Register, token)) => {
                let register = parse_register_key(environment, token)?;
                registers.get(register)
            }
            Some((CommandTokenKind::Flag, token)) | Some((CommandTokenKind::Equals, token)) => {
                return Err(CommandError::InvalidToken(token))
            }
            Some((CommandTokenKind::Unterminated, token)) => {
                return Err(CommandError::UnterminatedToken(token))
            }
            None => break,
        };
        match environment_tokens.next() {
            Some((CommandTokenKind::Equals, _)) => (),
            Some((_, token)) => return Err(CommandError::InvalidToken(token)),
            None => {
                return Err(CommandError::UnterminatedToken(
                    environment_tokens.end_token(),
                ))
            }
        }
        let value = match environment_tokens.next() {
            Some((CommandTokenKind::Identifier, token))
            | Some((CommandTokenKind::String, token)) => token.as_str(environment),
            Some((CommandTokenKind::Register, token)) => {
                let register = parse_register_key(environment, token)?;
                registers.get(register)
            }
            Some((CommandTokenKind::Flag, token)) | Some((CommandTokenKind::Equals, token)) => {
                return Err(CommandError::InvalidToken(token))
            }
            Some((CommandTokenKind::Unterminated, token)) => {
                return Err(CommandError::UnterminatedToken(token))
            }
            None => {
                return Err(CommandError::UnterminatedToken(
                    environment_tokens.end_token(),
                ))
            }
        };

        process_command.env(key, value);
    }

    Ok(process_command)
}

#[cfg(test)]
mod tests {
    use super::*;

    static EMPTY_REGISTERS: RegisterCollection = RegisterCollection::new();

    fn create_commands() -> CommandManager {
        let builtin_commands = &[BuiltinCommand {
            name: "command-name",
            alias: "c",
            hidden: false,
            completions: &[],
            func: |_| Ok(None),
        }];

        CommandManager {
            builtin_commands,
            macro_commands: Vec::new(),
            request_commands: Vec::new(),
            history: Default::default(),
            spawned_processes: Vec::new(),
        }
    }

    #[test]
    fn operation_size() {
        assert_eq!(1, std::mem::size_of::<CommandOperation>());
        assert_eq!(1, std::mem::size_of::<Option<CommandOperation>>());
    }

    #[test]
    fn command_tokens() {
        let command = "value -flag";
        let mut tokens = CommandTokenIter::new(command);
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Identifier, token)) if token.as_str(command) == "value",
        ));
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Flag, token)) if token.as_str(command) == "-flag",
        ));
        assert!(tokens.next().is_none());

        let command = "value --long-flag";
        let mut tokens = CommandTokenIter::new(command);
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Identifier, token)) if token.as_str(command) == "value",
        ));
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Flag, token)) if token.as_str(command) == "--long-flag",
        ));
        assert!(tokens.next().is_none());
    }

    #[test]
    fn command_parsing() {
        fn assert_bang(commands: &CommandManager, command: &str, expect_bang: bool) {
            let (source, args) = match commands.parse(command) {
                Ok(ParsedStatement {
                    expression: ParsedExpression::Command { source, args },
                    ..
                }) => (source, args),
                _ => panic!("command parse error at '{}'", command),
            };
            assert!(matches!(source, CommandSource::Builtin(0)));
            assert_eq!(expect_bang, args.bang);
        }

        let commands = create_commands();
        assert_bang(&commands, "command-name", false);
        assert_bang(&commands, "  command-name  ", false);
        assert_bang(&commands, "  command-name!  ", true);
        assert_bang(&commands, "  command-name!", true);
    }

    #[test]
    fn arg_parsing() {
        fn parse_args<'a>(commands: &CommandManager, command: &'a str) -> CommandArgs<'a> {
            match commands.parse(command) {
                Ok(ParsedStatement {
                    expression: ParsedExpression::Command { args, .. },
                    ..
                }) => args.with(&EMPTY_REGISTERS),
                _ => panic!("command '{}' parse error", command),
            }
        }

        fn collect<'a>(mut args: CommandArgs<'a>) -> Vec<&'a str> {
            let mut values = Vec::new();
            loop {
                match args.try_next() {
                    Ok(Some(arg)) => values.push(arg.text),
                    Ok(None) => break,
                    Err(error) => {
                        let discriminant = std::mem::discriminant(&error);
                        panic!("error parsing args {:?}", discriminant);
                    }
                }
            }
            values
        }

        let commands = create_commands();
        let args = parse_args(&commands, "c  aaa  bbb  ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);
        let args = parse_args(&commands, "c  'aaa'  \"bbb\"  ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);
        let args = parse_args(&commands, "c  \"aaa\"\"bbb\"ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);
        let args = parse_args(&commands, "c  {aaa}{bbb}ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);
        let args = parse_args(&commands, "c  {aaa}{{bb}b}ccc  ");
        assert_eq!(["aaa", "{bb}b", "ccc"], &collect(args)[..]);

        fn flag_value<'a>(
            flags: &[(&str, Option<CommandValue<'a>>)],
            index: usize,
        ) -> Option<&'a str> {
            flags[index].1.as_ref().map(|f| f.text)
        }

        let args = parse_args(&commands, "c -option=value aaa");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(None, flag_value(&flags, 0));
        assert_eq!(Some("value"), flag_value(&flags, 1));
        assert_eq!(["aaa"], &collect(args)[..]);

        let args = parse_args(&commands, "c 'aaa' -option=value");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(None, flag_value(&flags, 0));
        assert_eq!(Some("value"), flag_value(&flags, 1));
        assert_eq!(["aaa"], &collect(args)[..]);

        let args = parse_args(&commands, "c aaa -switch bbb -option=value ccc");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(Some("-switch"), flag_value(&flags, 0));
        assert_eq!(Some("value"), flag_value(&flags, 1));
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);

        let args = parse_args(&commands, "c -switch -option=value aaa");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(Some("-switch"), flag_value(&flags, 0));
        assert_eq!(Some("value"), flag_value(&flags, 1));
        assert_eq!(["aaa"], &collect(args)[..]);
    }

    #[test]
    fn command_parsing_fail() {
        let commands = create_commands();

        macro_rules! assert_fail {
            ($command:expr, $error_pattern:pat => $value:ident == $expect:expr) => {
                let command = $command;
                match commands.parse(command) {
                    Ok(_) => panic!("command parsed successfully"),
                    Err($error_pattern) => assert_eq!($expect, $value.as_str(command)),
                    Err(_) => panic!("other error occurred"),
                }
            };
        }

        assert_fail!("", CommandError::InvalidCommandName(s) => s == "");
        assert_fail!("   ", CommandError::InvalidCommandName(s) => s == "");
        assert_fail!(" !", CommandError::InvalidCommandName(s) => s == "!");
        assert_fail!("!  'aa'", CommandError::InvalidCommandName(s) => s == "!");
        assert_fail!("  a \"bb\"", CommandError::CommandNotFound(s) => s == "a");

        fn assert_unterminated(args: &str) {
            let args = CommandArgsBuilder {
                tokens: CommandTokenIter::new(args),
                bang: false,
            };
            let mut args = args.with(&EMPTY_REGISTERS);

            loop {
                match args.try_next() {
                    Ok(Some(_)) => (),
                    Ok(None) => panic!("no unterminated token"),
                    Err(CommandError::UnterminatedToken(_)) => return,
                    Err(_) => panic!("other error"),
                }
            }
        }

        assert_unterminated("0 1 'abc");
        assert_unterminated("0 1 '");
        assert_unterminated("0 1 \"'");
    }

    #[test]
    fn test_find_balanced() {
        assert_eq!(None, find_balanced(b"", b'{', b'}'));
        assert_eq!(Some(0), find_balanced(b"}", b'{', b'}'));
        assert_eq!(Some(2), find_balanced(b"  }}", b'{', b'}'));
        assert_eq!(Some(2), find_balanced(b"{}}", b'{', b'}'));
        assert_eq!(Some(4), find_balanced(b"{{}}}", b'{', b'}'));
    }

    #[test]
    fn multi_command_line_parsing() {
        let mut commands = CommandIter("command0\ncommand1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0\n\n\ncommand1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0 {\n still command0\n}\ncommand1");
        assert_eq!(Some("command0 {\n still command0\n}"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0 }}} {\n {\n still command0\n}\n}\ncommand1");
        assert_eq!(
            Some("command0 }}} {\n {\n still command0\n}\n}"),
            commands.next()
        );
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("   #command0");
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0 # command1");
        assert_eq!(Some("command0 "), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("    # command0\ncommand1");
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands =
            CommandIter("command0# comment\n\n# more comment\n\n# one more comment\ncommand1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter("command0;command1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter(";;  command0;   ;;command1   ;");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1   "), commands.next());
        assert_eq!(None, commands.next());
    }
}

// ========================

mod compiled {
    use std::ops::Range;

    pub enum CommandCompileError {
        UnterminatedQuotedLiteral(CommandTokenRange),
        InvalidFlagName(CommandTokenRange),
        InvalidVariableName(CommandTokenRange),
    }

    pub enum CommandTokenKind {
        Literal,
        QuotedLiteral(bool),
        Flag,
        Equals,
        Variable,
        OpenCurlyBrackets,
        CloseCurlyBrackets,
        OpenParenthesis,
        CloseParenthesis,
        EndOfStatement,
    }

    pub struct CommandTokenRange {
        pub from: usize,
        pub to: usize,
    }

    pub struct CommandToken {
        pub kind: CommandTokenKind,
        pub range: CommandTokenRange,
    }

    pub struct CommandTokenIter<'a> {
        bytes: &'a [u8],
        index: usize,
    }
    impl<'a> Iterator for CommandTokenIter<'a> {
        type Item = Result<CommandToken, CommandCompileError>;
        fn next(&mut self) -> Option<Self::Item> {
            fn error(
                iter: &mut CommandTokenIter,
                error: CommandCompileError,
            ) -> CommandCompileError {
                iter.index = iter.bytes.len();
                error
            }
            fn consume_identifier(iter: &mut CommandTokenIter) {
                let bytes = &iter.bytes[iter.index..];
                let len = match bytes.iter().position(
                    |b| !matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-'),
                ) {
                    Some(len) => len,
                    None => bytes.len(),
                };
                iter.index += len;
            }
            fn single_byte_token(
                iter: &mut CommandTokenIter,
                kind: CommandTokenKind,
            ) -> Option<Result<CommandToken, CommandCompileError>> {
                let from = iter.index;
                iter.index += 1;
                let range = CommandTokenRange {
                    from,
                    to: iter.index,
                };
                Some(Ok(CommandToken { kind, range }))
            }

            loop {
                if self.index >= self.bytes.len() {
                    return None;
                }
                if matches!(self.bytes[self.index], b' ' | b'\t') {
                    self.index += 1;
                } else {
                    break;
                }
            }

            loop {
                match self.bytes[self.index] {
                    delim @ b'"' | delim @ b'\'' => {
                        let from = self.index;
                        self.index += 1;
                        let mut has_escaping = false;
                        loop {
                            if self.index >= self.bytes.len() {
                                return Some(Err(error(
                                    self,
                                    CommandCompileError::UnterminatedQuotedLiteral(
                                        CommandTokenRange {
                                            from,
                                            to: self.bytes.len(),
                                        },
                                    ),
                                )));
                            }

                            let byte = self.bytes[self.index];
                            if byte == b'\\' {
                                has_escaping = true;
                                self.index += 2;
                            } else {
                                self.index += 1;
                                if byte == delim {
                                    break;
                                }
                            }
                        }
                        self.index += 1;
                        let to = self.index;
                        let range = CommandTokenRange { from, to };
                        break Some(Ok(CommandToken {
                            kind: CommandTokenKind::QuotedLiteral(has_escaping),
                            range,
                        }));
                    }
                    b'-' => {
                        let from = self.index;
                        self.index += 1;
                        consume_identifier(self);
                        let to = self.index;
                        let range = CommandTokenRange { from, to };
                        if range.from + 1 == range.to {
                            break Some(Err(error(
                                self,
                                CommandCompileError::InvalidFlagName(range),
                            )));
                        } else {
                            break Some(Ok(CommandToken {
                                kind: CommandTokenKind::Flag,
                                range,
                            }));
                        }
                    }
                    b'$' => {
                        let from = self.index;
                        self.index += 1;
                        consume_identifier(self);
                        let to = self.index;
                        let range = CommandTokenRange { from, to };
                        if range.from + 1 == range.to {
                            break Some(Err(error(
                                self,
                                CommandCompileError::InvalidVariableName(range),
                            )));
                        } else {
                            break Some(Ok(CommandToken {
                                kind: CommandTokenKind::Variable,
                                range,
                            }));
                        }
                    }
                    b'=' => break single_byte_token(self, CommandTokenKind::Equals),
                    b'{' => break single_byte_token(self, CommandTokenKind::OpenCurlyBrackets),
                    b'}' => break single_byte_token(self, CommandTokenKind::CloseCurlyBrackets),
                    b'(' => break single_byte_token(self, CommandTokenKind::OpenParenthesis),
                    b')' => break single_byte_token(self, CommandTokenKind::CloseParenthesis),
                    b'\\' => {
                        let from = self.index;
                        self.index += 1;
                        match &self.bytes[self.index..] {
                            b"\r" | b"\n" => self.index += 1,
                            b"\r\n" => self.index += 2,
                            _ => {
                                let to = self.index;
                                break Some(Ok(CommandToken {
                                    kind: CommandTokenKind::Literal,
                                    range: CommandTokenRange { from, to },
                                }));
                            }
                        }
                    }
                    b'\r' | b'\n' | b';' => {
                        let token = single_byte_token(self, CommandTokenKind::EndOfStatement);
                        while self.index < self.bytes.len()
                            && matches!(self.bytes[self.index], b'\r' | b'\n' | b';')
                        {
                            self.index += 1;
                        }
                        break token;
                    }
                    _ => {
                        let from = self.index;
                        self.index += 1;
                        while self.index < self.bytes.len() {
                            match self.bytes[self.index] {
                                b'{' | b'(' | b' ' | b'\t' => break,
                                _ => self.index += 1,
                            }
                        }
                        let to = self.index;
                        let range = CommandTokenRange { from, to };
                        break Some(Ok(CommandToken {
                            kind: CommandTokenKind::Literal,
                            range,
                        }));
                    }
                }
            }
        }
    }

    enum Op {
        BuiltinCommand,
        MacroCommand,
        RequestCommand,
    }

    struct MacroCommand {
        name_range: Range<u32>,
        chunk: ByteCodeChunk,
    }

    struct MacroCommandCollection {
        names: String,
        commands: Vec<MacroCommand>,
    }

    struct ByteCodeChunk {
        ops: Vec<Op>,
        texts: String,
    }

    fn compile(commands: &str, chunk: &mut ByteCodeChunk) {
        chunk.ops.clear();
        todo!();
    }
}

