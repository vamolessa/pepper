use std::borrow::Cow;

use crate::{client::ClientManager, editor::Editor};

mod builtin;

pub enum CommandParseError {
    InvalidCommandName(usize),
    CommandNotFound(usize),
    InvalidArgument(usize),
    InvalidOptionValue(usize),
    UnterminatedArgument(usize),
}

pub type CommandResult = Result<Option<CommandOperation>, Cow<'static, str>>;
type CommandFn = fn(CommandContext) -> CommandResult;

pub enum CommandOperation {
    Quit,
    QuitAll,
}

#[repr(u8)]
enum CompletionSource {
    None = 0b0,
    Files = 0b1,
    Buffers = 0b10,
    Commands = 0b100,
}

struct CommandContext<'a> {
    editor: &'a mut Editor,
    clients: &'a mut ClientManager,
    client_index: usize,
    bang: bool,
    args: &'a CommandArgs,
}

pub struct BuiltinCommand {
    name: &'static str,
    alias: Option<&'static str>,
    help: &'static str,
    completion_sources: u8,
    params: &'static [(&'static str, u8)],
    func: CommandFn,
}

pub struct CommandManager {
    builtin_commands: Vec<BuiltinCommand>,
    parsed_args: CommandArgs,
}

impl CommandManager {
    pub fn new() -> Self {
        let mut this = Self {
            builtin_commands: Vec::new(),
            parsed_args: CommandArgs::default(),
        };
        builtin::register_all(&mut this);
        this
    }

    pub fn register_builtin(&mut self, command: BuiltinCommand) {
        self.builtin_commands.push(command);
    }

    pub fn eval_from_read_line(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_index: usize,
    ) -> CommandResult {
        let command = editor.read_line.input();
        let result = editor.commands.parse(command);
        Self::eval_parsed(editor, clients, client_index, result)
    }

    pub fn eval(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_index: usize,
        command: &str,
    ) -> CommandResult {
        let result = editor.commands.parse(command);
        Self::eval_parsed(editor, clients, client_index, result)
    }

    fn eval_parsed(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_index: usize,
        parsed: Result<(CommandFn, bool), CommandParseError>,
    ) -> CommandResult {
        match parsed {
            Ok((command, bang)) => {
                let mut args = CommandArgs::default();
                std::mem::swap(&mut args, &mut editor.commands.parsed_args);
                let ctx = CommandContext {
                    editor,
                    clients,
                    client_index,
                    bang,
                    args: &args,
                };
                let result = command(ctx);
                std::mem::swap(&mut args, &mut editor.commands.parsed_args);
                result
            }
            // TODO: point error location
            Err(CommandParseError::InvalidCommandName(i)) => Err("invalid command name".into()),
            Err(CommandParseError::CommandNotFound(i)) => Err("command not found".into()),
            Err(CommandParseError::InvalidArgument(i)) => Err("invalid argument".into()),
            Err(CommandParseError::InvalidOptionValue(i)) => Err("invalid option value".into()),
            Err(CommandParseError::UnterminatedArgument(i)) => Err("unterminated argument".into()),
        }
    }

    fn parse<'a>(&mut self, text: &str) -> Result<(CommandFn, bool), CommandParseError> {
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
                    c == ' ' || c == '=' || c == '!' || c == '"' || c == '\''
                }

                self.rest = self.rest.trim_start();
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
                match self
                    .builtin_commands
                    .iter()
                    .find(|c| c.alias == Some(s) || c.name == s)
                {
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
                    return Err(CommandParseError::InvalidArgument(error_index));
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
        let mut commands = CommandManager {
            builtin_commands: Vec::new(),
            parsed_args: CommandArgs::default(),
        };
        commands.register_builtin(BuiltinCommand {
            name: "command-name",
            alias: Some("c"),
            help: "",
            completion_sources: CompletionSource::None as _,
            params: &[],
            func: |_| Ok(None),
        });
        commands
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
            if let Err(_) = commands.parse(&format!("command-name {}", params)) {
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

        let args = parse_args(&mut commands, "-switch'value'-option=\"option value!\"");
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
}
