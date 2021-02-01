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
