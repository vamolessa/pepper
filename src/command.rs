use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use crate::{
    buffer::{Buffer, BufferCollection, BufferContent},
    buffer_view::{BufferView, BufferViewCollection, BufferViewHandle},
    connection::TargetClient,
    editor::{EditorOperation, EditorOperationSender},
};

pub enum CommandOperation {
    Complete,
    Quit,
    Error(String),
}

pub struct CommandContext<'a> {
    pub target_client: TargetClient,
    pub operations: &'a mut EditorOperationSender,

    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub current_buffer_view_handle: &'a mut Option<BufferViewHandle>,
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
        this.register("close".into(), commands::close);
        this.register("write".into(), commands::write);
        this.register("write-all".into(), commands::write_all);

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

mod helper {
    use super::*;

    pub fn new_buffer_from_content(
        ctx: &mut CommandContext,
        path: Option<PathBuf>,
        content: BufferContent,
    ) {
        ctx.operations.send_content(ctx.target_client, &content);
        ctx.operations
            .send(ctx.target_client, EditorOperation::Path(path.clone()));

        let buffer_handle = ctx.buffers.add(Buffer::new(path, content));
        let buffer_view = BufferView::new(ctx.target_client, buffer_handle);
        let buffer_view_handle = ctx.buffer_views.add(buffer_view);
        *ctx.current_buffer_view_handle = Some(buffer_view_handle);
    }

    pub fn new_buffer_from_file(ctx: &mut CommandContext, path: &Path) -> Result<(), String> {
        if let Some(buffer_handle) = ctx.buffers.find_with_path(path) {
            let view = match ctx
                .buffer_views
                .iter()
                .filter_map(|view| {
                    if view.buffer_handle == buffer_handle
                        && view.target_client == ctx.target_client
                    {
                        Some(view)
                    } else {
                        None
                    }
                })
                .next()
            {
                Some(view) => view.clone_with_target_client(ctx.target_client),
                None => BufferView::new(ctx.target_client, buffer_handle),
            };

            ctx.operations.send_content(
                ctx.target_client,
                &ctx.buffers.get(buffer_handle).unwrap().content,
            );
            ctx.operations
                .send(ctx.target_client, EditorOperation::Path(Some(path.into())));
            ctx.operations
                .send_cursors(ctx.target_client, &view.cursors);

            let view_handle = ctx.buffer_views.add(view);
            *ctx.current_buffer_view_handle = Some(view_handle);
        } else {
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

            new_buffer_from_content(ctx, Some(path.into()), content);
        }

        Ok(())
    }

    pub fn write_buffer_to_file(buffer: &Buffer, path: &Path) -> Result<(), String> {
        let mut file =
            File::create(path).map_err(|e| format!("could not create file {:?}: {:?}", path, e))?;

        buffer
            .content
            .write(&mut file)
            .map_err(|e| format!("could not write to file {:?}: {:?}", path, e))
    }
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
        match helper::new_buffer_from_file(&mut ctx, Path::new(args)) {
            Ok(()) => CommandOperation::Complete,
            Err(error) => CommandOperation::Error(error),
        }
    }

    pub fn close(ctx: CommandContext, args: &str) -> CommandOperation {
        assert_empty!(args);
        if let Some(handle) = ctx
            .current_buffer_view_handle
            .take()
            .map(|h| ctx.buffer_views.get(&h).buffer_handle)
        {
            for view in ctx.buffer_views.iter() {
                if view.buffer_handle == handle {
                    ctx.operations.send_empty_content(view.target_client);
                    ctx.operations
                        .send(view.target_client, EditorOperation::Path(None));
                }
            }
            ctx.buffer_views
                .remove_where(|view| view.buffer_handle == handle);
        }

        CommandOperation::Complete
    }

    pub fn write(ctx: CommandContext, args: &str) -> CommandOperation {
        let view_handle = match ctx.current_buffer_view_handle {
            Some(handle) => handle,
            None => return CommandOperation::Error("no buffer opened".into()),
        };

        let buffer_handle = ctx.buffer_views.get(view_handle).buffer_handle;
        let buffer = match ctx.buffers.get_mut(buffer_handle) {
            Some(buffer) => buffer,
            None => return CommandOperation::Error("no buffer opened".into()),
        };

        let path_arg = args.trim();
        if path_arg.is_empty() {
            let path = match &buffer.path {
                Some(path) => path,
                None => return CommandOperation::Error("buffer has no path".into()),
            };
            match helper::write_buffer_to_file(buffer, &path) {
                Ok(()) => CommandOperation::Complete,
                Err(error) => CommandOperation::Error(error),
            }
        } else {
            let path = PathBuf::from(path_arg);
            match helper::write_buffer_to_file(buffer, &path) {
                Ok(()) => {
                    for view in ctx.buffer_views.iter() {
                        if view.buffer_handle == buffer_handle {
                            ctx.operations.send(
                                view.target_client,
                                EditorOperation::Path(Some(path.clone())),
                            );
                        }
                    }
                    buffer.path = Some(path.clone());
                    CommandOperation::Complete
                }
                Err(error) => CommandOperation::Error(error),
            }
        }
    }

    pub fn write_all(ctx: CommandContext, args: &str) -> CommandOperation {
        assert_empty!(args);
        for buffer in ctx.buffers.iter() {
            if let Some(ref path) = buffer.path {
                match helper::write_buffer_to_file(buffer, path) {
                    Ok(()) => (),
                    Err(error) => return CommandOperation::Error(error),
                }
            }
        }

        CommandOperation::Complete
    }
}
