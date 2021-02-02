use crate::{client::ClientManager, editor::Editor};

mod builtin;

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
    args: &'a str,
}

struct BuiltinCommand {
    name: &'static str,
    alias: Option<&'static str>,
    help: &'static str,
    completion_sources: u8,
    func: fn(CommandContext) -> Option<CommandOperation>,
}

pub struct CommandManager {
    builtin_commands: Vec<BuiltinCommand>,
    executing_command: String,
}

impl CommandManager {
    pub fn new() -> Self {
        let mut this = Self {
            builtin_commands: Vec::new(),
            executing_command: String::new(),
        };
        builtin::register_all(&mut this);
        this
    }

    pub fn register_builtin(&mut self, command: BuiltinCommand) {
        self.builtin_commands.push(command);
    }

    pub fn eval(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_index: usize,
    ) -> Option<CommandOperation> {
        None
    }
}

fn parse_command(text: &str) -> (&str, bool, CommandArgs) {
    let text = text.trim();
    for i in 0..text.len() {
        let bang = match text.as_bytes()[i] {
            b' ' => false,
            b'!' => true,
            _ => continue,
        };

        let command = &text[..i];
        let rest = &text[(i + 1)..];
        return (command, bang, CommandArgs { rest });
    }

    (text, false, CommandArgs { rest: "" })
}

pub enum ArgParseError {
    UnterminatedString,
}
pub struct CommandArgs<'a> {
    rest: &'a str,
}
impl<'a> Iterator for CommandArgs<'a> {
    type Item = Result<&'a str, ArgParseError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.rest = self.rest.trim_start();
        let mut bytes = self.rest.bytes();
        match bytes.next() {
            None => None,
            Some(delim @ b'"') | Some(delim @ b'\'') => match bytes.position(|b| b == delim) {
                Some(i) => {
                    let (arg, rest) = self.rest[1..].split_at(i);
                    self.rest = &rest[1..];
                    Some(Ok(arg))
                }
                None => Some(Err(ArgParseError::UnterminatedString)),
            },
            Some(_) => {
                let end = match bytes.position(|b| b == b' ' || b == b'"' || b == b'\'') {
                    Some(i) => i + 1,
                    None => self.rest.len(),
                };
                let (arg, rest) = self.rest.split_at(end);
                self.rest = rest;
                Some(Ok(arg))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_parsing() {
        macro_rules! assert_command {
            ($text:expr => ($command:expr, $bang:expr)) => {
                let (command, bang, _) = parse_command($text);
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
        macro_rules! assert_next_arg {
            ($args:expr, $arg:expr) => {
                match $args.next() {
                    Some(Ok(arg)) => assert_eq!($arg, arg),
                    Some(Err(_)) => panic!("arg parse error"),
                    None => panic!("no more args"),
                }
            };
        }

        fn args_from(text: &str) -> CommandArgs {
            CommandArgs { rest: text }
        }

        let mut args = args_from("  aaa  bbb  ccc  ");
        assert_next_arg!(args, "aaa");
        assert_next_arg!(args, "bbb");
        assert_next_arg!(args, "ccc");
        assert!(args.next().is_none());

        let mut args = args_from("  'aaa'  \"bbb\"  ccc  ");
        assert_next_arg!(args, "aaa");
        assert_next_arg!(args, "bbb");
        assert_next_arg!(args, "ccc");
        assert!(args.next().is_none());

        let mut args = args_from("  'aaa'\"bbb\"\"ccc\"ddd  ");
        assert_next_arg!(args, "aaa");
        assert_next_arg!(args, "bbb");
        assert_next_arg!(args, "ccc");
        assert_next_arg!(args, "ddd");
        assert!(args.next().is_none());
    }
}
