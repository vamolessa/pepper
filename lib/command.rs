use std::{collections::VecDeque, fmt};

use crate::{
    buffer::{Buffer, BufferCollection, BufferError, BufferHandle},
    buffer_view::BufferViewHandle,
    client::{Client, ClientHandle, ClientManager},
    editor::Editor,
    events::KeyParseError,
    pattern::PatternError,
    platform::Platform,
};

mod builtin;

pub const HISTORY_CAPACITY: usize = 10;

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
    CustomCommandError {
        index: usize,
        body: String,
        location: usize,
        error: Box<CommandError>,
    },
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
    BufferError(BufferHandle, BufferError),
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
    PatternError(CommandToken, PatternError),
    KeyParseError(CommandToken, KeyParseError),
    InvalidRegisterKey(CommandToken),
    LspServerNotRunning,
}
impl CommandError {
    pub fn display<'command, 'error>(
        &'error self,
        command: &'command str,
        commands: &'error CommandManager,
        buffers: &'error BufferCollection,
    ) -> CommandErrorDisplay<'command, 'error> {
        CommandErrorDisplay {
            command,
            location: command.as_ptr() as usize,
            commands,
            buffers,
            error: self,
        }
    }
}

pub struct CommandErrorDisplay<'command, 'error> {
    command: &'command str,
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
            let error_len = error_token.len;
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
            CommandError::CustomCommandError {
                index,
                body,
                location,
                error,
            } => {
                let error_display = CommandErrorDisplay {
                    command: body,
                    location: *location,
                    commands: self.commands,
                    buffers: self.buffers,
                    error,
                };
                let command_name = &self.commands.custom_commands()[*index].name;
                write(
                    self,
                    f,
                    &c.trim().into(),
                    format_args!("at custom command '{}':\n{}", command_name, error_display),
                )
            }
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
                f.write_fmt(format_args!("invalid buffer handle {}", handle))
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
            CommandError::BufferError(handle, error) => match self.buffers.get(*handle) {
                Some(buffer) => f.write_fmt(format_args!("{}", error.display(buffer))),
                None => Ok(()),
            },
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

fn find_balanced_curly_bracket(bytes: &[u8]) -> Option<usize> {
    let mut balance: usize = 1;
    let mut i = 0;

    loop {
        if i == bytes.len() {
            return None;
        }

        match bytes[i] {
            b'{' => balance += 1,
            b'}' => {
                balance -= 1;
                if balance == 0 {
                    return Some(i);
                }
            }
            _ => (),
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
                    b'{' => match find_balanced_curly_bracket(&bytes[(i + 1)..]) {
                        Some(len) => {
                            i += len;
                        }
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

#[derive(Clone, Copy)]
pub enum CommandTokenKind {
    Text,
    Flag,
    Equals,
    Unterminated,
}
pub struct CommandTokenIter<'a> {
    pub rest: &'a str,
}
impl<'a> Iterator for CommandTokenIter<'a> {
    type Item = (CommandTokenKind, &'a str);
    fn next(&mut self) -> Option<Self::Item> {
        fn split_at_boundary(s: &str) -> (&str, &str) {
            match s.find(|c: char| c.is_ascii_whitespace() || matches!(c, '"' | '\'' | '{' | '=')) {
                Some(i) => s.split_at(i),
                None => (s, ""),
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
                        let token = &self.rest[..i];
                        self.rest = &self.rest[(i + 1)..];
                        Some((CommandTokenKind::Text, token))
                    }
                    None => {
                        let token = self.rest;
                        self.rest = "";
                        Some((CommandTokenKind::Unterminated, token))
                    }
                }
            }
            b'{' => {
                self.rest = &self.rest[1..];
                match find_balanced_curly_bracket(self.rest.as_bytes()) {
                    Some(i) => {
                        let token = &self.rest[..i];
                        self.rest = &self.rest[(i + 1)..];
                        Some((CommandTokenKind::Text, token))
                    }
                    None => {
                        let token = self.rest;
                        self.rest = "";
                        Some((CommandTokenKind::Unterminated, token))
                    }
                }
            }
            b'-' => {
                let (token, rest) = split_at_boundary(&self.rest[1..]);
                self.rest = rest;
                Some((CommandTokenKind::Flag, token))
            }
            b'=' => {
                let (token, rest) = self.rest.split_at(1);
                self.rest = rest;
                Some((CommandTokenKind::Equals, token))
            }
            _ => {
                let (token, rest) = split_at_boundary(self.rest);
                self.rest = rest;
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
        let mut tokens = CommandTokenIter {
            rest: self.tokens.rest,
        };
        loop {
            let previous_state = self.tokens.rest;
            let token = match tokens.next() {
                Some(token) => token,
                None => break Ok(()),
            };
            match token {
                (CommandTokenKind::Text, _) => (),
                (CommandTokenKind::Flag, key) => {
                    let value = match flags.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, value)) => value,
                        None => break Err(CommandError::UnknownFlag(key.into())),
                    };

                    match tokens.next() {
                        Some((CommandTokenKind::Text, _)) => (),
                        Some((CommandTokenKind::Flag, _)) => {
                            *value = Some("");
                            tokens.rest = previous_state;
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
                            *value = Some("");
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
                Some((CommandTokenKind::Flag, _)) => match self.tokens.next() {
                    Some((CommandTokenKind::Text, arg)) => break Ok(Some(arg)),
                    Some((CommandTokenKind::Flag, _)) => (),
                    Some((CommandTokenKind::Equals, _)) => {
                        self.tokens.next();
                    }
                    Some((CommandTokenKind::Unterminated, token)) => {
                        break Err(CommandError::UnterminatedToken(token.into()))
                    }
                    None => break Ok(None),
                },
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
                self.tokens.rest.into(),
                self.len,
            )),
        }
    }

    pub fn assert_empty(&mut self) -> Result<(), CommandError> {
        match self.tokens.next() {
            Some((_, token)) => Err(CommandError::TooManyArguments(token.into(), self.len)),
            None => Ok(()),
        }
    }
}

pub enum CommandSource {
    Builtin(usize),
    Custom(usize),
}

pub struct BuiltinCommand {
    pub name: &'static str,
    pub alias: &'static str,
    pub help: &'static str,
    pub completions: &'static [CompletionSource],
    pub func: CommandFn,
}

pub struct CustomCommand {
    pub name: String,
    pub alias: String,
    pub help: String,
    pub param_count: usize,
    pub body: String,
}

pub struct CommandManager {
    builtin_commands: &'static [BuiltinCommand],
    custom_commands: Vec<CustomCommand>,
    history: VecDeque<String>,
}

impl CommandManager {
    pub fn new() -> Self {
        Self {
            builtin_commands: builtin::COMMANDS,
            custom_commands: Vec::new(),
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
        }
    }

    pub fn find_command(&self, name: &str) -> Option<CommandSource> {
        if let Some(i) = self
            .custom_commands
            .iter()
            .position(|c| c.alias == name || c.name == name)
        {
            return Some(CommandSource::Custom(i));
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

    pub fn custom_commands(&self) -> &[CustomCommand] {
        &self.custom_commands
    }

    pub fn register_custom_command(&mut self, command: CustomCommand) {
        self.custom_commands.push(command);
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

    pub fn eval<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: &'command str,
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
                    args,
                    output,
                };
                command(&mut ctx)
            }
            CommandSource::Custom(i) => {
                args.assert_no_bang()?;
                args.get_flags(&mut [])?;
                args.assert_empty()?;

                let mut result = Ok(None);
                let mut body = editor.string_pool.acquire();
                body.clear();
                body.push_str(&editor.commands.custom_commands[i].body);
                for command in CommandIter(&body) {
                    match Self::eval(editor, platform, clients, client_handle, command, output) {
                        Ok(None) => (),
                        Ok(Some(op)) => {
                            result = Ok(Some(op));
                            break;
                        }
                        Err(error) => {
                            result = Err(CommandError::CustomCommandError {
                                index: i,
                                body: command.into(),
                                location: command.as_ptr() as _,
                                error: Box::new(error),
                            });
                            break;
                        }
                    }
                }
                editor.string_pool.release(body);
                result
            }
        }
    }

    fn parse<'a>(&self, text: &'a str) -> Result<(CommandSource, CommandArgs<'a>), CommandError> {
        let mut tokens = CommandTokenIter { rest: text };

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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_commands() -> CommandManager {
        let builtin_commands = &[BuiltinCommand {
            name: "command-name",
            alias: "c",
            help: "",
            completions: &[],
            func: |_| Ok(None),
        }];

        CommandManager {
            builtin_commands,
            custom_commands: Vec::new(),
            history: Default::default(),
        }
    }

    #[test]
    fn operation_size() {
        assert_eq!(1, std::mem::size_of::<CommandOperation>());
        assert_eq!(1, std::mem::size_of::<Option<CommandOperation>>());
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
                    Err(_) => panic!("error parsing args"),
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
                tokens: CommandTokenIter { rest: args },
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
    fn test_find_balanced_curly_bracket() {
        assert_eq!(None, find_balanced_curly_bracket(b""));
        assert_eq!(Some(0), find_balanced_curly_bracket(b"}"));
        assert_eq!(Some(2), find_balanced_curly_bracket(b"  }}"));
        assert_eq!(Some(2), find_balanced_curly_bracket(b"{}}"));
        assert_eq!(Some(4), find_balanced_curly_bracket(b"{{}}}"));
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
    }
}
