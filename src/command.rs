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
    executing_args: CommandArgs,
}

impl CommandManager {
    pub fn new() -> Self {
        let mut this = Self {
            builtin_commands: Vec::new(),
            executing_args: CommandArgs::default(),
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
        let mut args = CommandArgs::default();
        std::mem::swap(&mut args, &mut editor.commands.executing_args);
        let result = Self::eval_parsed(editor, clients, client_index, result);
        std::mem::swap(&mut args, &mut editor.commands.executing_args);
        result
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
                std::mem::swap(&mut args, &mut editor.commands.executing_args);
                let ctx = CommandContext {
                    editor,
                    clients,
                    client_index,
                    bang,
                    args: &args,
                };
                let result = command(ctx);
                std::mem::swap(&mut args, &mut editor.commands.executing_args);
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
                        let (token, rest) = match self.rest.find(delim as char) {
                            Some(i) => (&self.rest[..i], &self.rest[(i + 1)..]),
                            None => (self.rest, ""),
                        };
                        self.rest = rest;
                        Some((TokenKind::Text, token))
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
                    b => match self.rest.find(is_separator) {
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
            text.as_ptr() as usize - token.as_ptr() as usize
        }

        self.executing_args.texts.clear();
        self.executing_args.args.clear();

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
                    let range = push_str_and_get_range(&mut self.executing_args.texts, s);
                    self.executing_args.args.push(CommandArg::Value(range));
                }
                Some((TokenKind::Flag, s)) => {
                    let flag_range = push_str_and_get_range(&mut self.executing_args.texts, s);
                    match tokens.peek() {
                        Some((TokenKind::Equals, equals_slice)) => {
                            let equals_index = error_index(text, equals_slice);
                            tokens.next();
                            match tokens.next() {
                                Some((TokenKind::Text, s)) => {
                                    let value_range =
                                        push_str_and_get_range(&mut self.executing_args.texts, s);
                                    self.executing_args
                                        .args
                                        .push(CommandArg::Option(flag_range, value_range));
                                }
                                Some((_, s)) => {
                                    let error_index = error_index(text, s);
                                    return Err(CommandParseError::InvalidOptionValue(error_index));
                                }
                                None => {
                                    return Err(CommandParseError::InvalidOptionValue(equals_index));
                                }
                            }
                        }
                        _ => self
                            .executing_args
                            .args
                            .push(CommandArg::Switch(flag_range)),
                    }
                }
                Some((TokenKind::Equals, s)) | Some((TokenKind::Bang, s)) => {
                    let error_index = error_index(text, s);
                    return Err(CommandParseError::InvalidArgument(error_index));
                }
                Some((TokenKind::Unterminated, s)) => {
                    let error_index = error_index(text, s);
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
pub enum CommandArg {
    Value(CommandTextRange),
    Switch(CommandTextRange),
    Option(CommandTextRange, CommandTextRange),
}
#[derive(Default)]
pub struct CommandArgs {
    texts: String,
    args: Vec<CommandArg>,
}
impl CommandArgs {
    pub fn iter(&self) -> impl Iterator<Item = &CommandArg> {
        self.args.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_parsing() {
        macro_rules! assert_command {
            ($text:expr => ($command:expr, $bang:expr)) => {
                let (command, bang, _) = match parse_command($text) {
                    Ok(command) => command,
                    Err(_) => panic!("command parse error"),
                };
                assert_eq!($command, command);
                assert_eq!($bang, bang);
            };
        }

        assert_command!("command-name" => ("command-name", false));
        assert_command!("  command-name  " => ("command-name", false));
        assert_command!("  command-name!  " => ("command-name", true));
        assert_command!("  command-name!" => ("command-name", true));
    }

    #[test]
    fn arg_parsing() {
        fn args_from(text: &str) -> CommandArgs {
            CommandArgs { rest: text }
        }

        let mut args = args_from("  aaa  bbb  ccc  ");
        assert_eq!(Some("aaa"), args.next());
        assert_eq!(Some("bbb"), args.next());
        assert_eq!(Some("ccc"), args.next());
        assert_eq!(None, args.next());

        let mut args = args_from("  'aaa'  \"bbb\"  ccc  ");
        assert_eq!(Some("aaa"), args.next());
        assert_eq!(Some("bbb"), args.next());
        assert_eq!(Some("ccc"), args.next());
        assert_eq!(None, args.next());

        let mut args = args_from("  'aaa'\"bbb\"\"ccc\"ddd  ");
        assert_eq!(Some("aaa"), args.next());
        assert_eq!(Some("bbb"), args.next());
        assert_eq!(Some("ccc"), args.next());
        assert_eq!(Some("ddd"), args.next());
        assert_eq!(None, args.next());
    }

    #[test]
    fn full_command_parsing() {
        let (command, bang, mut args) =
            match parse_command("  command-name! 'my arg 1' 034 another-arg   ") {
                Ok(command) => command,
                Err(_) => panic!("command parse error"),
            };
        assert_eq!("command-name", command);
        assert_eq!(true, bang);
        assert_eq!(Some("my arg 1"), args.next());
        assert_eq!(Some("034"), args.next());
        assert_eq!(Some("another-arg"), args.next());
        assert_eq!(None, args.next());
    }

    #[test]
    fn command_parsing_fail() {
        macro_rules! assert_fail {
            ($text:expr, $err:pat => $value:ident == $expect:expr) => {
                match parse_command($text) {
                    Ok(_) => panic!("command parsed successfuly"),
                    Err($err) => assert_eq!($expect, $value),
                    Err(_) => panic!("other error occurred"),
                }
            };
        }

        assert_fail!("", CommandParseError::InvalidCommandName(i) => i == 0);
        assert_fail!("   ", CommandParseError::InvalidCommandName(i) => i == 3);
        assert_fail!(" !", CommandParseError::InvalidCommandName(i) => i == 1);
        assert_fail!("  'aa'", CommandParseError::InvalidCommandName(i) => i == 2);
        assert_fail!("  \"aa\"", CommandParseError::InvalidCommandName(i) => i == 2);
        assert_fail!("\"aa\"", CommandParseError::InvalidCommandName(i) => i == 0);

        assert_fail!("c! 'abc", CommandParseError::UnterminatedArgument(i) => i == 3);
        assert_fail!("c! '", CommandParseError::UnterminatedArgument(i) => i == 3);
        assert_fail!("c! \"'", CommandParseError::UnterminatedArgument(i) => i == 3);
    }
}
