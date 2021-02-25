use std::{collections::VecDeque, fmt};

use crate::{
    buffer::BufferHandle,
    buffer_view::BufferViewHandle,
    client::{Client, ClientHandle, ClientManager},
    editor::Editor,
    editor_utils::MessageKind,
    platform::Platform,
};

//mod builtin;

pub const MAX_REQUIRED_VALUES_LEN: usize = 4;
pub const MAX_OTHER_VALUES_LEN: usize = 8;
pub const MAX_FLAGS_LEN: usize = 8;
pub const HISTORY_CAPACITY: usize = 10;

#[derive(Debug)]
pub enum CommandParseError<'command> {
    InvalidCommandName(&'command str),
    CommandNotFound(&'command str),
    CommandDoesNotAcceptBang(&'command str),
    UnterminatedArgument(&'command str),
    InvalidArgument(&'command str),
    TooFewValues(&'command str, u8),
    TooManyValues(&'command str, u8),
    UnknownFlag(&'command str),
    InvalidFlagValue(&'command str),
}

pub enum CommandError<'command> {
    Aborted,
    ParseError(CommandParseError<'command>),
}
impl<'command> CommandError<'command> {
    pub fn display<'error>(
        &'error self,
        command: &'command str,
    ) -> CommandErrorDisplay<'command, 'error> {
        CommandErrorDisplay {
            command,
            error: self,
        }
    }
}

pub struct CommandErrorDisplay<'command, 'error> {
    command: &'command str,
    error: &'error CommandError<'command>,
}
impl<'command, 'error> fmt::Display for CommandErrorDisplay<'command, 'error> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn error_offset(command: &str, token: &str) -> usize {
            token.as_ptr() as usize - command.as_ptr() as usize + 1
        }

        match self.error {
            CommandError::Aborted => Ok(()),
            CommandError::ParseError(ref error) => match error {
                CommandParseError::InvalidCommandName(s) => f.write_fmt(format_args!(
                    "{:>offset$} invalid command name",
                    '^',
                    offset = error_offset(self.command, s),
                )),
                CommandParseError::CommandNotFound(s) => f.write_fmt(format_args!(
                    "{:>offset$} command not found",
                    '^',
                    offset = error_offset(self.command, s),
                )),
                CommandParseError::CommandDoesNotAcceptBang(s) => f.write_fmt(format_args!(
                    "{:>offset$} command does not accept bang",
                    '^',
                    offset = error_offset(self.command, s),
                )),
                CommandParseError::UnterminatedArgument(s) => f.write_fmt(format_args!(
                    "{:>offset$} unterminated argument",
                    '^',
                    offset = error_offset(self.command, s),
                )),
                CommandParseError::InvalidArgument(s) => f.write_fmt(format_args!(
                    "{:>offset$} invalid argument",
                    '^',
                    offset = error_offset(self.command, s)
                )),
                CommandParseError::TooFewValues(s, min) => f.write_fmt(format_args!(
                    "{:>offset$} command expects at least {} values",
                    '^',
                    min,
                    offset = error_offset(self.command, s),
                )),
                CommandParseError::TooManyValues(s, max) => f.write_fmt(format_args!(
                    "{:>offset$} command expects at most {} values",
                    '^',
                    max,
                    offset = error_offset(self.command, s),
                )),
                CommandParseError::UnknownFlag(s) => f.write_fmt(format_args!(
                    "{:>offset$} unknown flag",
                    '^',
                    offset = error_offset(self.command, s),
                )),
                CommandParseError::InvalidFlagValue(s) => f.write_fmt(format_args!(
                    "{:>offset$} invalid flag value",
                    '^',
                    offset = error_offset(self.command, s),
                )),
            },
        }
    }
}

type CommandFn =
    for<'state, 'command> fn(
        CommandContext<'state, 'command>,
    ) -> Result<Option<CommandOperation>, CommandError<'command>>;

pub enum CommandOperation {
    Quit,
    QuitAll,
}

pub enum CompletionSource {
    Files,
    Buffers,
    Commands,
    Custom(&'static [&'static str]),
}

pub struct CommandContext<'state, 'command> {
    pub editor: &'state mut Editor,
    pub platform: &'state mut Platform,
    pub clients: &'state mut ClientManager,
    pub client_handle: Option<ClientHandle>,
    pub args: &'state mut CommandArgs<'command>,
}
impl<'state, 'command> CommandContext<'state, 'command> {
    pub fn current_buffer_view_handle_or_error(&mut self) -> Option<BufferViewHandle> {
        match self
            .client_handle
            .and_then(|h| self.clients.get(h))
            .and_then(Client::buffer_view_handle)
        {
            Some(handle) => Some(handle),
            None => {
                self.editor
                    .status_bar
                    .write(MessageKind::Error)
                    .str("no buffer view opened");
                None
            }
        }
    }

    pub fn current_buffer_handle_or_error(&mut self) -> Option<BufferHandle> {
        let buffer_view_handle = self.current_buffer_view_handle_or_error()?;
        match self
            .editor
            .buffer_views
            .get(buffer_view_handle)
            .map(|v| v.buffer_handle)
        {
            Some(handle) => Some(handle),
            None => {
                self.editor
                    .status_bar
                    .write(MessageKind::Error)
                    .str("no buffer opened");
                None
            }
        }
    }

    pub fn validate_buffer_handle(&mut self, handle: BufferHandle) -> Option<BufferHandle> {
        match self.editor.buffers.get(handle) {
            Some(_) => Some(handle),
            None => {
                self.editor
                    .status_bar
                    .write(MessageKind::Error)
                    .str("invalid buffer handle");
                None
            }
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
                    b'\\' => i += 1,
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

enum CommandSource {
    Builtin(usize),
}

pub struct BuiltinCommand {
    names: &'static [&'static str],
    help: &'static str,
    accepts_bang: bool,
    required_values: &'static [(&'static str, Option<CompletionSource>)],
    optional_values: &'static [(&'static str, Option<CompletionSource>)],
    extra_values: Option<Option<CompletionSource>>,
    flags: &'static [(&'static str, Option<CompletionSource>)],
    func: CommandFn,
}

pub struct CommandManager {
    builtin_commands: &'static [BuiltinCommand],
    history: VecDeque<String>,
    output_stack: String,
}

impl CommandManager {
    pub fn new() -> Self {
        Self {
            // TODO: use builtin::COMMANDS
            builtin_commands: &[], //builtin::COMMANDS,
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
            output_stack: String::new(),
        }
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

    pub fn push_output_str(&mut self, output: &str) {
        self.output_stack.push_str(output);
    }

    pub fn push_output_fmt(&mut self, args: fmt::Arguments) {
        let _ = fmt::write(&mut self.output_stack, args);
    }

    pub fn eval_command<'command>(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: &'command str,
        output: &mut String,
    ) -> Result<Option<CommandOperation>, CommandError<'command>> {
        match editor.commands.parse(command) {
            Ok((source, mut args)) => {
                let command = match source {
                    CommandSource::Builtin(i) => editor.commands.builtin_commands[i].func,
                };
                command(CommandContext {
                    editor,
                    platform,
                    clients,
                    client_handle,
                    args: &mut args,
                })
            }
            Err(error) => Err(CommandError::ParseError(error)),
        }
    }

    fn parse<'a>(
        &self,
        text: &'a str,
    ) -> Result<(CommandSource, CommandArgs<'a>), CommandParseError<'a>> {
        enum TokenKind {
            Text,
            Flag,
            Equals,
            Bang,
            Unterminated,
        }
        struct TokenIterator<'a> {
            rest: &'a str,
        }
        impl<'a> Iterator for TokenIterator<'a> {
            type Item = (TokenKind, &'a str);
            fn next(&mut self) -> Option<Self::Item> {
                fn next_token(mut rest: &str) -> Option<(TokenKind, &str, &str)> {
                    fn is_separator(c: char) -> bool {
                        c.is_ascii_whitespace() || c == '=' || c == '!' || c == '"' || c == '\''
                    }

                    rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\\');
                    if rest.is_empty() {
                        return None;
                    }

                    match rest.as_bytes()[0] {
                        b'-' => {
                            rest = &rest[1..];
                            let (token, rest) = match rest.find(is_separator) {
                                Some(i) => rest.split_at(i),
                                None => (rest, ""),
                            };
                            Some((TokenKind::Flag, token, rest))
                        }
                        delim @ b'"' | delim @ b'\'' => {
                            rest = &rest[1..];
                            match rest.find(delim as char) {
                                Some(i) => Some((TokenKind::Text, &rest[..i], &rest[(i + 1)..])),
                                None => Some((TokenKind::Unterminated, rest, "")),
                            }
                        }
                        b'=' => {
                            let (token, rest) = rest.split_at(1);
                            Some((TokenKind::Equals, token, rest))
                        }
                        b'!' => {
                            let (token, rest) = rest.split_at(1);
                            Some((TokenKind::Bang, token, rest))
                        }
                        _ => match rest.find(is_separator) {
                            Some(i) => {
                                let (token, rest) = rest.split_at(i);
                                Some((TokenKind::Text, token, rest))
                            }
                            None => Some((TokenKind::Text, rest, "")),
                        },
                    }
                }

                match next_token(self.rest) {
                    Some((kind, token, rest)) => {
                        self.rest = rest;
                        Some((kind, token))
                    }
                    None => None,
                }
            }
        }

        struct CommandParamsInfo<'a> {
            min_values_len: u8,
            max_values_len: Option<u8>,
            flags: &'a [(&'a str, Option<CompletionSource>)],
        }

        fn add_value<'a>(
            params: &CommandParamsInfo,
            args: &mut CommandArgs<'a>,
            values_count: &mut u8,
            value: &'a str,
        ) -> Result<(), CommandParseError<'a>> {
            if *values_count < params.min_values_len {
                args.required_values[*values_count as usize] = value;
            } else {
                let len = *values_count - params.min_values_len;
                let max = params
                    .max_values_len
                    .unwrap_or(args.other_values.len() as u8);
                if len < max {
                    args.other_values[len as usize] = Some(value);
                } else {
                    let max = max + params.min_values_len;
                    return Err(CommandParseError::TooManyValues(value, max));
                }
            }
            *values_count += 1;
            Ok(())
        }

        fn add_flag<'a>(
            params: &CommandParamsInfo,
            args: &mut CommandArgs<'a>,
            key: &'a str,
            value: &'a str,
        ) -> Result<(), CommandParseError<'a>> {
            match params.flags.iter().position(|f| f.0 == key) {
                Some(i) => {
                    args.flags[i] = Some(value);
                    Ok(())
                }
                None => Err(CommandParseError::UnknownFlag(key)),
            }
        }

        let mut values_count = 0;
        let mut args = CommandArgs::default();
        let mut tokens = TokenIterator { rest: text };
        let mut peeked_token = None;

        let command_name = match tokens.next() {
            Some((TokenKind::Text, s)) => s,
            Some((_, s)) => return Err(CommandParseError::InvalidCommandName(s)),
            None => return Err(CommandParseError::InvalidCommandName(text.trim_start())),
        };

        args.bang = match tokens.next() {
            Some((TokenKind::Bang, _)) => true,
            token => {
                peeked_token = token;
                false
            }
        };

        let (source, params) = match self
            .builtin_commands
            .iter()
            .position(|c| c.names.contains(&command_name))
        {
            Some(i) => {
                let command = &self.builtin_commands[i];
                if args.bang && !command.accepts_bang {
                    return Err(CommandParseError::CommandDoesNotAcceptBang(command_name));
                }
                let params = CommandParamsInfo {
                    min_values_len: command.required_values.len() as _,
                    max_values_len: match command.extra_values {
                        Some(_) => None,
                        None => Some(
                            (command.required_values.len() + command.optional_values.len()) as _,
                        ),
                    },
                    flags: &command.flags,
                };
                (CommandSource::Builtin(i), params)
            }
            None => return Err(CommandParseError::CommandNotFound(command_name)),
        };

        loop {
            let token = match peeked_token.take() {
                Some(token) => token,
                None => match tokens.next() {
                    Some(token) => token,
                    None => break,
                },
            };

            match token {
                (TokenKind::Text, s) => add_value(&params, &mut args, &mut values_count, s)?,
                (TokenKind::Flag, flag_token) => match tokens.next() {
                    Some((TokenKind::Equals, equals_token)) => match tokens.next() {
                        Some((TokenKind::Text, s)) => add_flag(&params, &mut args, flag_token, s)?,
                        Some((TokenKind::Unterminated, s)) => {
                            return Err(CommandParseError::UnterminatedArgument(s))
                        }
                        Some((_, s)) => return Err(CommandParseError::InvalidFlagValue(s)),
                        None => return Err(CommandParseError::InvalidFlagValue(equals_token)),
                    },
                    token => {
                        add_flag(&params, &mut args, flag_token, "")?;
                        peeked_token = token;
                    }
                },
                (TokenKind::Equals, s) | (TokenKind::Bang, s) => {
                    return Err(CommandParseError::InvalidArgument(s))
                }
                (TokenKind::Unterminated, s) => {
                    return Err(CommandParseError::UnterminatedArgument(s))
                }
            }
        }

        if values_count < params.min_values_len {
            let token = if values_count > 0 {
                args.required_values[values_count as usize - 1]
            } else {
                command_name
            };
            let min = params.min_values_len;
            return Err(CommandParseError::TooFewValues(token, min));
        }

        Ok((source, args))
    }
}

#[derive(Default)]
pub struct CommandArgs<'a> {
    pub bang: bool,
    pub required_values: [&'a str; MAX_REQUIRED_VALUES_LEN],
    pub other_values: [Option<&'a str>; MAX_OTHER_VALUES_LEN],
    pub flags: [Option<&'a str>; MAX_FLAGS_LEN],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_commands() -> CommandManager {
        let builtin_commands = &[BuiltinCommand {
            names: &["command-name", "c"],
            help: "",
            accepts_bang: true,
            required_values: &[],
            optional_values: &[],
            extra_values: Some(None),
            flags: &[("switch", None), ("option", None)],
            func: |_| Ok(None),
        }];

        CommandManager {
            builtin_commands,
            history: Default::default(),
            output_stack: Default::default(),
        }
    }

    #[test]
    fn command_parsing() {
        let commands = create_commands();

        macro_rules! assert_command {
            ($text:expr => bang = $bang:expr) => {
                let (source, args) = match commands.parse($text) {
                    Ok(result) => result,
                    Err(e) => panic!("command parse error {:?}", e),
                };
                assert!(matches!(source, CommandSource::Builtin(0)));
                assert_eq!($bang, args.bang);
            };
        }

        assert_command!("command-name" => bang = false);
        assert_command!("  command-name  " => bang = false);
        assert_command!("  command-name!  " => bang = true);
        assert_command!("  command-name!" => bang = true);
    }

    #[test]
    fn arg_parsing() {
        fn parse_args<'a>(commands: &CommandManager, command: &'a str) -> CommandArgs<'a> {
            match commands.parse(command) {
                Ok((_, args)) => args,
                Err(_) => panic!("command '{}' parse error", command),
            }
        }

        fn other_values_vec<'a>(args: &CommandArgs<'a>) -> Vec<&'a str> {
            args.other_values.iter().flatten().cloned().collect()
        }

        let commands = create_commands();
        let args = parse_args(&commands, "c  aaa  bbb  ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &other_values_vec(&args)[..]);
        let args = parse_args(&commands, "c  'aaa'  \"bbb\"  ccc  ");
        assert_eq!(["aaa", "bbb", "ccc"], &other_values_vec(&args)[..]);
        let args = parse_args(&commands, "c  'aaa'\"bbb\"\"ccc\"ddd  ");
        assert_eq!(["aaa", "bbb", "ccc", "ddd"], &other_values_vec(&args)[..]);

        let args = parse_args(
            &commands,
            "c \\\n-switch'value'\\\n-option=\"option value!\"\\\n",
        );
        assert_eq!(["value"], &other_values_vec(&args)[..]);
        assert_eq!(Some(""), args.flags[0]);
        assert_eq!(Some("option value!"), args.flags[1]);
    }

    #[test]
    fn command_parsing_fail() {
        let commands = create_commands();

        macro_rules! assert_fail {
            ($command:expr, $error_pattern:pat => $value:ident == $expect:expr) => {
                match commands.parse($command) {
                    Ok(_) => panic!("command parsed successfully"),
                    Err($error_pattern) => assert_eq!($expect, $value),
                    Err(e) => panic!("other error occurred {:?}", e),
                }
            };
        }

        assert_fail!("", CommandParseError::InvalidCommandName(s) => s == "");
        assert_fail!("   ", CommandParseError::InvalidCommandName(s) => s == "");
        assert_fail!(" !", CommandParseError::InvalidCommandName(s) => s == "!");
        assert_fail!("!  'aa'", CommandParseError::InvalidCommandName(s) => s == "!");
        assert_fail!("c -option=", CommandParseError::InvalidFlagValue(s) => s == "=");
        assert_fail!("  a \"aa\"", CommandParseError::CommandNotFound(s) => s == "a");

        assert_fail!("c! 'abc", CommandParseError::UnterminatedArgument(s) => s == "abc");
        assert_fail!("c! '", CommandParseError::UnterminatedArgument(s) => s == "");
        assert_fail!("c! \"'", CommandParseError::UnterminatedArgument(s) => s == "'");

        const TOO_MANY_VALUES_LEN: u8 = MAX_OTHER_VALUES_LEN as _;
        let mut too_many_values_command = String::new();
        too_many_values_command.push('c');
        for _ in 0..TOO_MANY_VALUES_LEN {
            too_many_values_command.push_str(" a");
        }
        too_many_values_command.push_str(" b");
        assert_fail!(&too_many_values_command, CommandParseError::TooManyValues(s, TOO_MANY_VALUES_LEN) => s == "b");
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

        let mut commands = CommandIter::new("command0\\\n still command0\ncommand1");
        assert_eq!(Some("command0\\\n still command0"), commands.next());
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
