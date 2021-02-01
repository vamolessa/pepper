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

fn parse_command(text: &str) -> (&str, bool, ArgIter) {
    let text = text.trim();
    for i in 0..text.len() {
        let bang = match text.as_bytes()[i] {
            b' ' => false,
            b'!' => true,
            _ => continue,
        };

        let command = &text[..i];
        let rest = &text[(i + 1)..];
        return (command, bang, ArgIter { rest });
    }

    (text, false, ArgIter { rest: "" })
}

pub enum ArgParseError {
    UnterminatedString(usize),
}
pub struct ArgIter<'a> {
    rest: &'a str,
}
impl<'a> Iterator for ArgIter<'a> {
    type Item = Result<&'a str, ArgParseError>;
    fn next(&mut self) -> Option<Self::Item> {
        self.rest = self.rest.trim_start();
        if self.rest.is_empty() {
            return None;
        }

        // TODO: finish
        match self.rest.as_bytes()[0]
        {
            b'"' => todo!("parse string"),
            b'\'' => todo!("parse string"),
            _ => (),
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_parsing() {
        assert!(matches!(parse_command("command-name"), ("command-name", false, _)));
        assert!(matches!(parse_command("  command-name  "), ("command-name", false, _)));
        assert!(matches!(parse_command("  command-name!  "), ("command-name", true, _)));
        assert!(matches!(parse_command("  command-name!"), ("command-name", true, _)));
    }
}
