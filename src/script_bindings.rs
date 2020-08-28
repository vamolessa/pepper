use std::{
    fmt,
    fs::File,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

use crate::{
    buffer::{Buffer, BufferCollection, BufferContent, TextRef},
    buffer_view::{BufferView, BufferViewCollection, BufferViewHandle},
    config::{Config, ParseConfigError},
    connection::{ConnectionWithClientHandle, TargetClient},
    editor::ClientTargetMap,
    editor_operation::{EditorOperation, EditorOperationSerializer, StatusMessageKind},
    keymap::{KeyMapCollection, ParseKeyMapError},
    mode::Mode,
    pattern::Pattern,
    script::{ScriptContext, ScriptEngine, ScriptResult, ScriptStr},
    syntax::TokenKind,
    theme::ParseThemeError,
};

pub fn bind_all<'a>(scripts: &'a mut ScriptEngine) -> ScriptResult<()> {
    macro_rules! register {
        ($func:ident) => {
            scripts.register_ctx_function(stringify!($func), bindings::$func)
        }
    }

    register!(print)?;
    Ok(())
}

mod bindings {
    use super::*;

    pub fn print(ctx: &mut ScriptContext, message: ScriptStr) -> ScriptResult<()> {
        let message = message.to_str()?;
        println!("printing: {}", message);
        ctx.operations.serialize(
            TargetClient::All,
            &EditorOperation::StatusMessage(StatusMessageKind::Info, message),
        );
        Ok(())
    }
}

mod helper {
    use super::*;

    pub fn parsing_error<T>(message: T, text: &str, error_index: usize) -> String
    where
        T: fmt::Display,
    {
        let (before, after) = text.split_at(error_index);
        match (before.len(), after.len()) {
            (0, 0) => format!("{} at ''", message),
            (_, 0) => format!("{} at '{}' <- here", message, before),
            (0, _) => format!("{} at here -> '{}'", message, after),
            (_, _) => format!("{} at '{}' <- here '{}'", message, before, after),
        }
    }

    pub fn new_buffer_from_content(ctx: &mut ScriptContext, path: &Path, content: BufferContent) {
        ctx.operations.serialize_buffer(ctx.target_client, &content);
        ctx.operations
            .serialize(ctx.target_client, &EditorOperation::Path(path));

        let buffer_handle = ctx.buffers.add(Buffer::new(path.into(), content));
        let buffer_view = BufferView::new(ctx.target_client, buffer_handle);
        let buffer_view_handle = ctx.buffer_views.add(buffer_view);
        *ctx.current_buffer_view_handle = Some(buffer_view_handle);
    }

    pub fn new_buffer_from_file(ctx: &mut ScriptContext, path: &Path) -> Result<(), String> {
        if let Some(buffer_handle) = ctx.buffers.find_with_path(path) {
            let mut iter = ctx
                .buffer_views
                .iter_with_handles()
                .filter_map(|(handle, view)| {
                    if view.buffer_handle == buffer_handle
                        && view.target_client == ctx.target_client
                    {
                        Some((handle, view))
                    } else {
                        None
                    }
                });

            let view = match iter.next() {
                Some((handle, view)) => {
                    *ctx.current_buffer_view_handle = Some(handle);
                    view
                }
                None => {
                    drop(iter);
                    let view = BufferView::new(ctx.target_client, buffer_handle);
                    let view_handle = ctx.buffer_views.add(view);
                    let view = ctx.buffer_views.get(&view_handle);
                    *ctx.current_buffer_view_handle = Some(view_handle);
                    view
                }
            };

            ctx.operations.serialize_buffer(
                ctx.target_client,
                &ctx.buffers.get(buffer_handle).unwrap().content,
            );
            ctx.operations
                .serialize(ctx.target_client, &EditorOperation::Path(path));
            ctx.operations
                .serialize_cursors(ctx.target_client, &view.cursors);
        } else if path.to_str().map(|s| s.trim().len()).unwrap_or(0) > 0 {
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

            new_buffer_from_content(ctx, path, content);
        } else {
            return Err(format!("invalid path {:?}", path));
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
