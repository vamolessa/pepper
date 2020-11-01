use std::{
    fmt,
    io::Write,
    num::NonZeroU8,
    path::Path,
    process::{Child, Command, Stdio},
};

use crate::{
    buffer::BufferHandle,
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    client::TargetClient,
    cursor::Cursor,
    editor::{EditorLoop, StatusMessageKind},
    keymap::ParseKeyMapError,
    mode::{self, Mode},
    navigation_history::NavigationHistory,
    pattern::Pattern,
    register::RegisterKey,
    script::{
        ScriptArray, ScriptContext, ScriptContextGuard, ScriptEngineRef, ScriptError,
        ScriptFunction, ScriptObject, ScriptResult, ScriptString, ScriptValue,
    },
    syntax::{Syntax, TokenKind},
    theme::Color,
};

pub struct QuitError;
impl fmt::Display for QuitError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("could not quit now")
    }
}

pub fn bind_all(scripts: ScriptEngineRef) -> ScriptResult<()> {
    macro_rules! register {
        ($namespace:ident => $($func:ident,)*) => {
            let globals = scripts.globals_object();
            let $namespace = scripts.create_object()?;
            $(
                let func = scripts.create_ctx_function($namespace::$func)?;
                $namespace.set(stringify!($func), ScriptValue::Function(func))?;
            )*
            globals.set(stringify!($namespace), ScriptValue::Object($namespace))?;
        };
    }

    macro_rules! register_object {
        ($name:ident) => {
            let $name = scripts.create_object()?;
            let meta = scripts.create_object()?;
            meta.set(
                "__index",
                ScriptValue::Function(scripts.create_ctx_function($name::index)?),
            )?;
            meta.set(
                "__newindex",
                ScriptValue::Function(scripts.create_ctx_function($name::newindex)?),
            )?;
            $name.set_meta_object(Some(meta));
            scripts
                .globals_object()
                .set(stringify!($name), ScriptValue::Object($name))?;
        };
    }

    register!(client => index, current_buffer_view_handle, quit, quit_all, force_quit_all,);
    register!(editor => version, print,);
    register!(buffer => all_handles, line_count, line_at, path, extension, has_extension, needs_save, set_search, open,
        close, force_close, close_all, force_close_all, save, save_all, reload, force_reload, reload_all, force_reload_all, commit_edits, on_open,);
    register!(buffer_view => buffer_handle, all_handles, handle_from_path, selection_text, insert_text,
        insert_text_at, delete_selection, delete_in, undo, redo,);
    register!(cursors => len, all, set_all, main_index, main, set, move_columns, move_lines, move_words,
        move_home, move_end, move_first_line, move_last_line,);
    register!(read_line => prompt, read,);
    register!(picker => prompt, reset, entry, pick,);
    register!(process => pipe, spawn,);
    register!(keymap => normal, insert,);
    register!(syntax => rules,);

    {
        let globals = scripts.globals_object();

        let editor = globals.get::<ScriptObject>("editor")?;
        globals.set("print", editor.get::<ScriptValue>("print")?)?;

        let client = globals.get::<ScriptObject>("client")?;
        globals.set("q", client.get::<ScriptValue>("quit")?)?;
        globals.set("qa", client.get::<ScriptValue>("quit_all")?)?;
        globals.set("fqa", client.get::<ScriptValue>("force_quit_all")?)?;

        let buffer = globals.get::<ScriptObject>("buffer")?;
        globals.set("o", buffer.get::<ScriptValue>("open")?)?;
        globals.set("c", buffer.get::<ScriptValue>("close")?)?;
        globals.set("fc", buffer.get::<ScriptValue>("force_close")?)?;
        globals.set("ca", buffer.get::<ScriptValue>("close_all")?)?;
        globals.set("fca", buffer.get::<ScriptValue>("force_close_all")?)?;
        globals.set("s", buffer.get::<ScriptValue>("save")?)?;
        globals.set("sa", buffer.get::<ScriptValue>("save_all")?)?;
        globals.set("r", buffer.get::<ScriptValue>("reload")?)?;
        globals.set("fr", buffer.get::<ScriptValue>("force_reload")?)?;
        globals.set("ra", buffer.get::<ScriptValue>("reload_all")?)?;
        globals.set("fra", buffer.get::<ScriptValue>("force_reload_all")?)?;
    }

    register_object!(config);
    register_object!(theme);
    register_object!(registers);

    Ok(())
}

mod client {
    use super::*;

    pub fn index(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<usize> {
        Ok(ctx.target_client.into_index())
    }

    pub fn current_buffer_view_handle(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        target: Option<usize>,
    ) -> ScriptResult<Option<BufferViewHandle>> {
        let target = target
            .map(|i| TargetClient::from_index(i))
            .unwrap_or(ctx.target_client);

        Ok(ctx
            .clients
            .get(target)
            .and_then(|c| c.current_buffer_view_handle()))
    }

    pub fn quit(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        let can_quit =
            ctx.target_client != TargetClient::Local || ctx.buffers.iter().all(|b| !b.needs_save());
        if can_quit {
            ctx.editor_loop = EditorLoop::Quit;
            Err(ScriptError::from(QuitError))
        } else {
            ctx.status_message.write_str(
                StatusMessageKind::Error,
                "there are unsaved changes in buffers. try 'force_quit_all' to force quit",
            );
            Ok(())
        }
    }

    pub fn quit_all(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        let can_quit = ctx.buffers.iter().all(|b| !b.needs_save());
        if can_quit {
            ctx.editor_loop = EditorLoop::QuitAll;
            Err(ScriptError::from(QuitError))
        } else {
            ctx.status_message.write_str(
                StatusMessageKind::Error,
                "there are unsaved changes in buffers. try 'force_quit_all' to force quit all",
            );
            Ok(())
        }
    }

    pub fn force_quit_all(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        ctx.editor_loop = EditorLoop::QuitAll;
        Err(ScriptError::from(QuitError))
    }
}

mod editor {
    use super::*;

    pub fn version<'a>(
        engine: ScriptEngineRef<'a>,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'a>> {
        engine
            .create_string(env!("CARGO_PKG_VERSION").as_bytes())
            .map(ScriptValue::String)
    }

    pub fn print(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        value: ScriptValue,
    ) -> ScriptResult<()> {
        ctx.status_message
            .write_fmt(StatusMessageKind::Info, format_args!("{}", value));
        Ok(())
    }
}

mod buffer {
    use super::*;

    pub fn all_handles<'a>(
        engine: ScriptEngineRef<'a>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'a>> {
        let handles = engine.create_array()?;
        for (h, _) in ctx.buffers.iter_with_handles() {
            handles.push(h)?;
        }
        Ok(ScriptValue::Array(handles))
    }

    pub fn line_count(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<Option<usize>> {
        Ok(handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get(h))
            .map(|b| b.content().line_count()))
    }

    pub fn line_at<'a>(
        engine: ScriptEngineRef<'a>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (index, handle): (usize, Option<BufferHandle>),
    ) -> ScriptResult<ScriptValue<'a>> {
        match handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get(h))
        {
            Some(buffer) => {
                if index < buffer.content().line_count() {
                    let line_bytes = buffer.content().line_at(index).as_str().as_bytes();
                    Ok(ScriptValue::String(engine.create_string(line_bytes)?))
                } else {
                    Ok(ScriptValue::Nil)
                }
            }
            None => Ok(ScriptValue::Nil),
        }
    }

    pub fn path<'a>(
        engine: ScriptEngineRef<'a>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<ScriptValue<'a>> {
        match handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get(h))
            .and_then(|b| b.path())
            .and_then(|p| p.to_str())
            .map(|p| p.as_bytes())
        {
            Some(bytes) => Ok(ScriptValue::String(engine.create_string(bytes)?)),
            None => Ok(ScriptValue::Nil),
        }
    }

    pub fn extension<'a>(
        engine: ScriptEngineRef<'a>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<ScriptValue<'a>> {
        match handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get(h))
            .and_then(|b| b.path())
            .and_then(|p| p.extension())
            .and_then(|p| p.to_str())
            .map(|p| p.as_bytes())
        {
            Some(bytes) => Ok(ScriptValue::String(engine.create_string(bytes)?)),
            None => Ok(ScriptValue::Nil),
        }
    }

    pub fn has_extension<'a>(
        _: ScriptEngineRef<'a>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (extension, handle): (ScriptString, Option<BufferHandle>),
    ) -> ScriptResult<bool> {
        Ok(handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get(h))
            .and_then(|b| b.path())
            .and_then(|p| p.extension())
            .and_then(|p| p.to_str())
            .map(|p| p.as_bytes() == extension.as_bytes())
            .unwrap_or(false))
    }

    pub fn needs_save(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<bool> {
        Ok(handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get(h))
            .map(|b| b.needs_save())
            .unwrap_or(false))
    }

    pub fn set_search(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (search, handle): (ScriptString, Option<BufferHandle>),
    ) -> ScriptResult<()> {
        let search = search.to_str()?;
        if let Some(buffer) = handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get_mut(h))
        {
            buffer.set_search(search);
        }

        Ok(())
    }

    pub fn open(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (path, line_number): (ScriptString, Option<usize>),
    ) -> ScriptResult<()> {
        NavigationHistory::save_client_snapshot(ctx.clients, ctx.buffer_views, ctx.target_client);

        let path = Path::new(path.to_str()?);
        let buffer_view_handle = ctx
            .buffer_views
            .buffer_view_handle_from_path(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                ctx.target_client,
                path,
                line_number.map(|l| l.saturating_sub(1)),
                ctx.events,
            )
            .map_err(ScriptError::from)?;
        ctx.set_current_buffer_view_handle(Some(buffer_view_handle));
        Ok(())
    }

    pub fn close(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<()> {
        if let Some(handle) = handle.or_else(|| ctx.current_buffer_handle()) {
            let unsaved = ctx
                .buffers
                .get(handle)
                .map(|b| b.needs_save())
                .unwrap_or(false);
            if unsaved {
                ctx.status_message.write_str(
                    StatusMessageKind::Error,
                    "there are unsaved changes in buffer. try 'force_close' to force close",
                );
                return Ok(());
            }

            ctx.buffer_views.remove_where(
                ctx.buffers,
                ctx.clients,
                ctx.word_database,
                ctx.events,
                |view| view.buffer_handle == handle,
            );
        }

        ctx.set_current_buffer_view_handle(None);
        Ok(())
    }

    pub fn force_close(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<()> {
        if let Some(handle) = handle.or_else(|| ctx.current_buffer_handle()) {
            ctx.buffer_views.remove_where(
                ctx.buffers,
                ctx.clients,
                ctx.word_database,
                ctx.events,
                |view| view.buffer_handle == handle,
            );
        }

        ctx.set_current_buffer_view_handle(None);
        Ok(())
    }

    pub fn close_all(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        let unsaved_buffers = ctx.buffers.iter().any(|b| b.needs_save());
        if unsaved_buffers {
            ctx.status_message.write_str(
                StatusMessageKind::Error,
                "there are unsaved changes in buffers. try 'force_close_all' to force close all",
            );
            Ok(())
        } else {
            ctx.buffer_views.remove_where(
                ctx.buffers,
                ctx.clients,
                ctx.word_database,
                ctx.events,
                |_| true,
            );
            for c in ctx.clients.client_refs() {
                c.client.set_current_buffer_view_handle(None);
            }
            Ok(())
        }
    }

    pub fn force_close_all(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        ctx.buffer_views.remove_where(
            ctx.buffers,
            ctx.clients,
            ctx.word_database,
            ctx.events,
            |_| true,
        );
        for c in ctx.clients.client_refs() {
            c.client.set_current_buffer_view_handle(None);
        }
        Ok(())
    }

    pub fn save(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (path, handle): (Option<ScriptString>, Option<BufferHandle>),
    ) -> ScriptResult<()> {
        let handle = match handle.or_else(|| ctx.current_buffer_handle()) {
            Some(handle) => handle,
            None => return Err(ScriptError::from("no buffer opened")),
        };

        let buffer = match ctx.buffers.get_mut(handle) {
            Some(buffer) => buffer,
            None => return Err(ScriptError::from("no buffer opened")),
        };

        if let Some(path) = path {
            let path = Path::new(path.to_str()?);
            buffer.set_path(&ctx.config.syntaxes, Some(path));
        }

        if let Some(path) = buffer.path().and_then(|p| p.to_str()) {
            ctx.status_message
                .write_fmt(StatusMessageKind::Info, format_args!("saved to '{}'", path));
        }

        buffer.save_to_file().map_err(ScriptError::from)
    }

    pub fn save_all(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        let mut buffer_count = 0;
        for buffer in ctx.buffers.iter_mut() {
            buffer.save_to_file().map_err(ScriptError::from)?;
            buffer_count += 1;
        }

        ctx.status_message.write_fmt(
            StatusMessageKind::Info,
            format_args!("{} buffers saved", buffer_count),
        );

        Ok(())
    }

    pub fn reload(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<()> {
        if let Some((buffer, line_pool)) = handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get_mut_with_line_pool(h))
        {
            if buffer.needs_save() {
                ctx.status_message.write_str(
                    StatusMessageKind::Error,
                    "there are unsaved changes in buffer. try 'force_reload' to force reload",
                );
                return Ok(());
            }

            if let Err(error) = buffer.discard_and_reload_from_file(line_pool) {
                ctx.status_message
                    .write_str(StatusMessageKind::Error, &error);
            }
        }
        Ok(())
    }

    pub fn force_reload(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<()> {
        if let Some((buffer, line_pool)) = handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get_mut_with_line_pool(h))
        {
            if let Err(error) = buffer.discard_and_reload_from_file(line_pool) {
                ctx.status_message
                    .write_str(StatusMessageKind::Error, &error);
            }
        }
        Ok(())
    }

    pub fn reload_all(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        let unsaved_buffers = ctx.buffers.iter().any(|b| b.needs_save());
        if unsaved_buffers {
            ctx.status_message.write_str(
                StatusMessageKind::Error,
                "there are unsaved changes in buffers. try 'force_reload_all' to force reload all",
            );
            Ok(())
        } else {
            let (buffers, line_pool) = ctx.buffers.iter_mut_with_line_pool();
            for buffer in buffers {
                if let Err(error) = buffer.discard_and_reload_from_file(line_pool) {
                    ctx.status_message
                        .write_str(StatusMessageKind::Error, &error);
                }
            }
            Ok(())
        }
    }

    pub fn force_reload_all(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        let (buffers, line_pool) = ctx.buffers.iter_mut_with_line_pool();
        for buffer in buffers {
            if let Err(error) = buffer.discard_and_reload_from_file(line_pool) {
                ctx.status_message
                    .write_str(StatusMessageKind::Error, &error);
            }
        }
        Ok(())
    }

    pub fn commit_edits(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<()> {
        let buffer_handle = handle.or_else(|| {
            ctx.current_buffer_view_handle()
                .and_then(|h| ctx.buffer_views.get(h))
                .map(|v| v.buffer_handle)
        });
        if let Some(buffer) = buffer_handle.and_then(|h| ctx.buffers.get_mut(h)) {
            buffer.commit_edits();
        }
        Ok(())
    }

    pub fn on_open(
        engine: ScriptEngineRef,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        callback: ScriptFunction,
    ) -> ScriptResult<()> {
        engine.add_to_function_array_in_registry("buffer_on_open", callback)
    }
}

mod buffer_view {
    use super::*;

    pub fn buffer_handle(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: BufferViewHandle,
    ) -> ScriptResult<Option<BufferHandle>> {
        Ok(ctx.buffer_views.get(handle).map(|v| v.buffer_handle))
    }

    pub fn all_handles<'a>(
        engine: ScriptEngineRef<'a>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'a>> {
        let handles = engine.create_array()?;
        for (h, _) in ctx.buffer_views.iter_with_handles() {
            handles.push(h)?;
        }
        Ok(ScriptValue::Array(handles))
    }

    pub fn handle_from_path(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        path: ScriptString,
    ) -> ScriptResult<Option<BufferViewHandle>> {
        let path = path.to_str()?;
        match ctx.buffer_views.buffer_view_handle_from_path(
            ctx.buffers,
            ctx.word_database,
            &ctx.config.syntaxes,
            ctx.target_client,
            Path::new(path),
            None,
            ctx.events,
        ) {
            Ok(handle) => Ok(Some(handle)),
            Err(error) => {
                ctx.status_message
                    .write_str(StatusMessageKind::Error, &error);
                Ok(None)
            }
        }
    }

    pub fn selection_text(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<String> {
        let mut text = String::new();
        if let Some(view) = handle
            .or_else(|| ctx.current_buffer_view_handle())
            .and_then(|h| ctx.buffer_views.get(h))
        {
            view.get_selection_text(ctx.buffers, &mut text);
        }

        Ok(text)
    }

    pub fn insert_text(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (text, handle): (ScriptString, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        if let Some(handle) = handle.or_else(|| ctx.current_buffer_view_handle()) {
            let text = text.to_str()?;
            ctx.buffer_views.insert_text_at_cursor_positions(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
                text,
            );
        }
        Ok(())
    }

    pub fn insert_text_at(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (text, line, column, handle): (ScriptString, usize, usize, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        if let Some(handle) = handle.or_else(|| ctx.current_buffer_view_handle()) {
            let text = text.to_str()?;
            ctx.buffer_views.insert_text_at_position(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
                BufferPosition::line_col(line, column),
                text,
                0,
            );
        }
        Ok(())
    }

    pub fn delete_selection(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<()> {
        if let Some(handle) = handle.or_else(|| ctx.current_buffer_view_handle()) {
            ctx.buffer_views.delete_in_cursor_ranges(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
            );
        }
        Ok(())
    }

    pub fn delete_in(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (from_line, from_column, to_line, to_column, handle): (
            usize,
            usize,
            usize,
            usize,
            Option<BufferViewHandle>,
        ),
    ) -> ScriptResult<()> {
        if let Some(handle) = handle.or_else(|| ctx.current_buffer_view_handle()) {
            ctx.buffer_views.delete_in_range(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
                BufferRange::between(
                    BufferPosition::line_col(from_line, from_column),
                    BufferPosition::line_col(to_line, to_column),
                ),
                0,
            );
        }
        Ok(())
    }

    pub fn undo(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<()> {
        if let Some(handle) = handle.or_else(|| ctx.current_buffer_view_handle()) {
            ctx.buffer_views
                .undo(ctx.buffers, &ctx.config.syntaxes, handle);
        }
        Ok(())
    }

    pub fn redo(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<()> {
        if let Some(handle) = handle.or_else(|| ctx.current_buffer_view_handle()) {
            ctx.buffer_views
                .redo(ctx.buffers, &ctx.config.syntaxes, handle);
        }
        Ok(())
    }
}

mod cursors {
    use super::*;

    pub fn len<'a>(
        _: ScriptEngineRef<'a>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<Option<usize>> {
        Ok(handle
            .or_else(|| ctx.current_buffer_view_handle())
            .and_then(|h| ctx.buffer_views.get(h))
            .map(|v| v.cursors[..].len()))
    }

    pub fn all<'a>(
        engine: ScriptEngineRef<'a>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<ScriptValue<'a>> {
        let script_cursors = engine.create_array()?;
        if let Some(cursors) = handle
            .or_else(|| ctx.current_buffer_view_handle())
            .and_then(|h| ctx.buffer_views.get(h))
            .map(|v| &v.cursors)
        {
            for cursor in &cursors[..] {
                script_cursors.push(*cursor)?;
            }
        }
        Ok(ScriptValue::Array(script_cursors))
    }

    pub fn set_all(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (script_cursors, handle): (ScriptArray, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        if let Some(cursors) = handle
            .or_else(|| ctx.current_buffer_view_handle())
            .and_then(|h| ctx.buffer_views.get_mut(h))
            .map(|v| &mut v.cursors)
        {
            let mut cursors = cursors.mut_guard();
            cursors.clear();
            for cursor in script_cursors.iter() {
                cursors.add(cursor?);
            }
        }
        Ok(())
    }

    pub fn main_index(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<Option<usize>> {
        Ok(handle
            .or_else(|| ctx.current_buffer_view_handle())
            .and_then(|h| ctx.buffer_views.get_mut(h))
            .map(|v| v.cursors.main_cursor_index()))
    }

    pub fn main(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<Option<Cursor>> {
        Ok(handle
            .or_else(|| ctx.current_buffer_view_handle())
            .and_then(|h| ctx.buffer_views.get_mut(h))
            .map(|v| *v.cursors.main_cursor()))
    }

    pub fn set(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (index, cursor, handle): (usize, Cursor, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        if let Some(cursors) = handle
            .or_else(|| ctx.current_buffer_view_handle())
            .and_then(|h| ctx.buffer_views.get_mut(h))
            .map(|v| &mut v.cursors)
        {
            let mut cursors = cursors.mut_guard();
            if index < cursors[..].len() {
                cursors[index] = cursor;
            }
        }
        Ok(())
    }

    pub fn move_columns(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (count, selecting, handle): (isize, bool, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        let movement = if count > 0 {
            CursorMovement::ColumnsForward(count as _)
        } else {
            CursorMovement::ColumnsBackward(-count as _)
        };
        move_cursors(ctx, movement, selecting, handle);
        Ok(())
    }

    pub fn move_lines(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (count, selecting, handle): (isize, bool, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        let movement = if count > 0 {
            CursorMovement::LinesForward(count as _)
        } else {
            CursorMovement::LinesBackward(-count as _)
        };
        move_cursors(ctx, movement, selecting, handle);
        Ok(())
    }

    pub fn move_words(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (count, selecting, handle): (isize, bool, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        let movement = if count > 0 {
            CursorMovement::WordsForward(count as _)
        } else {
            CursorMovement::WordsBackward(-count as _)
        };
        move_cursors(ctx, movement, selecting, handle);
        Ok(())
    }

    pub fn move_home(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (selecting, handle): (bool, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        move_cursors(ctx, CursorMovement::Home, selecting, handle);
        Ok(())
    }

    pub fn move_end(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (selecting, handle): (bool, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        move_cursors(ctx, CursorMovement::End, selecting, handle);
        Ok(())
    }

    pub fn move_first_line(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (selecting, handle): (bool, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        move_cursors(ctx, CursorMovement::FirstLine, selecting, handle);
        Ok(())
    }

    pub fn move_last_line(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (selecting, handle): (bool, Option<BufferViewHandle>),
    ) -> ScriptResult<()> {
        move_cursors(ctx, CursorMovement::LastLine, selecting, handle);
        Ok(())
    }

    fn move_cursors(
        ctx: &mut ScriptContext,
        movement: CursorMovement,
        selecting: bool,
        handle: Option<BufferViewHandle>,
    ) {
        let handle = match handle.or_else(|| ctx.current_buffer_view_handle()) {
            Some(handle) => handle,
            None => return,
        };
        let view = match ctx.buffer_views.get_mut(handle) {
            Some(view) => view,
            None => return,
        };

        let kind = match selecting {
            false => CursorMovementKind::PositionAndAnchor,
            true => CursorMovementKind::PositionOnly,
        };
        view.move_cursors(ctx.buffers, movement, kind);
    }
}

mod read_line {
    use super::*;

    pub fn prompt(
        engine: ScriptEngineRef,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        prompt: ScriptString,
    ) -> ScriptResult<()> {
        mode::read_line::custom::prompt(engine, prompt)
    }

    pub fn read(
        engine: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        callback: ScriptFunction,
    ) -> ScriptResult<()> {
        ctx.next_mode = mode::read_line::custom::mode(engine, callback)?;
        Ok(())
    }
}

mod picker {
    use super::*;

    pub fn prompt(
        engine: ScriptEngineRef,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        prompt: ScriptString,
    ) -> ScriptResult<()> {
        mode::picker::custom::prompt(engine, prompt)
    }

    pub fn reset(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        ctx.picker.reset();
        Ok(())
    }

    pub fn entry(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (name, description): (ScriptString, Option<ScriptString>),
    ) -> ScriptResult<()> {
        let description = match description {
            Some(ref d) => d.to_str()?,
            None => "",
        };
        ctx.picker.add_custom_entry(name.to_str()?, description);
        Ok(())
    }

    pub fn pick(
        engine: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        callback: ScriptFunction,
    ) -> ScriptResult<()> {
        ctx.next_mode = mode::picker::custom::mode(engine, callback)?;
        Ok(())
    }
}

mod process {
    use super::*;

    pub fn pipe(
        _: ScriptEngineRef,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        (name, args, input): (ScriptString, Option<ScriptArray>, Option<ScriptString>),
    ) -> ScriptResult<String> {
        let child = match args {
            Some(args) => {
                let args = args.iter().filter_map(|i| match i {
                    Ok(i) => Some(i),
                    Err(_) => None,
                });
                run_process(name, args, input, Stdio::piped())?
            }
            None => run_process(name, std::iter::empty(), input, Stdio::piped())?,
        };

        let child_output = child.wait_with_output().map_err(ScriptError::from)?;
        if child_output.status.success() {
            let child_output = String::from_utf8_lossy(&child_output.stdout);
            Ok(child_output.into_owned())
        } else {
            let child_output = String::from_utf8_lossy(&child_output.stdout);
            Err(ScriptError::from(child_output.into_owned()))
        }
    }

    pub fn spawn(
        _: ScriptEngineRef,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        (name, args, input): (ScriptString, Option<ScriptArray>, Option<ScriptString>),
    ) -> ScriptResult<()> {
        match args {
            Some(args) => {
                let args = args.iter().filter_map(|i| match i {
                    Ok(i) => Some(i),
                    Err(_) => None,
                });
                run_process(name, args, input, Stdio::null())?;
            }
            None => {
                run_process(name, std::iter::empty(), input, Stdio::null())?;
            }
        }
        Ok(())
    }

    fn run_process<'a, I>(
        name: ScriptString,
        args: I,
        input: Option<ScriptString>,
        output: Stdio,
    ) -> ScriptResult<Child>
    where
        I: Iterator<Item = ScriptString<'a>>,
    {
        let mut command = Command::new(name.to_str()?);
        command.stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        command.stdout(output);
        command.stderr(Stdio::piped());
        for arg in args {
            command.arg(arg.to_str()?);
        }

        let mut child = command.spawn().map_err(ScriptError::from)?;
        if let Some(stdin) = child.stdin.as_mut() {
            let bytes = match input {
                Some(ref input) => input.as_bytes(),
                None => &[],
            };
            let _ = stdin.write_all(bytes);
        }
        child.stdin = None;
        Ok(child)
    }
}

mod config {
    use super::*;

    pub fn index<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (_, index): (ScriptObject, ScriptString),
    ) -> ScriptResult<ScriptValue<'script>> {
        macro_rules! char_to_string {
            ($c:expr) => {{
                let mut buf = [0; std::mem::size_of::<char>()];
                ScriptValue::String(engine.create_string($c.encode_utf8(&mut buf).as_bytes())?)
            }};
        }

        let config = &ctx.config.values;
        let index = index.to_str()?;
        match index {
            "tab_size" => Ok(ScriptValue::Integer(config.tab_size.get() as _)),
            "indent_with_tabs" => Ok(ScriptValue::Boolean(config.indent_with_tabs)),
            "visual_empty" => Ok(char_to_string!(config.visual_empty)),
            "visual_space" => Ok(char_to_string!(config.visual_space)),
            "visual_tab_first" => Ok(char_to_string!(config.visual_tab_first)),
            "visual_tab_repeat" => Ok(char_to_string!(config.visual_tab_repeat)),
            "picker_max_height" => Ok(ScriptValue::Integer(config.picker_max_height.get() as _)),
            _ => Err(ScriptError::from(format!("no such property {}", index))),
        }
    }

    pub fn newindex(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (_, index, value): (ScriptObject, ScriptString, ScriptValue),
    ) -> ScriptResult<()> {
        macro_rules! try_bool {
            ($value:expr) => {{
                match $value {
                    ScriptValue::Boolean(b) => b,
                    _ => return Err(ScriptError::<bool>::convert_from_script(&$value)),
                }
            }};
        }
        macro_rules! try_non_zero_u8 {
            ($value:expr) => {{
                let integer = match $value {
                    ScriptValue::Integer(i) if i > 0 => i,
                    _ => {
                        return Err(ScriptError::<NonZeroU8>::convert_from_script(&$value));
                    }
                };
                NonZeroU8::new(integer as _).unwrap()
            }};
        }
        macro_rules! try_char {
            ($value:expr) => {{
                match $value {
                    ScriptValue::String(s) => {
                        s.to_str()?.parse().map_err(|e| ScriptError::from(e))?
                    }
                    _ => return Err(ScriptError::<char>::convert_from_script(&$value)),
                }
            }};
        }

        let config = &mut ctx.config.values;
        let index = index.to_str()?;
        match index {
            "tab_size" => config.tab_size = try_non_zero_u8!(value),
            "indent_with_tabs" => config.indent_with_tabs = try_bool!(value),
            "visual_empty" => config.visual_empty = try_char!(value),
            "visual_space" => config.visual_space = try_char!(value),
            "visual_tab_first" => config.visual_tab_first = try_char!(value),
            "visual_tab_repeat" => config.visual_tab_repeat = try_char!(value),
            "picker_max_height" => config.picker_max_height = try_non_zero_u8!(value),
            _ => return Err(ScriptError::from(format!("no such property {}", index))),
        }

        Ok(())
    }
}

mod keymap {
    use super::*;

    pub fn normal(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (from, to): (ScriptString, ScriptString),
    ) -> ScriptResult<()> {
        map_mode(ctx, Mode::Normal(Default::default()), from, to)
    }

    pub fn insert(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (from, to): (ScriptString, ScriptString),
    ) -> ScriptResult<()> {
        map_mode(ctx, Mode::Insert(Default::default()), from, to)
    }

    fn map_mode(
        ctx: &mut ScriptContext,
        mode: Mode,
        from: ScriptString,
        to: ScriptString,
    ) -> ScriptResult<()> {
        let from = from.to_str()?;
        let to = to.to_str()?;

        match ctx.keymaps.parse_and_map(mode.discriminant(), from, to) {
            Ok(()) => Ok(()),
            Err(ParseKeyMapError::From(e)) => {
                let message = helper::parsing_error(e.error, from, e.index);
                Err(ScriptError::from(message))
            }
            Err(ParseKeyMapError::To(e)) => {
                let message = helper::parsing_error(e.error, to, e.index);
                Err(ScriptError::from(message))
            }
        }
    }
}

mod syntax {
    use super::*;

    pub fn rules(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (extensions, rules): (ScriptArray, ScriptObject),
    ) -> ScriptResult<()> {
        fn try_add_rules(
            syntax: &mut Syntax,
            token_kind: TokenKind,
            token_kind_key: &str,
            rules: &ScriptObject,
        ) -> ScriptResult<()> {
            if let Ok(patterns) = rules.get::<ScriptArray>(token_kind_key) {
                for pattern in patterns.iter::<ScriptString>() {
                    let pattern = pattern?;
                    let pattern = pattern.to_str()?;
                    let pattern = Pattern::new(pattern).map_err(|e| {
                        let message = helper::parsing_error(e, pattern, 0);
                        ScriptError::from(message)
                    })?;
                    syntax.add_rule(token_kind, pattern);
                }
            }
            Ok(())
        }

        let mut syntax = Syntax::default();

        for extension in extensions.iter::<ScriptString>() {
            syntax.add_extension(extension?.to_str()?.into());
        }

        try_add_rules(&mut syntax, TokenKind::Keyword, "keyword", &rules)?;
        try_add_rules(&mut syntax, TokenKind::Symbol, "symbol", &rules)?;
        try_add_rules(&mut syntax, TokenKind::Type, "type", &rules)?;
        try_add_rules(&mut syntax, TokenKind::Literal, "literal", &rules)?;
        try_add_rules(&mut syntax, TokenKind::String, "string", &rules)?;
        try_add_rules(&mut syntax, TokenKind::Comment, "comment", &rules)?;
        try_add_rules(&mut syntax, TokenKind::Text, "text", &rules)?;

        ctx.config.syntaxes.add(syntax);

        for buffer in ctx.buffers.iter_mut() {
            buffer.refresh_syntax(&ctx.config.syntaxes);
        }

        Ok(())
    }
}

mod theme {
    use super::*;

    pub fn index<'script>(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (_, index): (ScriptObject, ScriptString),
    ) -> ScriptResult<ScriptValue<'script>> {
        let theme = &mut ctx.config.theme;
        let index = index.to_str()?;
        match theme.color_from_name(index) {
            Some(color) => Ok(ScriptValue::Integer(color.into_u32() as _)),
            None => Err(ScriptError::from(format!("no such property {}", index))),
        }
    }

    pub fn newindex(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (_, index, value): (ScriptObject, ScriptString, u32),
    ) -> ScriptResult<()> {
        let theme = &mut ctx.config.theme;
        let index = index.to_str()?;
        match theme.color_from_name(index) {
            Some(color) => *color = Color::from_u32(value),
            None => return Err(ScriptError::from(format!("no such property {}", index))),
        }
        Ok(())
    }
}

mod registers {
    use super::*;

    pub fn index<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (_, index): (ScriptObject, ScriptString),
    ) -> ScriptResult<ScriptValue<'script>> {
        let key = parse_register_key(index)?;
        let register = ctx.registers.get(key);
        let register = engine.create_string(register.as_bytes())?;
        Ok(ScriptValue::String(register))
    }

    pub fn newindex(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (_, index, value): (ScriptObject, ScriptString, ScriptString),
    ) -> ScriptResult<()> {
        let key = parse_register_key(index)?;
        let value = value.to_str()?;
        ctx.registers.set(key, value);
        Ok(())
    }

    fn parse_register_key(text: ScriptString) -> ScriptResult<RegisterKey> {
        let text = text.to_str()?;
        let bytes = text.as_bytes();
        if bytes.len() == 1 {
            if let Some(key) = RegisterKey::from_char(bytes[0] as _) {
                return Ok(key);
            }
        }
        Err(ScriptError::from(format!("no such property {}", text)))
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
}
