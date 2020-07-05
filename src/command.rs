use std::{collections::HashMap, fs::File, io::Read, path::PathBuf};

use crate::{
    buffer::{Buffer, BufferCollection, BufferContent},
    buffer_view::{BufferView, BufferViewCollection},
    viewport::ViewportCollection,
};

pub enum CommandOperation {
    Done,
    Quit,
    Error(String),
}

pub struct CommandContext<'a> {
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub viewports: &'a mut ViewportCollection,
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

fn new_buffer_from_content(
    ctx: &mut CommandContext,
    path: Option<PathBuf>,
    content: BufferContent,
) {
    let buffer_handle = ctx.buffers.add(Buffer::new(path, content));
    let buffer_view_index = ctx.buffer_views.add(BufferView::with_handle(buffer_handle));
    ctx.viewports
        .current_viewport_mut()
        .set_current_buffer_view_handle(buffer_view_index);
}

fn new_buffer_from_file(ctx: &mut CommandContext, path: PathBuf) -> Result<(), String> {
    let mut file =
        File::open(&path).map_err(|e| format!("could not open file {:?}: {:?}", path, e))?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("could not read contents from file {:?}: {:?}", path, e))?;
    new_buffer_from_content(ctx, Some(path), BufferContent::from_str(&content[..]));
    Ok(())
}

mod commands {
    use super::*;

    pub fn quit(_ctx: CommandContext, _args: &str) -> CommandOperation {
        CommandOperation::Quit
    }

    pub fn edit(mut ctx: CommandContext, args: &str) -> CommandOperation {
        match new_buffer_from_file(&mut ctx, args.into()) {
            Ok(()) => CommandOperation::Done,
            Err(error) => CommandOperation::Error(error),
        }
    }
}
