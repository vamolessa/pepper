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

pub enum CommandError<'command> {
    Aborted,
    InvalidCommandName(&'command str),
    CommandNotFound(&'command str),
    CommandDoesNotAcceptBang,
    UnterminatedToken(&'command str),
    InvalidToken(&'command str),
    TooFewArguments(&'command str, u8),
    TooManyArguments(&'command str, u8),
    UnknownFlag(&'command str),
    UnsavedChanges,
    NoBufferOpened,
    InvalidBufferHandle(BufferHandle),
    InvalidPath(&'command str),
    ParseArgError {
        arg: &'command str,
        type_name: &'static str,
    },
    BufferError(BufferHandle, BufferError),
    ConfigNotFound(&'command str),
    InvalidConfigValue {
        key: &'command str,
        value: &'command str,
    },
    ColorNotFound(&'command str),
    InvalidColorValue {
        key: &'command str,
        value: &'command str,
    },
    InvalidGlob(&'command str),
    PatternError(&'command str, PatternError),
    KeyParseError(&'command str, KeyParseError),
    InvalidRegisterKey(&'command str),
    LspServerNotRunning,
}
impl<'command> CommandError<'command> {
    pub fn display<'error>(
        &'error self,
        command: &'command str,
        buffers: &'error BufferCollection,
    ) -> CommandErrorDisplay<'command, 'error> {
        CommandErrorDisplay {
            command,
            buffers,
            error: self,
        }
    }
}

pub struct CommandErrorDisplay<'command, 'error> {
    command: &'command str,
    buffers: &'error BufferCollection,
    error: &'error CommandError<'command>,
}
impl<'command, 'error> fmt::Display for CommandErrorDisplay<'command, 'error> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn write(
            this: &CommandErrorDisplay,
            f: &mut fmt::Formatter,
            error_token: &str,
            message: fmt::Arguments,
        ) -> fmt::Result {
            let error_offset = error_token.as_ptr() as usize - this.command.as_ptr() as usize;
            let error_len = error_token.len();
            write!(
                f,
                "{}\n{: >offset$}{:^<len$}\n",
                this.command,
                "",
                "",
                offset = error_offset,
                len = error_len
            )?;
            f.write_fmt(message)?;
            Ok(())
        }

        match self.error {
            CommandError::Aborted => Ok(()),
            CommandError::InvalidCommandName(token) => write(
                self,
                f,
                token,
                format_args!("invalid command name '{}'", token),
            ),
            CommandError::CommandNotFound(command) => write(
                self,
                f,
                command,
                format_args!("no such command '{}'", command),
            ),
            CommandError::CommandDoesNotAcceptBang => write(
                self,
                f,
                self.command.trim(),
                format_args!("command does not accept bang"),
            ),
            CommandError::UnterminatedToken(token) => {
                write(self, f, token, format_args!("unterminated token"))
            }
            CommandError::InvalidToken(token) => {
                write(self, f, token, format_args!("invalid token '{}'", token))
            }
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
            CommandError::UnknownFlag(token) => {
                write(self, f, token, format_args!("unknown flag '{}'", token))
            }
            CommandError::UnsavedChanges => f.write_str(
                "there are unsaved changes. try appending a '!' to command name to force execute",
            ),
            CommandError::NoBufferOpened => f.write_str("no buffer opened"),
            CommandError::InvalidBufferHandle(handle) => {
                f.write_fmt(format_args!("invalid buffer handle {}", handle))
            }
            CommandError::InvalidPath(path) => {
                write(self, f, path, format_args!("invalid path '{}'", path))
            }
            CommandError::ParseArgError { arg, type_name } => write(
                self,
                f,
                arg,
                format_args!("could not parse '{}' as {}", arg, type_name),
            ),
            CommandError::BufferError(handle, error) => match self.buffers.get(*handle) {
                Some(buffer) => f.write_fmt(format_args!("{}", error.display(buffer))),
                None => Ok(()),
            },
            CommandError::ConfigNotFound(key) => {
                write(self, f, key, format_args!("no such config '{}'", key))
            }
            CommandError::InvalidConfigValue { key, value } => write(
                self,
                f,
                value,
                format_args!("invalid value '{}' for config '{}'", value, key),
            ),
            CommandError::ColorNotFound(key) => {
                write(self, f, key, format_args!("no such theme color '{}'", key))
            }
            CommandError::InvalidColorValue { key, value } => write(
                self,
                f,
                value,
                format_args!("invalid value '{}' for theme color '{}'", value, key),
            ),
            CommandError::InvalidGlob(glob) => {
                write(self, f, glob, format_args!("invalid glob '{}'", glob))
            }
            CommandError::PatternError(pattern, error) => {
                write(self, f, pattern, format_args!("{}", error))
            }
            CommandError::KeyParseError(keys, error) => {
                write(self, f, keys, format_args!("{}", error))
            }
            CommandError::InvalidRegisterKey(key) => {
                write(self, f, key, format_args!("invalid register key '{}'", key))
            }
            CommandError::LspServerNotRunning => f.write_str("lsp server not running"),
        }
    }
}

type CommandFn =
    for<'state, 'command> fn(
        &mut CommandContext<'state, 'command>,
    ) -> Result<Option<CommandOperation>, CommandError<'command>>;

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
    pub fn current_buffer_view_handle(&self) -> Result<BufferViewHandle, CommandError<'command>> {
        match self
            .client_handle
            .and_then(|h| self.clients.get(h))
            .and_then(Client::buffer_view_handle)
        {
            Some(handle) => Ok(handle),
            None => Err(CommandError::NoBufferOpened),
        }
    }

    pub fn current_buffer_handle(&self) -> Result<BufferHandle, CommandError<'command>> {
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

    pub fn assert_can_discard_all_buffers(&self) -> Result<(), CommandError<'command>> {
        if self.args.bang || !self.editor.buffers.iter().any(Buffer::needs_save) {
            Ok(())
        } else {
            Err(CommandError::UnsavedChanges)
        }
    }

    pub fn assert_can_discard_buffer(
        &self,
        handle: BufferHandle,
    ) -> Result<(), CommandError<'command>> {
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

pub struct CommandIter<'a>(&'a str);
impl<'a> CommandIter<'a> {
    pub fn new(commands: &'a str) -> Self {
        CommandIter(commands)
    }
}
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
            // TODO: make it balanced
            b'{' => {
                self.rest = &self.rest[1..];
                match self.rest.find('}') {
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
    fn new(bang: bool, args: &'a str) -> Self {
        Self {
            bang,
            tokens: CommandTokenIter { rest: args },
            len: 0,
        }
    }

    pub fn assert_no_bang(&self) -> Result<(), CommandError<'a>> {
        if self.bang {
            Err(CommandError::CommandDoesNotAcceptBang)
        } else {
            Ok(())
        }
    }

    pub fn get_flags(
        &self,
        flags: &mut [(&'static str, Option<&'a str>)],
    ) -> Result<(), CommandError<'a>> {
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
                        None => break Err(CommandError::UnknownFlag(key)),
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
                        None => {
                            *value = Some("");
                            break Ok(());
                        }
                    }
                }
                (CommandTokenKind::Equals, token) => return Err(CommandError::InvalidToken(token)),
                (CommandTokenKind::Unterminated, token) => {
                    return Err(CommandError::UnterminatedToken(token))
                }
            }
        }
    }

    pub fn try_next(&mut self) -> Result<Option<&'a str>, CommandError<'a>> {
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
                        break Err(CommandError::UnterminatedToken(token))
                    }
                    None => break Ok(None),
                },
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

    pub fn next(&mut self) -> Result<&'a str, CommandError<'a>> {
        match self.try_next()? {
            Some(arg) => Ok(arg),
            None => Err(CommandError::TooFewArguments(self.tokens.rest, self.len)),
        }
    }

    pub fn assert_empty(&mut self) -> Result<(), CommandError<'a>> {
        match self.tokens.next() {
            Some((_, token)) => Err(CommandError::TooManyArguments(token, self.len)),
            None => Ok(()),
        }
    }
}

pub enum CommandSource {
    Builtin(usize),
}

pub struct BuiltinCommand {
    pub names: &'static [&'static str],
    pub help: &'static str,
    pub completions: &'static [CompletionSource],
    pub func: CommandFn,
}

pub struct CommandManager {
    builtin_commands: &'static [BuiltinCommand],
    history: VecDeque<String>,
}

impl CommandManager {
    pub fn new() -> Self {
        Self {
            builtin_commands: builtin::COMMANDS,
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
        }
    }

    pub fn find_command(&self, name: &str) -> Option<CommandSource> {
        match self
            .builtin_commands
            .iter()
            .position(|c| c.names.contains(&name))
        {
            Some(i) => Some(CommandSource::Builtin(i)),
            None => None,
        }
    }

    pub fn builtin_commands(&self) -> &[BuiltinCommand] {
        &self.builtin_commands
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn history_entry(&self, index: usize) -> &str {
        match self.history.get(index) {
            Some(e) => e.as_str(),
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

    pub fn eval_command<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: &'command str,
        output: &mut String,
    ) -> Result<Option<CommandOperation>, CommandError<'command>> {
        let (source, bang, args) = editor.commands.parse(command)?;
        let command = match source {
            CommandSource::Builtin(i) => editor.commands.builtin_commands[i].func,
        };
        let mut ctx = CommandContext {
            editor,
            platform,
            clients,
            client_handle,
            args: CommandArgs::new(bang, args),
            output,
        };
        command(&mut ctx)
    }

    fn parse<'a>(&self, text: &'a str) -> Result<(CommandSource, bool, &'a str), CommandError<'a>> {
        let mut tokens = CommandTokenIter { rest: text };

        let command_name = match tokens.next() {
            Some((CommandTokenKind::Text, s)) => s,
            Some((_, s)) => return Err(CommandError::InvalidCommandName(s)),
            None => return Err(CommandError::InvalidCommandName(text.trim_start())),
        };

        let (command_name, bang) = match command_name.strip_suffix('!') {
            Some(command) => (command, true),
            None => (command_name, false),
        };
        if command_name.is_empty() {
            return Err(CommandError::InvalidCommandName(command_name));
        }

        let source = match self.find_command(command_name) {
            Some(source) => source,
            None => return Err(CommandError::CommandNotFound(command_name)),
        };

        Ok((source, bang, tokens.rest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_commands() -> CommandManager {
        let builtin_commands = &[BuiltinCommand {
            names: &["command-name", "c"],
            help: "",
            completions: &[],
            func: |_| Ok(None),
        }];

        CommandManager {
            builtin_commands,
            history: Default::default(),
        }
    }

    #[test]
    fn command_parsing() {
        fn assert_bang(commands: &CommandManager, command: &str, expect_bang: bool) {
            let (source, bang, _) = match commands.parse(command) {
                Ok(result) => result,
                Err(_) => panic!("command parse error at '{}'", command),
            };
            assert!(matches!(source, CommandSource::Builtin(0)));
            assert_eq!(expect_bang, bang);
        }

        let commands = create_commands();
        assert_bang(&commands, "command-name", false);
        assert_bang(&commands, "  command-name  ", false);
        assert_bang(&commands, "  command-name!  ", true);
        assert_bang(&commands, "  command-name!", true);
    }

    #[test]
    fn arg_parsing() {
        fn parse_args<'a>(commands: &CommandManager, command: &'a str) -> &'a str {
            match commands.parse(command) {
                Ok((_, _, args)) => args,
                Err(_) => panic!("command '{}' parse error", command),
            }
        }

        fn collect<'a>(args: &'a str) -> Vec<&'a str> {
            let mut values = Vec::new();
            let mut args = CommandArgs::new(false, args);
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
        assert_eq!(["aaa", "bbb", "ccc"], &collect(&args)[..]);
        let args = parse_args(&commands, "c  'aaa'  \"bbb\"  ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(&args)[..]);
        let args = parse_args(&commands, "c  \"aaa\"\"bbb\"ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(&args)[..]);
        let args = parse_args(&commands, "c  {aaa}{bbb}ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &collect(&args)[..]);
    }

    #[test]
    fn command_parsing_fail() {
        let commands = create_commands();

        macro_rules! assert_fail {
            ($command:expr, $error_pattern:pat => $value:ident == $expect:expr) => {
                match commands.parse($command) {
                    Ok(_) => panic!("command parsed successfully"),
                    Err($error_pattern) => assert_eq!($expect, $value),
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
            let mut args = CommandArgs::new(false, args);
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
    fn multi_command_line_parsing() {
        let mut commands = CommandIter::new("command0\ncommand1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter::new("command0\n\n\ncommand1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter::new("   #command0");
        assert_eq!(None, commands.next());

        let mut commands = CommandIter::new("command0 # command1");
        assert_eq!(Some("command0 "), commands.next());
        assert_eq!(None, commands.next());

        let mut commands = CommandIter::new("    # command0\ncommand1");
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());

        let mut commands =
            CommandIter::new("command0# comment\n\n# more comment\n\n# one more comment\ncommand1");
        assert_eq!(Some("command0"), commands.next());
        assert_eq!(Some("command1"), commands.next());
        assert_eq!(None, commands.next());
    }
}
