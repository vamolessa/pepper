use crate::{client::ClientManager, editor::Editor};

mod builtin;

pub enum CommandParseError {
    InvalidCommandName(usize),
    UnterminatedString(usize),
}

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
    args: CommandArgs<'a>,
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

fn parse_command(text: &str) -> Result<(&str, bool, CommandArgs), CommandParseError> {
    let text_original_len = text.len();
    let text = text.trim_start();
    let trim_offset = text_original_len - text.len();
    let text = text.trim_end();

    let mut command = text;
    let mut bang = false;
    let mut rest = "";

    for i in 0..text.len() {
        match text.as_bytes()[i] {
            b' ' => (),
            b'!' => bang = true,
            b'"' | b'\'' => return Err(CommandParseError::InvalidCommandName(trim_offset + i)),
            _ => continue,
        }

        command = &text[..i];
        rest = &text[(i + 1)..];
        break;
    }

    let command = command;
    let bang = bang;
    let rest = rest;

    if command.is_empty() {
        return Err(CommandParseError::InvalidCommandName(trim_offset));
    }

    let mut bytes = rest.bytes();
    loop {
        match bytes.next() {
            None => break,
            Some(delim @ b'"') | Some(delim @ b'\'') => {
                let pending_len = bytes.len();
                if let None = bytes.position(|b| b == delim) {
                    let i = rest.len() - pending_len + 1;
                    return Err(CommandParseError::UnterminatedString(trim_offset + i));
                }
            }
            Some(_) => continue,
        };
    }

    Ok((command, bang, CommandArgs { rest }))
}

pub struct CommandArgs<'a> {
    rest: &'a str,
}
impl<'a> Iterator for CommandArgs<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        self.rest = self.rest.trim_start();
        let mut bytes = self.rest.bytes();
        match bytes.next() {
            None => None,
            Some(delim @ b'"') | Some(delim @ b'\'') => match bytes.position(|b| b == delim) {
                Some(i) => {
                    let (arg, rest) = self.rest[1..].split_at(i);
                    self.rest = &rest[1..];
                    Some(arg)
                }
                None => unreachable!(),
            },
            Some(_) => {
                let end = match bytes.position(|b| b == b' ' || b == b'"' || b == b'\'') {
                    Some(i) => i + 1,
                    None => self.rest.len(),
                };
                let (arg, rest) = self.rest.split_at(end);
                self.rest = rest;
                Some(arg)
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
            }
        }

        assert_fail!("", CommandParseError::InvalidCommandName(i) => i == 0);
        assert_fail!("   ", CommandParseError::InvalidCommandName(i) => i == 3);
        assert_fail!(" !", CommandParseError::InvalidCommandName(i) => i == 1);
        assert_fail!("  'aa'", CommandParseError::InvalidCommandName(i) => i == 2);
        assert_fail!("  \"aa\"", CommandParseError::InvalidCommandName(i) => i == 2);
        assert_fail!("\"aa\"", CommandParseError::InvalidCommandName(i) => i == 0);

        assert_fail!("c! 'abc", CommandParseError::UnterminatedString(i) => i == 3);
        assert_fail!("c! '", CommandParseError::UnterminatedString(i) => i == 3);
        assert_fail!("c! \"'", CommandParseError::UnterminatedString(i) => i == 3);
    }
}
