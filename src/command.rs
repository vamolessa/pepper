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
    pattern::PatternError,
    platform::{Platform, PlatformRequest, ProcessHandle, ProcessTag, SharedBuf},
    serialization::Serialize,
};

mod builtin;

pub const HISTORY_CAPACITY: usize = 10;

const START_OF_TEXT_BYTE: u8 = b'\x02';
const END_OF_TEXT_BYTE: u8 = b'\x03';

#[derive(Clone, Copy)]
pub struct CommandToken {
    location: usize,
    len: usize,
}
impl CommandToken {
    fn as_str_at<'command>(&self, command: &'command str, location: usize) -> &'command str {
        let start = self.location - location;
        let end = start + self.len;
        &command[start..end]
    }
}
impl<'a> From<&'a str> for CommandToken {
    fn from(s: &'a str) -> Self {
        Self {
            location: s.as_ptr() as _,
            len: s.len(),
        }
    }
}

pub enum CommandError {
    InvalidCommandName(CommandToken),
    CommandNotFound(CommandToken),
    CommandDoesNotAcceptBang,
    UnterminatedToken(CommandToken),
    InvalidToken(CommandToken),
    TooFewArguments(CommandToken, u8),
    TooManyArguments(CommandToken, u8),
    UnknownFlag(CommandToken),
    UnsavedChanges,
    NoBufferOpened,
    InvalidBufferHandle(BufferHandle),
    InvalidPath(CommandToken),
    ParseArgError {
        arg: CommandToken,
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
    PatternError(CommandToken, PatternError),
    KeyParseError(CommandToken, KeyParseError),
    InvalidRegisterKey(CommandToken),
    LspServerNotRunning,
    MacroCommandError {
        index: usize,
        commands: String,
        location: usize,
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
            location: command.as_ptr() as usize,
            commands,
            buffers,
            error: self,
        }
    }
}

pub struct CommandErrorDisplay<'command, 'error> {
    command: &'command str,
    source_path: Option<&'command Path>,
    location: usize,
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
            let error_offset = error_token.location - this.location;

            let error_len = this.command[error_offset..(error_offset + error_token.len)]
                .chars()
                .count()
                .max(1);
            let error_offset = this
                .command
                .char_indices()
                .take_while(|(i, _)| *i < error_offset)
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
        let l = self.location;

        match self.error {
            CommandError::InvalidCommandName(token) => write(
                self,
                f,
                token,
                format_args!("invalid command name '{}'", token.as_str_at(c, l)),
            ),
            CommandError::CommandNotFound(command) => write(
                self,
                f,
                command,
                format_args!("no such command '{}'", command.as_str_at(c, l)),
            ),
            CommandError::CommandDoesNotAcceptBang => write(
                self,
                f,
                &c.trim().into(),
                format_args!("command does not accept bang"),
            ),
            CommandError::UnterminatedToken(token) => {
                write(self, f, token, format_args!("unterminated token"))
            }
            CommandError::InvalidToken(token) => write(
                self,
                f,
                token,
                format_args!("invalid token '{}'", token.as_str_at(c, l)),
            ),
            CommandError::TooFewArguments(token, min) => write(
                self,
                f,
                token,
                format_args!("command expects at least {} arguments", min),
            ),
            CommandError::TooManyArguments(token, max) => write(
                self,
                f,
                token,
                format_args!("command expects at most {} arguments", max),
            ),
            CommandError::UnknownFlag(token) => write(
                self,
                f,
                token,
                format_args!("unknown flag '{}'", token.as_str_at(c, l)),
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
                format_args!("invalid path '{}'", path.as_str_at(c, l)),
            ),
            CommandError::ParseArgError { arg, type_name } => write(
                self,
                f,
                arg,
                format_args!("could not parse '{}' as {}", arg.as_str_at(c, l), type_name),
            ),
            CommandError::OpenFileError { path, error } => write(
                self,
                f,
                path,
                format_args!("could not open file '{}': {}", path.as_str_at(c, l), error),
            ),
            CommandError::BufferError(handle, error) => match self.buffers.get(*handle) {
                Some(buffer) => write!(f, "{}", error.display(buffer)),
                None => Ok(()),
            },
            CommandError::BufferedKeysParseError(token) => write(
                self,
                f,
                token,
                format_args!("could not parse keys '{}'", token.as_str_at(c, l)),
            ),
            CommandError::ConfigNotFound(key) => write(
                self,
                f,
                key,
                format_args!("no such config '{}'", key.as_str_at(c, l)),
            ),
            CommandError::InvalidConfigValue { key, value } => write(
                self,
                f,
                value,
                format_args!(
                    "invalid value '{}' for config '{}'",
                    value.as_str_at(c, l),
                    key.as_str_at(c, l)
                ),
            ),
            CommandError::ColorNotFound(key) => write(
                self,
                f,
                key,
                format_args!("no such theme color '{}'", key.as_str_at(c, l)),
            ),
            CommandError::InvalidColorValue { key, value } => write(
                self,
                f,
                value,
                format_args!(
                    "invalid value '{}' for theme color '{}'",
                    value.as_str_at(c, l),
                    key.as_str_at(c, l)
                ),
            ),
            CommandError::InvalidGlob(glob) => write(
                self,
                f,
                glob,
                format_args!("invalid glob '{}'", glob.as_str_at(c, l)),
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
            CommandError::InvalidRegisterKey(key) => write(
                self,
                f,
                key,
                format_args!("invalid register key '{}'", key.as_str_at(c, l)),
            ),
            CommandError::LspServerNotRunning => f.write_str("lsp server not running"),
            CommandError::MacroCommandError {
                index,
                commands: body,
                location,
                error,
            } => {
                let command = &self.commands.macro_commands()[*index];
                let error_display = CommandErrorDisplay {
                    command: body,
                    source_path: command.source_path.as_ref().map(PathBuf::as_path),
                    location: *location,
                    commands: self.commands,
                    buffers: self.buffers,
                    error,
                };
                write(
                    self,
                    f,
                    &c.trim().into(),
                    format_args!("\n@ command macro '{}':\n{}", &command.name, error_display),
                )
            }
        }
    }
}

type CommandFn = for<'state, 'command> fn(
    &mut CommandContext<'state, 'command>,
) -> Result<Option<CommandOperation>, CommandError>;

pub enum CommandOperation {
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
    pub args: CommandArgs<'command>,
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

pub fn replace_to_between_text_markers(text: &mut String, from: &str, to: &str) {
    let from_len = from.len();
    let to_len = to.len();
    let mut offset = 0;
    while let Some(i) = text[offset..].find(from) {
        offset += i;
        text.insert(offset, START_OF_TEXT_BYTE as char);
        offset += 1;
        text.replace_range(offset..(offset + from_len), to);
        offset += to_len;
        text.insert(offset, END_OF_TEXT_BYTE as char);
        offset += 1;
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

pub struct CommandIter<'a>(pub &'a str);
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
                    b'\n' => {
                        let (command, rest) = self.0.split_at(i);
                        self.0 = rest;
                        if command.is_empty() {
                            break;
                        } else {
                            return Some(command);
                        }
                    }
                    b';' => {
                        let command = &self.0[..i];
                        self.0 = &self.0[(i + 1)..];
                        if command.is_empty() {
                            break;
                        } else {
                            return Some(command);
                        }
                    }
                    b'{' => match find_balanced(&bytes[(i + 1)..], b'{', b'}') {
                        Some(len) => i += len + 1,
                        None => {
                            let command = self.0;
                            self.0 = "";
                            return Some(command);
                        }
                    },
                    START_OF_TEXT_BYTE => {
                        match find_balanced(&bytes[(i + 1)..], START_OF_TEXT_BYTE, END_OF_TEXT_BYTE)
                        {
                            Some(len) => i += len + 1,
                            None => {
                                let command = self.0;
                                self.0 = "";
                                return Some(command);
                            }
                        }
                    }
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

#[derive(Clone, Copy)]
pub enum CommandTokenKind {
    Text,
    Flag,
    Equals,
    Unterminated,
}
pub struct CommandTokenIter<'a>(pub &'a str);
impl<'a> Iterator for CommandTokenIter<'a> {
    type Item = (CommandTokenKind, &'a str);
    fn next(&mut self) -> Option<Self::Item> {
        fn split_at_boundary(s: &str) -> (&str, &str) {
            match s.find(|c: char| c.is_ascii_whitespace() || matches!(c, '"' | '\'' | '{' | '=')) {
                Some(i) => s.split_at(i),
                None => (s, &s[s.len()..]),
            }
        }

        self.0 = self.0.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if self.0.is_empty() {
            return None;
        }

        match self.0.as_bytes()[0] {
            delim @ b'"' | delim @ b'\'' => {
                self.0 = &self.0[1..];
                match self.0.find(delim as char) {
                    Some(i) => {
                        let token = &self.0[..i];
                        self.0 = &self.0[(i + 1)..];
                        Some((CommandTokenKind::Text, token))
                    }
                    None => {
                        let token = self.0;
                        self.0 = &self.0[self.0.len()..];
                        Some((CommandTokenKind::Unterminated, token))
                    }
                }
            }
            b'{' => {
                self.0 = &self.0[1..];
                match find_balanced(self.0.as_bytes(), b'{', b'}') {
                    Some(i) => {
                        let token = &self.0[..i];
                        self.0 = &self.0[(i + 1)..];
                        Some((CommandTokenKind::Text, token))
                    }
                    None => {
                        let token = self.0;
                        self.0 = &self.0[self.0.len()..];
                        Some((CommandTokenKind::Unterminated, token))
                    }
                }
            }
            START_OF_TEXT_BYTE => {
                self.0 = &self.0[1..];
                match find_balanced(self.0.as_bytes(), START_OF_TEXT_BYTE, END_OF_TEXT_BYTE) {
                    Some(i) => {
                        let token = &self.0[..i];
                        self.0 = &self.0[(i + 1)..];
                        Some((CommandTokenKind::Text, token))
                    }
                    None => {
                        let token = self.0;
                        self.0 = &self.0[self.0.len()..];
                        Some((CommandTokenKind::Unterminated, token))
                    }
                }
            }
            b'-' => {
                let (token, rest) = split_at_boundary(&self.0);
                self.0 = rest;
                Some((CommandTokenKind::Flag, token))
            }
            b'=' => {
                let (token, rest) = self.0.split_at(1);
                self.0 = rest;
                Some((CommandTokenKind::Equals, token))
            }
            _ => {
                let (token, rest) = split_at_boundary(self.0);
                self.0 = rest;
                Some((CommandTokenKind::Text, token))
            }
        }
    }
}
pub struct CommandArgs<'a> {
    pub bang: bool,
    tokens: CommandTokenIter<'a>,
    len: u8,
}
impl<'a> CommandArgs<'a> {
    pub fn assert_no_bang(&self) -> Result<(), CommandError> {
        if self.bang {
            Err(CommandError::CommandDoesNotAcceptBang)
        } else {
            Ok(())
        }
    }

    pub fn get_flags(
        &self,
        flags: &mut [(&'static str, Option<&'a str>)],
    ) -> Result<(), CommandError> {
        let mut tokens = CommandTokenIter(self.tokens.0);
        loop {
            let token = match tokens.next() {
                Some(token) => token,
                None => break Ok(()),
            };
            match token {
                (CommandTokenKind::Text, _) => (),
                (CommandTokenKind::Flag, key) => {
                    let key = &key[1..];
                    let value = match flags.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, value)) => value,
                        None => break Err(CommandError::UnknownFlag(key.into())),
                    };

                    let previous_state = tokens.0;
                    match tokens.next() {
                        Some((CommandTokenKind::Text, _)) => *value = Some(&key[key.len()..]),
                        Some((CommandTokenKind::Flag, _)) => {
                            *value = Some(&key[key.len()..]);
                            tokens.0 = previous_state;
                        }
                        Some((CommandTokenKind::Equals, token)) => match tokens.next() {
                            Some((CommandTokenKind::Text, token)) => *value = Some(token),
                            Some((CommandTokenKind::Flag, token))
                            | Some((CommandTokenKind::Equals, token)) => {
                                break Err(CommandError::InvalidToken(token.into()))
                            }
                            Some((CommandTokenKind::Unterminated, token)) => {
                                break Err(CommandError::UnterminatedToken(token.into()))
                            }
                            None => break Err(CommandError::InvalidToken(token.into())),
                        },
                        Some((CommandTokenKind::Unterminated, token)) => {
                            break Err(CommandError::UnterminatedToken(token.into()))
                        }
                        None => {
                            *value = Some(&key[key.len()..]);
                            break Ok(());
                        }
                    }
                }
                (CommandTokenKind::Equals, token) => {
                    return Err(CommandError::InvalidToken(token.into()))
                }
                (CommandTokenKind::Unterminated, token) => {
                    return Err(CommandError::UnterminatedToken(token.into()))
                }
            }
        }
    }

    pub fn try_next(&mut self) -> Result<Option<&'a str>, CommandError> {
        self.len += 1;
        loop {
            match self.tokens.next() {
                Some((CommandTokenKind::Text, arg)) => break Ok(Some(arg)),
                Some((CommandTokenKind::Flag, _)) => {
                    let previous_state = self.tokens.0;
                    match self.tokens.next() {
                        Some((CommandTokenKind::Text, arg)) => break Ok(Some(arg)),
                        Some((CommandTokenKind::Flag, _)) => self.tokens.0 = previous_state,
                        Some((CommandTokenKind::Equals, _)) => {
                            self.tokens.next();
                        }
                        Some((CommandTokenKind::Unterminated, token)) => {
                            break Err(CommandError::UnterminatedToken(token.into()))
                        }
                        None => break Ok(None),
                    }
                }
                Some((CommandTokenKind::Equals, token)) => {
                    break Err(CommandError::InvalidToken(token.into()))
                }
                Some((CommandTokenKind::Unterminated, token)) => {
                    break Err(CommandError::UnterminatedToken(token.into()))
                }
                None => break Ok(None),
            }
        }
    }

    pub fn next(&mut self) -> Result<&'a str, CommandError> {
        match self.try_next()? {
            Some(arg) => Ok(arg),
            None => Err(CommandError::TooFewArguments(
                self.tokens.0.into(),
                self.len,
            )),
        }
    }

    pub fn assert_empty(&mut self) -> Result<(), CommandError> {
        loop {
            match self.tokens.next() {
                Some((CommandTokenKind::Text, token)) => {
                    break Err(CommandError::TooManyArguments(token.into(), self.len))
                }
                Some((CommandTokenKind::Flag, _)) => match self.tokens.next() {
                    Some((CommandTokenKind::Text, token)) => {
                        break Err(CommandError::TooManyArguments(token.into(), self.len))
                    }
                    Some((CommandTokenKind::Flag, _)) => (),
                    Some((CommandTokenKind::Equals, _)) => {
                        self.tokens.next();
                    }
                    Some((CommandTokenKind::Unterminated, token)) => {
                        break Err(CommandError::UnterminatedToken(token.into()))
                    }
                    None => (),
                },
                Some((CommandTokenKind::Equals, token)) => {
                    break Err(CommandError::InvalidToken(token.into()))
                }
                Some((CommandTokenKind::Unterminated, token)) => {
                    break Err(CommandError::UnterminatedToken(token.into()))
                }
                None => break Ok(()),
            }
        }
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
    pub help: &'static str,
    pub hidden: bool,
    pub completions: &'static [CompletionSource],
    pub func: CommandFn,
}

pub struct MacroCommand {
    pub name: String,
    pub help: String,
    pub hidden: bool,
    pub params: Vec<String>,
    pub commands: String,
    pub source_path: Option<PathBuf>,
}

pub struct RequestCommand {
    pub name: String,
    pub help: String,
    pub hidden: bool,
    pub client_handle: ClientHandle,
}

#[derive(Default)]
struct Process {
    pub alive: bool,
    pub client_handle: Option<ClientHandle>,
    pub input: Option<SharedBuf>,
    pub output: Vec<u8>,
    pub split_on_byte: Option<u8>,
    pub output_var_name: String,
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
        if entry.is_empty() {
            return;
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

    pub fn eval_commands_then_output<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        commands: &'command str,
        source_path: Option<&'command Path>,
    ) -> Option<CommandOperation> {
        let mut output = editor.string_pool.acquire();
        let mut operation = None;

        for command in CommandIter(&commands) {
            match Self::eval(
                editor,
                platform,
                clients,
                client_handle,
                command,
                source_path,
                &mut output,
            ) {
                Ok(None) => (),
                Ok(Some(op)) => {
                    operation = Some(op);
                    break;
                }
                Err(error) => {
                    output.clear();
                    let error =
                        error.display(command, source_path, &editor.commands, &editor.buffers);
                    editor
                        .status_bar
                        .write(MessageKind::Error)
                        .fmt(format_args!("{}", error));
                    break;
                }
            }

            editor.trigger_event_handlers(platform, clients);
        }

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
        command: &'command str,
        source_path: Option<&'command Path>,
        output: &mut String,
    ) -> Result<Option<CommandOperation>, CommandError> {
        let (source, mut args) = editor.commands.parse(command)?;
        match source {
            CommandSource::Builtin(i) => {
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
            CommandSource::Macro(i) => {
                args.assert_no_bang()?;
                args.get_flags(&mut [])?;

                let command = &editor.commands.macro_commands[i];
                let mut result = Ok(None);
                let mut commands = editor.string_pool.acquire_with(&command.commands);

                for param in &command.params {
                    let value = args.next()?;
                    replace_to_between_text_markers(&mut commands, param, value);
                }
                args.assert_empty()?;

                for command in CommandIter(&commands) {
                    match Self::eval(
                        editor,
                        platform,
                        clients,
                        client_handle,
                        command,
                        source_path,
                        output,
                    ) {
                        Ok(None) => (),
                        Ok(Some(op)) => {
                            result = Ok(Some(op));
                            break;
                        }
                        Err(error) => {
                            result = Err(CommandError::MacroCommandError {
                                index: i,
                                commands: command.into(),
                                location: command.as_ptr() as _,
                                error: Box::new(error),
                            });
                            break;
                        }
                    }
                }
                editor.string_pool.release(commands);
                result
            }
            CommandSource::Request(i) => {
                args.assert_no_bang()?;

                let handle = editor.commands.request_commands[i].client_handle;

                let mut buf = platform.buf_pool.acquire();
                let write = buf.write();
                ServerEvent::Request(command).serialize(write);
                let buf = buf.share();
                platform.enqueue_request(PlatformRequest::WriteToClient { handle, buf });

                Ok(None)
            }
        }
    }

    pub fn spawn_process(
        &mut self,
        platform: &mut Platform,
        client_handle: Option<ClientHandle>,
        mut command: Command,
        stdin: Option<&str>,
        output_name: Option<&str>,
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
        process.output_var_name.clear();
        process.on_output.clear();

        if let Some(output_name) = output_name {
            process.output_var_name.push_str(output_name);
        }

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
            buf_len: if on_output.is_some() { 1024 } else { 0 },
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

        let mut commands = editor.string_pool.acquire();
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
                    commands.clear();
                    commands.push_str(&process.on_output);
                    replace_to_between_text_markers(&mut commands, &process.output_var_name, slice);
                    let client_handle = process.client_handle;
                    Self::eval_commands_then_output(
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
        success: bool,
    ) {
        let process = &mut editor.commands.spawned_processes[index];
        process.alive = false;
        if !success || process.on_output.is_empty() || process.output.is_empty() {
            return;
        }

        let stdout = match std::str::from_utf8(&process.output) {
            Ok(stdout) => stdout,
            Err(error) => {
                editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error));
                return;
            }
        };

        let mut commands = editor.string_pool.acquire_with(&process.on_output);
        replace_to_between_text_markers(&mut commands, &process.output_var_name, stdout);
        let client_handle = process.client_handle;
        Self::eval_commands_then_output(editor, platform, clients, client_handle, &commands, None);
        editor.string_pool.release(commands);
    }

    fn parse<'a>(&self, text: &'a str) -> Result<(CommandSource, CommandArgs<'a>), CommandError> {
        let mut tokens = CommandTokenIter(text);

        let command_name = match tokens.next() {
            Some((CommandTokenKind::Text, s)) => s,
            Some((_, s)) => return Err(CommandError::InvalidCommandName(s.into())),
            None => return Err(CommandError::InvalidCommandName(text.trim_start().into())),
        };

        let (command_name, bang) = match command_name.strip_suffix('!') {
            Some(command) => (command, true),
            None => (command_name, false),
        };
        if command_name.is_empty() {
            return Err(CommandError::InvalidCommandName(command_name.into()));
        }

        let source = match self.find_command(command_name) {
            Some(source) => source,
            None => return Err(CommandError::CommandNotFound(command_name.into())),
        };

        let args = CommandArgs {
            bang,
            tokens,
            len: 0,
        };

        Ok((source, args))
    }
}

pub fn parse_process_command(command: &str, environment: &str) -> Result<Command, CommandError> {
    let mut command_tokens = CommandTokenIter(command);
    let command = match command_tokens.next() {
        Some((CommandTokenKind::Text, token))
        | Some((CommandTokenKind::Flag, token))
        | Some((CommandTokenKind::Equals, token)) => token,
        Some((CommandTokenKind::Unterminated, token)) => {
            return Err(CommandError::UnterminatedToken(token.into()))
        }
        None => return Err(CommandError::InvalidToken(command.into())),
    };

    let mut command = Command::new(command);
    while let Some((kind, token)) = command_tokens.next() {
        if let CommandTokenKind::Unterminated = kind {
            return Err(CommandError::InvalidToken(token.into()));
        } else {
            command.arg(token);
        }
    }

    let mut environment_tokens = CommandTokenIter(environment);
    loop {
        let key = match environment_tokens.next() {
            Some((CommandTokenKind::Text, token)) => token,
            Some((CommandTokenKind::Flag, token)) | Some((CommandTokenKind::Equals, token)) => {
                return Err(CommandError::InvalidToken(token.into()))
            }
            Some((CommandTokenKind::Unterminated, token)) => {
                return Err(CommandError::UnterminatedToken(token.into()))
            }
            None => break,
        };
        let equals_token = match environment_tokens.next() {
            Some((CommandTokenKind::Equals, token)) => token,
            Some((_, token)) => return Err(CommandError::InvalidToken(token.into())),
            None => return Err(CommandError::UnterminatedToken(key.into())),
        };
        let value = match environment_tokens.next() {
            Some((CommandTokenKind::Text, token)) => token,
            Some((CommandTokenKind::Flag, token)) | Some((CommandTokenKind::Equals, token)) => {
                return Err(CommandError::InvalidToken(token.into()))
            }
            Some((CommandTokenKind::Unterminated, token)) => {
                return Err(CommandError::UnterminatedToken(token.into()))
            }
            None => return Err(CommandError::UnterminatedToken(equals_token.into())),
        };

        command.env(key, value);
    }

    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_commands() -> CommandManager {
        let builtin_commands = &[BuiltinCommand {
            name: "command-name",
            alias: "c",
            help: "",
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
    fn test_replace_all() {
        fn assert_replace_all(text_expected: (&str, &str), from_to: (&str, &str)) {
            let mut text = text_expected.0.into();
            replace_to_between_text_markers(&mut text, from_to.0, from_to.1);
            assert_eq!(text_expected.1, text);
        }

        assert_eq!(b'\x02', START_OF_TEXT_BYTE);
        assert_eq!(b'\x03', END_OF_TEXT_BYTE);

        assert_replace_all(("xxxx", "xxxx"), ("FROM", "to"));
        assert_replace_all(("xxxx A", "xxxx \x02b\x03"), ("A", "b"));
        assert_replace_all(
            ("A xxxx AA", "\x02b\x03 xxxx \x02b\x03\x02b\x03"),
            ("A", "b"),
        );
    }

    #[test]
    fn command_tokens() {
        let mut tokens = CommandTokenIter("value -flag");
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Text, "value"))
        ));
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Flag, "-flag"))
        ));
        assert!(tokens.next().is_none());

        let mut tokens = CommandTokenIter("value --long-flag");
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Text, "value"))
        ));
        assert!(matches!(
            tokens.next(),
            Some((CommandTokenKind::Flag, "--long-flag"))
        ));
        assert!(tokens.next().is_none());
    }

    #[test]
    fn command_parsing() {
        fn assert_bang(commands: &CommandManager, command: &str, expect_bang: bool) {
            let (source, args) = match commands.parse(command) {
                Ok(result) => result,
                Err(_) => panic!("command parse error at '{}'", command),
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
                Ok((_, args)) => args,
                Err(_) => panic!("command '{}' parse error", command),
            }
        }

        fn collect<'a>(mut args: CommandArgs<'a>) -> Vec<&'a str> {
            let mut values = Vec::new();
            loop {
                match args.try_next() {
                    Ok(Some(arg)) => values.push(arg),
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

        let args = parse_args(&commands, "c -option=value aaa");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(None, flags[0].1);
        assert_eq!(Some("value"), flags[1].1);
        assert_eq!(["aaa"], &collect(args)[..]);

        let args = parse_args(&commands, "c 'aaa' -option=value");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(None, flags[0].1);
        assert_eq!(Some("value"), flags[1].1);
        assert_eq!(["aaa"], &collect(args)[..]);

        let args = parse_args(&commands, "c aaa -switch bbb -option=value ccc");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(Some(""), flags[0].1);
        assert_eq!(Some("value"), flags[1].1);
        assert_eq!(["aaa", "bbb", "ccc"], &collect(args)[..]);

        let args = parse_args(&commands, "c -switch -option=value aaa");
        let mut flags = [("switch", None), ("option", None)];
        if args.get_flags(&mut flags).is_err() {
            panic!("error parsing args");
        }
        assert_eq!(Some(""), flags[0].1);
        assert_eq!(Some("value"), flags[1].1);
        assert_eq!(["aaa"], &collect(args)[..]);
    }

    #[test]
    fn command_parsing_fail() {
        let commands = create_commands();

        macro_rules! assert_fail {
            ($command:expr, $error_pattern:pat => $value:ident == $expect:expr) => {
                let command = $command;
                let location = command.as_ptr() as _;
                match commands.parse(command) {
                    Ok(_) => panic!("command parsed successfully"),
                    Err($error_pattern) => assert_eq!($expect, $value.as_str_at(command, location)),
                    Err(_) => panic!("other error occurred"),
                }
            };
        }

        assert_fail!("", CommandError::InvalidCommandName(s) => s == "");
        assert_fail!("   ", CommandError::InvalidCommandName(s) => s == "");
        assert_fail!(" !", CommandError::InvalidCommandName(s) => s == "");
        assert_fail!("!  'aa'", CommandError::InvalidCommandName(s) => s == "");
        assert_fail!("  a \"bb\"", CommandError::CommandNotFound(s) => s == "a");

        fn assert_unterminated(args: &str) {
            let mut args = CommandArgs {
                bang: false,
                tokens: CommandTokenIter(args),
                len: 0,
            };
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
