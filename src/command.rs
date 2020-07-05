use std::collections::HashMap;

use crate::{
    buffer::BufferCollection, buffer_view::BufferViewCollection, viewport::ViewportCollection,
};

pub struct CommandContext<'a> {
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub viewports: &'a mut ViewportCollection,
}

pub enum CommandOperation {
    None,
    Quit,
    Error(String),
}

type CommandBody = fn(CommandContext, &str) -> CommandOperation;

pub struct CommandCollection {
    commands: HashMap<String, CommandBody>,
}

impl Default for CommandCollection {
    fn default() -> Self {
        let mut this = Self {
            commands: HashMap::new(),
        };

        this.register("quit".into(), commands::quit);
        this.register("edit".into(), commands::edit);

        this
    }
}

impl CommandCollection {
    pub fn register(&mut self, name: String, body: CommandBody) {
        self.commands.insert(name, body);
    }

    pub fn execute(&self, name: &str, ctx: CommandContext, args: &str) -> CommandOperation {
        if let Some(command) = self.commands.get(name) {
            command(ctx, args)
        } else {
            CommandOperation::Error("no such command".into())
        }
    }
}

mod commands {
    use super::*;

    pub fn quit(_ctx: CommandContext, _args: &str) -> CommandOperation {
        CommandOperation::Quit
    }

    pub fn edit(_ctx: CommandContext, args: &str) -> CommandOperation {
        if args.len() > 0 {
            CommandOperation::None
        } else {
            CommandOperation::Error("no file supplied".into())
        }
    }
}
