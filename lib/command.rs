use std::collections::VecDeque;

use crate::{
    buffer_view::BufferViewHandle,
    client::{ClientManager, ClientHandle},
    editor::{Editor, EditorOutput, EditorOutputKind},
};

mod builtin;

pub const HISTORY_CAPACITY: usize = 10;

pub enum CommandParseError {
    InvalidCommandName(usize),
    CommandNotFound(usize),
    InvalidSwitchOrOption(usize),
    InvalidOptionValue(usize),
    UnterminatedArgument(usize),
}

type CommandFn = fn(CommandContext) -> Option<CommandOperation>;

pub enum CommandOperation {
    Quit,
    QuitAll,
}

enum CompletionSource {
    None,
    Files,
    Buffers,
    Commands,
    Custom(&'static [&'static str]),
}

struct CommandContext<'a> {
    editor: &'a mut Editor,
    clients: &'a mut ClientManager,
    client_handle: Option<ClientHandle>,
    bang: bool,
    args: &'a CommandArgs,
}
impl<'a> CommandContext<'a> {
    pub fn current_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.clients.get(self.client_handle?)?.buffer_view_handle()
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
    completion_source: CompletionSource,
    flags: &'static [(&'static str, CompletionSource)],
    func: CommandFn,
}

pub struct CommandManager {
    builtin_commands: &'static [BuiltinCommand],
    parsed_args: CommandArgs,
    history: VecDeque<String>,
}

impl CommandManager {
    pub fn new() -> Self {
        Self {
            builtin_commands: builtin::COMMANDS,
            parsed_args: CommandArgs::default(),
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
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
    ) -> Option<CommandOperation> {
        let command = editor.read_line.input();
        match editor.commands.parse(command) {
            Ok((command, bang)) => Self::eval_parsed(editor, clients, client_handle, command, bang),
            Err(error) => {
                Self::format_parse_error(&mut editor.output, error, command);
                None
            }
        }
    }

    pub fn eval(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: &str,
    ) -> Option<CommandOperation> {
        match editor.commands.parse(command) {
            Ok((command, bang)) => Self::eval_parsed(editor, clients, client_handle, command, bang),
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

        match error {
            CommandParseError::InvalidCommandName(i) => write.fmt(format_args!(
                "{:>index$} invalid command name",
                '^',
                index = i + 1
            )),
            CommandParseError::CommandNotFound(i) => write.fmt(format_args!(
                "{:>index$} command command not found",
                '^',
                index = i + 1
            )),
            CommandParseError::InvalidSwitchOrOption(i) => write.fmt(format_args!(
                "{:>index$} invalid switch or option",
                '^',
                index = i
            )),
            CommandParseError::InvalidOptionValue(i) => write.fmt(format_args!(
                "{:>index$} invalid option value",
                '^',
                index = i + 1
            )),
            CommandParseError::UnterminatedArgument(i) => write.fmt(format_args!(
                "{:>index$} unterminated argument",
                '^',
                index = i + 1
            )),
        }
    }

    fn eval_parsed(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_handle: Option<ClientHandle>,
        command: CommandFn,
        bang: bool,
    ) -> Option<CommandOperation> {
        let mut args = CommandArgs::default();
        std::mem::swap(&mut args, &mut editor.commands.parsed_args);

        let ctx = CommandContext {
            editor,
            clients,
            client_handle,
            bang,
            args: &args,
        };
        let result = command(ctx);

        std::mem::swap(&mut args, &mut editor.commands.parsed_args);
        result
    }

    fn parse(&mut self, text: &str) -> Result<(CommandFn, bool), CommandParseError> {
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
                fn is_separator(c: char) -> bool {
                    c.is_ascii_whitespace() || c == '=' || c == '!' || c == '"' || c == '\''
                }

                self.rest = self
                    .rest
                    .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\\');
                if self.rest.is_empty() {
                    return None;
                }

                match self.rest.as_bytes()[0] {
                    b'-' => {
                        self.rest = &self.rest[1..];
                        let (token, rest) = match self.rest.find(is_separator) {
                            Some(i) => self.rest.split_at(i),
                            None => (self.rest, ""),
                        };
                        self.rest = rest;
                        Some((TokenKind::Flag, token))
                    }
                    delim @ b'"' | delim @ b'\'' => {
                        self.rest = &self.rest[1..];
                        match self.rest.find(delim as char) {
                            Some(i) => {
                                let (token, rest) = (&self.rest[..i], &self.rest[(i + 1)..]);
                                self.rest = rest;
                                Some((TokenKind::Text, token))
                            }
                            None => {
                                let token = self.rest;
                                self.rest = "";
                                Some((TokenKind::Unterminated, token))
                            }
                        }
                    }
                    b'=' => {
                        let (token, rest) = self.rest.split_at(1);
                        self.rest = rest;
                        Some((TokenKind::Equals, token))
                    }
                    b'!' => {
                        let (token, rest) = self.rest.split_at(1);
                        self.rest = rest;
                        Some((TokenKind::Bang, token))
                    }
                    _ => match self.rest.find(is_separator) {
                        Some(i) => {
                            let (token, rest) = self.rest.split_at(i);
                            self.rest = rest;
                            Some((TokenKind::Text, token))
                        }
                        None => {
                            let token = self.rest;
                            self.rest = "";
                            Some((TokenKind::Text, token))
                        }
                    },
                }
            }
        }

        fn push_str_and_get_range(texts: &mut String, s: &str) -> CommandTextRange {
            let from = texts.len() as _;
            texts.push_str(s);
            let to = texts.len() as _;
            CommandTextRange { from, to }
        }

        fn error_index(text: &str, token: &str) -> usize {
            token.as_ptr() as usize - text.as_ptr() as usize
        }

        self.parsed_args.clear();

        let mut tokens = TokenIterator { rest: text }.peekable();

        let command = match tokens.next() {
            Some((TokenKind::Text, s)) => {
                match self.builtin_commands.iter().find(|&c| c.names.contains(&s)) {
                    Some(command) => command.func,
                    None => {
                        let error_index = error_index(text, s);
                        return Err(CommandParseError::CommandNotFound(error_index));
                    }
                }
            }
            Some((_, s)) => {
                let error_index = error_index(text, s);
                return Err(CommandParseError::InvalidCommandName(error_index));
            }
            None => {
                let error_index = error_index(text, text.trim_start());
                return Err(CommandParseError::InvalidCommandName(error_index));
            }
        };

        let bang = match tokens.peek() {
            Some((TokenKind::Bang, _)) => {
                tokens.next();
                true
            }
            _ => false,
        };

        loop {
            match tokens.next() {
                Some((TokenKind::Text, s)) => {
                    let range = push_str_and_get_range(&mut self.parsed_args.texts, s);
                    self.parsed_args.values.push(range);
                }
                Some((TokenKind::Flag, s)) => {
                    let flag_range = push_str_and_get_range(&mut self.parsed_args.texts, s);
                    match tokens.peek() {
                        Some((TokenKind::Equals, equals_slice)) => {
                            let equals_index = error_index(text, equals_slice);
                            tokens.next();
                            match tokens.next() {
                                Some((TokenKind::Text, s)) => {
                                    let value_range =
                                        push_str_and_get_range(&mut self.parsed_args.texts, s);
                                    self.parsed_args.options.push((flag_range, value_range));
                                }
                                Some((TokenKind::Unterminated, s)) => {
                                    let error_index = error_index(text, s);
                                    return Err(CommandParseError::UnterminatedArgument(
                                        error_index,
                                    ));
                                }
                                Some((_, s)) => {
                                    let error_index = error_index(text, s);
                                    return Err(CommandParseError::InvalidOptionValue(error_index));
                                }
                                None => {
                                    return Err(CommandParseError::InvalidOptionValue(
                                        equals_index,
                                    ));
                                }
                            }
                        }
                        _ => self.parsed_args.switches.push(flag_range),
                    }
                }
                Some((TokenKind::Equals, s)) | Some((TokenKind::Bang, s)) => {
                    let error_index = error_index(text, s);
                    return Err(CommandParseError::InvalidSwitchOrOption(error_index));
                }
                Some((TokenKind::Unterminated, s)) => {
                    let error_index = error_index(text, s) - 1;
                    return Err(CommandParseError::UnterminatedArgument(error_index));
                }
                None => break,
            }
        }

        Ok((command, bang))
    }
}

#[derive(Clone, Copy)]
pub struct CommandTextRange {
    from: u16,
    to: u16,
}
impl CommandTextRange {
    pub fn as_str(self, args: &CommandArgs) -> &str {
        &args.texts[(self.from as usize)..(self.to as usize)]
    }
}
#[derive(Default)]
pub struct CommandArgs {
    texts: String,
    values: Vec<CommandTextRange>,
    switches: Vec<CommandTextRange>,
    options: Vec<(CommandTextRange, CommandTextRange)>,
}
impl CommandArgs {
    pub fn values(&self) -> &[CommandTextRange] {
        &self.values
    }

    pub fn switches(&self) -> &[CommandTextRange] {
        &self.switches
    }

    pub fn options(&self) -> &[(CommandTextRange, CommandTextRange)] {
        &self.options
    }

    fn clear(&mut self) {
        self.texts.clear();
        self.values.clear();
        self.switches.clear();
        self.options.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_commands() -> CommandManager {
        let builtin_commands = &[BuiltinCommand {
            names: &["command-name", "c"],
            help: "",
            completion_source: CompletionSource::None,
            flags: &[],
            func: |_| None,
        }];

        CommandManager {
            builtin_commands,
            parsed_args: CommandArgs::default(),
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
            &commands.parsed_args
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
