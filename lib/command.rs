use std::collections::VecDeque;

use crate::{
    buffer::BufferHandle,
    buffer_view::BufferViewHandle,
    client::{Client, ClientHandle, ClientManager},
    editor::{Editor, EditorOutput, EditorOutputKind},
    platform::Platform,
};

//mod builtin;

pub const MAX_COMMAND_ARGUMENT_VALUE_COUNT: usize = 16;
pub const MAX_COMMAND_ARGUMENT_FLAG_COUNT: usize = 16;
pub const HISTORY_CAPACITY: usize = 10;

pub enum CommandParseError<'a> {
    InvalidCommandName(&'a str),
    CommandNotFound(&'a str),
    CommandDoesNotAcceptBang(&'a str),
    InvalidArgument(&'a str),
    InvalidFlagValue(&'a str),
    UnterminatedArgument(&'a str),
    TooManyValues(&'a str),
    TooManyFlags(&'a str),
}

type CommandFn = fn(CommandContext) -> Option<CommandOperation>;

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

pub struct CommandContext<'a> {
    pub editor: &'a mut Editor,
    pub platform: &'a mut Platform,
    pub clients: &'a mut ClientManager,
    pub client_handle: Option<ClientHandle>,
    pub args: &'a CommandArgs<'a>,
}
impl<'a> CommandContext<'a> {
    pub fn current_buffer_view_handle_or_error(&mut self) -> Option<BufferViewHandle> {
        match self
            .client_handle
            .and_then(|h| self.clients.get(h))
            .and_then(Client::buffer_view_handle)
        {
            Some(handle) => Some(handle),
            None => {
                self.editor
                    .output
                    .write(EditorOutputKind::Error)
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
                    .output
                    .write(EditorOutputKind::Error)
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
                    .output
                    .write(EditorOutputKind::Error)
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
}

impl CommandManager {
    pub fn new() -> Self {
        Self {
            builtin_commands: &[], //builtin::COMMANDS,
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
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

    pub fn eval_from_read_line(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
    ) -> Option<CommandOperation> {
        // TODO: try to remove this memmory allocation
        let command = editor.read_line.input().to_string();
        match editor.commands.parse(&command) {
            Ok((command, args)) => command(CommandContext {
                editor,
                platform,
                clients,
                client_handle,
                args: &args,
            }),
            Err(error) => {
                Self::format_parse_error(&mut editor.output, error, &command);
                None
            }
        }
    }

    pub fn eval(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: &str,
    ) -> Option<CommandOperation> {
        match editor.commands.parse(command) {
            Ok((command, args)) => command(CommandContext {
                editor,
                platform,
                clients,
                client_handle,
                args: &args,
            }),
            Err(error) => {
                Self::format_parse_error(&mut editor.output, error, command);
                None
            }
        }
    }

    fn format_parse_error(output: &mut EditorOutput, error: CommandParseError, command: &str) {
        let mut write = output.write(EditorOutputKind::Error);
        write.str(command);
        write.str("\n");

        fn error_offset(command: &str, token: &str) -> usize {
            token.as_ptr() as usize - command.as_ptr() as usize + 1
        }

        match error {
            CommandParseError::InvalidCommandName(s) => write.fmt(format_args!(
                "{:>offset$} invalid command name",
                '^',
                offset = error_offset(command, s),
            )),
            CommandParseError::CommandNotFound(s) => write.fmt(format_args!(
                "{:>offset$} command not found",
                '^',
                offset = error_offset(command, s),
            )),
            CommandParseError::CommandDoesNotAcceptBang(s) => write.fmt(format_args!(
                "{:>offset$} command does not accept bang",
                '^',
                offset = error_offset(command, s),
            )),
            CommandParseError::InvalidArgument(s) => write.fmt(format_args!(
                "{:>offset$} invalid argument",
                '^',
                offset = error_offset(command, s)
            )),
            CommandParseError::InvalidFlagValue(s) => write.fmt(format_args!(
                "{:>offset$} invalid flag value",
                '^',
                offset = error_offset(command, s),
            )),
            CommandParseError::UnterminatedArgument(s) => write.fmt(format_args!(
                "{:>offset$} unterminated argument",
                '^',
                offset = error_offset(command, s),
            )),
            CommandParseError::TooManyValues(s) => write.fmt(format_args!(
                "{:>offset$} more than {} values passed to command",
                '^',
                MAX_COMMAND_ARGUMENT_VALUE_COUNT,
                offset = error_offset(command, s),
            )),
            CommandParseError::TooManyFlags(s) => write.fmt(format_args!(
                "{:>offset$} more than {} flags passed to command",
                '^',
                MAX_COMMAND_ARGUMENT_FLAG_COUNT,
                offset = error_offset(command, s),
            )),
        }
    }

    fn parse<'a>(
        &mut self,
        text: &'a str,
    ) -> Result<(CommandFn, CommandArgs<'a>), CommandParseError<'a>> {
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

        fn add_value<'a>(
            args: &mut CommandArgs<'a>,
            value: &'a str,
        ) -> Result<(), CommandParseError<'a>> {
            if args.values_len < args.values.len() {
                args.values[args.values_len] = value;
                args.values_len += 1;
                Ok(())
            } else {
                Err(CommandParseError::TooManyValues(value))
            }
        }

        fn add_flag<'a>(
            args: &mut CommandArgs<'a>,
            key: &'a str,
            value: &'a str,
        ) -> Result<(), CommandParseError<'a>> {
            if args.flags_len < args.flags.len() {
                args.flags[args.flags_len] = (key, value);
                args.flags_len += 1;
                Ok(())
            } else {
                Err(CommandParseError::TooManyFlags(key))
            }
        }

        let mut args = CommandArgs::default();
        let mut tokens = TokenIterator { rest: text };
        let mut peeked_token = None;

        let command = match tokens.next() {
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

        let command = match self
            .builtin_commands
            .iter()
            .find(|&c| c.names.contains(&command))
        {
            Some(definition) => {
                if !args.bang || definition.accepts_bang {
                    definition.func
                } else {
                    return Err(CommandParseError::CommandDoesNotAcceptBang(command));
                }
            }
            None => return Err(CommandParseError::CommandNotFound(command)),
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
                (TokenKind::Text, s) => add_value(&mut args, s)?,
                (TokenKind::Flag, flag_token) => match tokens.next() {
                    Some((TokenKind::Equals, equals_token)) => match tokens.next() {
                        Some((TokenKind::Text, s)) => add_flag(&mut args, flag_token, s)?,
                        Some((TokenKind::Unterminated, s)) => {
                            return Err(CommandParseError::UnterminatedArgument(s))
                        }
                        Some((_, s)) => return Err(CommandParseError::InvalidFlagValue(s)),
                        None => return Err(CommandParseError::InvalidFlagValue(equals_token)),
                    },
                    Some(token) => peeked_token = Some(token),
                    None => add_flag(&mut args, flag_token, "")?,
                },
                (TokenKind::Equals, s) | (TokenKind::Bang, s) => {
                    return Err(CommandParseError::InvalidArgument(s))
                }
                (TokenKind::Unterminated, s) => {
                    return Err(CommandParseError::UnterminatedArgument(s))
                }
            }
        }

        Ok((command, args))
    }
}

#[derive(Default)]
pub struct CommandArgs<'a> {
    bang: bool,
    values: [&'a str; MAX_COMMAND_ARGUMENT_VALUE_COUNT],
    values_len: usize,
    flags: [(&'a str, &'a str); MAX_COMMAND_ARGUMENT_FLAG_COUNT],
    flags_len: usize,
}
impl<'a> CommandArgs<'a> {
    pub fn values(&self) -> &[&'a str] {
        &self.values[..self.values_len]
    }

    pub fn flags(&self) -> &[(&'a str, &'a str)] {
        &self.flags[..self.flags_len]
    }
}

/*
#[cfg(test)]
mod tests {
    use super::*;

    fn create_commands() -> CommandManager {
        let builtin_commands = &[BuiltinCommand {
            names: &["command-name", "c"],
            help: "",
            accepts_bang: false,
            values_completion_source: None,
            switches: &[],
            options: &[],
            func: |_| None,
        }];

        CommandManager {
            builtin_commands,
            parsed_args: Some(CommandArgs::default()),
            history: VecDeque::default(),
        }
    }

    #[test]
    fn command_parsing() {
        let mut commands = create_commands();

        macro_rules! assert_command {
            ($text:expr => bang = $bang:expr) => {
                let (func, bang) = match commands.parse($text) {
                    Ok(result) => result,
                    Err(_) => panic!("command parse error"),
                };
                assert_eq!(commands.builtin_commands[0].func as usize, func as usize);
                assert_eq!($bang, bang);
            };
        }

        assert_command!("command-name" => bang = false);
        assert_command!("  command-name  " => bang = false);
        assert_command!("  command-name!  " => bang = true);
        assert_command!("  command-name!" => bang = true);
    }

    #[test]
    fn arg_parsing() {
        fn parse_args<'a>(commands: &'a mut CommandManager, params: &str) -> &'a CommandArgs {
            let mut command = String::new();
            command.push_str("command-name ");
            command.push_str(params);

            if let Err(_) = commands.parse(&command) {
                panic!("command parse error");
            }
            commands.parsed_args.as_ref().unwrap()
        }

        let mut commands = create_commands();

        let args = parse_args(&mut commands, "  aaa  bbb  ccc  ");
        assert_eq!(3, args.values().len());
        assert_eq!(0, args.switches().len());
        assert_eq!(0, args.options().len());

        assert_eq!("aaa", args.values()[0].as_str(&args));
        assert_eq!("bbb", args.values()[1].as_str(&args));
        assert_eq!("ccc", args.values()[2].as_str(&args));

        let args = parse_args(&mut commands, "  'aaa'  \"bbb\"  ccc  ");
        assert_eq!(3, args.values().len());
        assert_eq!(0, args.switches().len());
        assert_eq!(0, args.options().len());

        assert_eq!("aaa", args.values()[0].as_str(&args));
        assert_eq!("bbb", args.values()[1].as_str(&args));
        assert_eq!("ccc", args.values()[2].as_str(&args));

        let args = parse_args(&mut commands, "  'aaa'\"bbb\"\"ccc\"ddd  ");
        assert_eq!(4, args.values().len());
        assert_eq!(0, args.switches().len());
        assert_eq!(0, args.options().len());

        assert_eq!("aaa", args.values()[0].as_str(&args));
        assert_eq!("bbb", args.values()[1].as_str(&args));
        assert_eq!("ccc", args.values()[2].as_str(&args));
        assert_eq!("ddd", args.values()[3].as_str(&args));

        let args = parse_args(
            &mut commands,
            "\\\n-switch'value'\\\n-option=\"option value!\"\\\n",
        );

        assert_eq!(1, args.values().len());
        assert_eq!(1, args.switches().len());
        assert_eq!(1, args.options().len());

        assert_eq!("value", args.values()[0].as_str(&args));
        assert_eq!("switch", args.switches()[0].as_str(&args));
        assert_eq!("option", args.options()[0].0.as_str(&args));
        assert_eq!("option value!", args.options()[0].1.as_str(&args));
    }

    #[test]
    fn command_parsing_fail() {
        let mut commands = create_commands();

        macro_rules! assert_fail {
            ($command:expr, $error_pattern:pat => $value:ident == $expect:expr) => {
                let result = commands.parse($command);
                match result {
                    Ok(_) => panic!("command parsed successfully"),
                    Err($error_pattern) => assert_eq!($expect, $value),
                    Err(_) => panic!("other error occurred"),
                }
            };
        }

        assert_fail!("", CommandParseError::InvalidCommandName(i) => i == 0);
        assert_fail!("   ", CommandParseError::InvalidCommandName(i) => i == 3);
        assert_fail!(" !", CommandParseError::InvalidCommandName(i) => i == 1);
        assert_fail!("!  'aa'", CommandParseError::InvalidCommandName(i) => i == 0);
        assert_fail!("c -o=", CommandParseError::InvalidOptionValue(i) => i == 4);
        assert_fail!("  a \"aa\"", CommandParseError::CommandNotFound(i) => i == 2);

        assert_fail!("c! 'abc", CommandParseError::UnterminatedArgument(i) => i == 3);
        assert_fail!("c! '", CommandParseError::UnterminatedArgument(i) => i == 3);
        assert_fail!("c! \"'", CommandParseError::UnterminatedArgument(i) => i == 3);
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
*/
