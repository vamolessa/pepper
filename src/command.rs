use std::{collections::HashMap, fs::File, io::Read, path::PathBuf};

use crate::{
    buffer::{Buffer, BufferCollection, BufferContent},
    buffer_view::{BufferView, BufferViewCollection},
    viewport::ViewportCollection,
};

pub enum CommandOperation {
    Complete,
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
        this.register("write".into(), commands::write);

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
    let content = match File::open(&path) {
        Ok(mut file) => {
            let mut content = String::new();
            match file.read_to_string(&mut content) {
                Ok(_) => (),
                Err(error) => {
                    return Err(format!(
                        "could not read contents from file {:?}: {:?}",
                        path, error
                    ))
                }
            }
            BufferContent::from_str(&content[..])
        }
        Err(_) => BufferContent::from_str(""),
    };

    new_buffer_from_content(ctx, Some(path), content);
    Ok(())
}

mod commands {
    use super::*;

    macro_rules! assert_empty {
        ($args:ident) => {
            if $args.trim().len() > 0 {
                return CommandOperation::Error(format!("invalid command arguments '{}'", $args));
            }
        };
    }

    pub fn quit(_ctx: CommandContext, args: &str) -> CommandOperation {
        assert_empty!(args);
        CommandOperation::Quit
    }

    pub fn edit(mut ctx: CommandContext, args: &str) -> CommandOperation {
        match new_buffer_from_file(&mut ctx, args.into()) {
            Ok(()) => CommandOperation::Complete,
            Err(error) => CommandOperation::Error(error),
        }
    }

    pub fn write(ctx: CommandContext, args: &str) -> CommandOperation {
        let handle = match ctx
            .viewports
            .current_viewport()
            .current_buffer_view_handle()
        {
            Some(handle) => handle,
            None => return CommandOperation::Error("no buffer opened".into()),
        };

        let buffer = match ctx
            .buffers
            .get_mut(ctx.buffer_views.get(handle).buffer_handle)
        {
            Some(buffer) => buffer,
            None => return CommandOperation::Error("no buffer opened".into()),
        };

        let path_arg = args.trim();
        if path_arg.len() > 0 {
            buffer.path = Some(path_arg.into());
        }

        let path = match &buffer.path {
            Some(path) => path,
            None => return CommandOperation::Error("buffer has no path".into()),
        };

        let mut file = match File::create(path) {
            Ok(file) => file,
            Err(error) => {
                return CommandOperation::Error(format!(
                    "could not create file {:?}: {:?}",
                    path, error
                ))
            }
        };

        match buffer.content.write(&mut file) {
            Ok(()) => CommandOperation::Complete,
            Err(error) => {
                return CommandOperation::Error(format!(
                    "could not write to file {:?}: {:?}",
                    path, error
                ))
            }
        }
    }
}
