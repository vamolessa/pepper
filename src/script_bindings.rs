use std::{
    env, fmt,
    io::Write,
    num::NonZeroU8,
    path::Path,
    process::{Child, Command, Stdio},
};

use crate::{
    buffer::{BufferCapabilities, BufferHandle},
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    client::TargetClient,
    cursor::Cursor,
    editor::{EditorLoop, StatusMessageKind},
    glob::Glob,
    json::Json,
    keymap::ParseKeyMapError,
    lsp::{LspClient, LspClientContext, LspClientHandle},
    mode::{self, Mode},
    navigation_history::NavigationHistory,
    register::RegisterKey,
    script::{
        ScriptArray, ScriptCallback, ScriptContext, ScriptContextGuard, ScriptEngineRef,
        ScriptError, ScriptFunction, ScriptObject, ScriptResult, ScriptString, ScriptUserData,
        ScriptValue,
    },
    syntax::{Syntax, TokenKind},
    task::{TaskHandle, TaskRequest},
    theme::{Color, THEME_COLOR_NAMES},
};

#[derive(Default)]
pub struct EditorScriptCallbacks {
    pub on_idle: Vec<ScriptCallback>,
}

#[derive(Default)]
pub struct BufferScriptCallbacks {
    pub on_load: Vec<ScriptCallback>,
    pub on_open: Vec<ScriptCallback>,
    pub on_save: Vec<ScriptCallback>,
    pub on_close: Vec<ScriptCallback>,
}

#[derive(Default)]
pub struct ScriptCallbacks {
    pub read_line: Option<ScriptCallback>,
    pub picker: Option<ScriptCallback>,
    pub task: Vec<(TaskHandle, ScriptCallback)>,

    pub editor: EditorScriptCallbacks,
    pub buffer: BufferScriptCallbacks,
}

pub struct QuitError;
impl fmt::Display for QuitError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("could not quit now")
    }
}

pub fn bind_all(scripts: ScriptEngineRef) -> ScriptResult<()> {
    let globals = scripts.globals_object();

    macro_rules! register {
        ($namespace:ident) => {
            let func = scripts.create_ctx_function($namespace::$namespace)?;
            globals.set(stringify!($namespace), ScriptValue::Function(func))?;
        };
        ($namespace:ident => $($func:ident,)*) => {
            $(
                let func = scripts.create_ctx_function($namespace::$func)?;
                let name = concat!(stringify!($namespace), "_", stringify!($func));
                globals.set(name, ScriptValue::Function(func))?;
            )*
        };
    }

    macro_rules! register_callbacks {
        ($namespace:ident => $($callback:ident,)*) => {
            $(
                let name = concat!(stringify!($namespace), "_", stringify!($callback));
                let callback = scripts.create_ctx_function(move |engine, ctx, _, callback| {
                    let callback : ScriptFunction = callback;
                    let callback = engine.create_callback(callback)?;
                    ctx.script_callbacks.$namespace.$callback.push(callback);
                    Ok(())
                })?;
                globals.set(name, ScriptValue::Function(callback))?;
            )*
        };
    }

    register!(client => index, current_buffer_view_handle, quit, force_quit, quit_all, force_quit_all,);
    register!(editor => version, os, current_directory, print, eprint,);
    register!(script => source, directory,);
    register!(lsp => all_handles, start, stop, hover, signature_help, open_log,);
    register!(buffer => all_handles, line_count, line_at, path, path_matches, needs_save, set_search, open, close,
        force_close, close_all, force_close_all, save, save_all, reload, force_reload, reload_all, force_reload_all,
        commit_edits,);
    register!(buffer_view => buffer_handle, all_handles, handle_from_path, selection_text, insert_text, insert_text_at,
        delete_selection, delete_in, undo, redo,);
    register!(cursors => len, all, set_all, main_index, main, get, set, move_columns, move_lines, move_words,
        move_home, move_end, move_first_line, move_last_line,);
    register!(read_line => prompt, read,);
    register!(picker => reset, entry, pick,);
    register!(process => pipe, spawn,);
    register!(keymap => normal, insert, read_line, picker, script,);
    register!(syntax => rules,);
    register!(glob => compile, matches,);

    register!(config);
    register!(theme);
    register!(registers);

    register_callbacks!(editor => on_idle,);
    register_callbacks!(buffer => on_load, on_open, on_save, on_close,);

    {
        globals.set("print", globals.get::<ScriptValue>("editor_print")?)?;

        globals.set("q", globals.get::<ScriptValue>("client_quit")?)?;
        globals.set("fq", globals.get::<ScriptValue>("client_force_quit")?)?;
        globals.set("qa", globals.get::<ScriptValue>("client_quit_all")?)?;
        globals.set("fqa", globals.get::<ScriptValue>("client_force_quit_all")?)?;

        globals.set("o", globals.get::<ScriptValue>("buffer_open")?)?;
        globals.set("c", globals.get::<ScriptValue>("buffer_close")?)?;
        globals.set("fc", globals.get::<ScriptValue>("buffer_force_close")?)?;
        globals.set("ca", globals.get::<ScriptValue>("buffer_close_all")?)?;
        globals.set("fca", globals.get::<ScriptValue>("buffer_force_close_all")?)?;
        globals.set("s", globals.get::<ScriptValue>("buffer_save")?)?;
        globals.set("sa", globals.get::<ScriptValue>("buffer_save_all")?)?;
        globals.set("r", globals.get::<ScriptValue>("buffer_reload")?)?;
        globals.set("fr", globals.get::<ScriptValue>("buffer_force_reload")?)?;
        globals.set("ra", globals.get::<ScriptValue>("buffer_reload_all")?)?;
        globals.set(
            "fra",
            globals.get::<ScriptValue>("buffer_force_reload_all")?,
        )?;
    }

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
                "there are unsaved changes in buffers. try 'client.force_quit' to force quit",
            );
            Ok(())
        }
    }

    pub fn force_quit(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        ctx.editor_loop = EditorLoop::Quit;
        Err(ScriptError::from(QuitError))
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
                "there are unsaved changes in buffers. try 'client.force_quit_all' to force quit all",
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

    pub fn version<'script>(
        engine: ScriptEngineRef<'script>,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'script>> {
        engine
            .create_string(env!("CARGO_PKG_VERSION").as_bytes())
            .map(ScriptValue::String)
    }

    pub fn os<'script>(
        engine: ScriptEngineRef<'script>,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'script>> {
        engine
            .create_string(env::consts::OS.as_bytes())
            .map(ScriptValue::String)
    }

    pub fn current_directory<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'script>> {
        match ctx.current_directory.to_str() {
            Some(path) => engine
                .create_string(path.as_bytes())
                .map(ScriptValue::String),
            None => Ok(ScriptValue::Nil),
        }
    }

    pub fn print(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        guard: ScriptContextGuard,
        value: ScriptValue,
    ) -> ScriptResult<()> {
        ctx.status_message.write_fmt(
            StatusMessageKind::Info,
            format_args!("{}", value.display(&guard)),
        );
        Ok(())
    }

    pub fn eprint(
        _: ScriptEngineRef,
        _: &mut ScriptContext,
        guard: ScriptContextGuard,
        value: ScriptValue,
    ) -> ScriptResult<()> {
        eprintln!("{}", value.display(&guard));
        Ok(())
    }
}

mod script {
    use super::*;

    pub fn source<'script>(
        engine: ScriptEngineRef<'script>,
        _: &mut ScriptContext,
        guard: ScriptContextGuard,
        path: ScriptString,
    ) -> ScriptResult<ScriptValue<'script>> {
        let path = Path::new(path.to_str()?);
        engine.source(&guard, path)
    }

    pub fn directory<'script>(
        engine: ScriptEngineRef<'script>,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'script>> {
        match engine.current_source_directory()?.and_then(|p| p.to_str()) {
            Some(directory) => engine
                .create_string(directory.as_bytes())
                .map(ScriptValue::String),
            None => Ok(ScriptValue::Nil),
        }
    }
}

mod lsp {
    use super::*;

    pub fn all_handles<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'script>> {
        let handles = engine.create_array()?;
        for (handle, _) in ctx.lsp.client_with_handles() {
            handles.push(handle)?;
        }
        Ok(ScriptValue::Array(handles))
    }

    pub fn start(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (command, args, root): (ScriptString, Option<ScriptArray>, Option<ScriptString>),
    ) -> ScriptResult<LspClientHandle> {
        let command = command.to_str()?;
        let mut command = Command::new(command);
        if let Some(args) = args {
            for arg in args.iter() {
                let arg: ScriptString = arg?;
                command.arg(arg.to_str()?);
            }
        }

        let root = match root {
            Some(ref path) => Path::new(path.to_str()?),
            None => ctx.current_directory,
        };

        ctx.lsp.start(command, root).map_err(ScriptError::from)
    }

    pub fn stop(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: LspClientHandle,
    ) -> ScriptResult<()> {
        ctx.lsp.stop(handle);
        Ok(())
    }

    pub fn access_client<F, R, E>(
        ctx: &mut ScriptContext,
        client_handle: Option<LspClientHandle>,
        buffer_handle: Option<BufferHandle>,
        func: F,
    ) -> ScriptResult<Option<R>>
    where
        F: FnOnce(&mut LspClientContext, &mut LspClient, &mut Json) -> Result<R, E>,
        E: 'static + fmt::Display,
    {
        fn find_client_for_buffer(
            ctx: &ScriptContext,
            buffer_handle: Option<BufferHandle>,
        ) -> Option<LspClientHandle> {
            let buffer_handle = buffer_handle?;
            let buffer_path_bytes = ctx.buffers.get(buffer_handle)?.path()?.to_str()?.as_bytes();
            let (client_handle, _) = ctx
                .lsp
                .client_with_handles()
                .find(|(_, c)| c.handles_path(buffer_path_bytes))?;
            Some(client_handle)
        }

        let client_handle =
            match client_handle.or_else(|| find_client_for_buffer(ctx, buffer_handle)) {
                Some(handle) => handle,
                None => {
                    ctx.status_message
                        .write_str(StatusMessageKind::Error, "lsp server not running");
                    return Ok(None);
                }
            };
        let (lsp, mut ctx) = ctx.into_lsp_context();
        match lsp.access(client_handle, |client, json| func(&mut ctx, client, json)) {
            Some(Ok(value)) => Ok(Some(value)),
            Some(Err(error)) => Err(ScriptError::from(error)),
            None => {
                ctx.status_message
                    .write_str(StatusMessageKind::Error, "lsp server not running");
                Ok(None)
            }
        }
    }

    fn get_current_position(
        ctx: &ScriptContext,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Option<BufferPosition> {
        match (line, column) {
            (Some(line), Some(column)) => Some(BufferPosition::line_col(line, column)),
            _ => match ctx
                .current_buffer_view_handle()
                .and_then(|h| ctx.buffer_views.get(h))
            {
                Some(buffer_view) => Some(buffer_view.cursors.main_cursor().position),
                None => None,
            },
        }
    }

    pub fn hover(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (client_handle, line, column, buffer_handle): (
            Option<LspClientHandle>,
            Option<usize>,
            Option<usize>,
            Option<BufferHandle>,
        ),
    ) -> ScriptResult<()> {
        let buffer_handle = match buffer_handle.or_else(|| ctx.current_buffer_handle()) {
            Some(handle) => handle,
            None => return Ok(()),
        };
        let position = match get_current_position(ctx, line, column) {
            Some(position) => position,
            None => return Ok(()),
        };
        access_client(
            ctx,
            client_handle,
            Some(buffer_handle),
            |ctx, client, json| client.hover(ctx, json, buffer_handle, position),
        )
        .map(|r| r.unwrap_or(()))
    }

    pub fn signature_help(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (client_handle, line, column, buffer_handle): (
            Option<LspClientHandle>,
            Option<usize>,
            Option<usize>,
            Option<BufferHandle>,
        ),
    ) -> ScriptResult<()> {
        let buffer_handle = match buffer_handle.or_else(|| ctx.current_buffer_handle()) {
            Some(handle) => handle,
            None => return Ok(()),
        };
        let position = match get_current_position(ctx, line, column) {
            Some(position) => position,
            None => return Ok(()),
        };
        access_client(
            ctx,
            client_handle,
            Some(buffer_handle),
            |ctx, client, json| client.signature_help(ctx, json, buffer_handle, position),
        )
        .map(|r| r.unwrap_or(()))
    }

    pub fn open_log(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: LspClientHandle,
    ) -> ScriptResult<()> {
        let target_client = ctx.target_client;
        let buffers = &mut *ctx.buffers;
        let buffer_views = &mut *ctx.buffer_views;
        let mut view_handle = None;
        ctx.lsp.access(handle, |client, _| {
            let buffer = buffers.new(BufferCapabilities::log());
            let buffer_handle = buffer.handle();
            buffer.set_path(Some(Path::new("language-server-output")));
            client.set_log_buffer(Some(buffer_handle));
            view_handle = Some(
                buffer_views.buffer_view_handle_from_buffer_handle(target_client, buffer_handle),
            );
        });
        if let Some(view_handle) = view_handle {
            ctx.set_current_buffer_view_handle(Some(view_handle));
        }
        Ok(())
    }
}

mod buffer {
    use super::*;

    pub fn all_handles<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'script>> {
        let handles = engine.create_array()?;
        for buffer in ctx.buffers.iter() {
            handles.push(buffer.handle())?;
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

    pub fn line_at<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (index, handle): (usize, Option<BufferHandle>),
    ) -> ScriptResult<ScriptValue<'script>> {
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

    pub fn path<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferHandle>,
    ) -> ScriptResult<ScriptValue<'script>> {
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

    pub fn path_matches<'script>(
        _: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (glob, handle): (ScriptUserData<Glob>, Option<BufferHandle>),
    ) -> ScriptResult<bool> {
        let glob = glob.borrow()?;
        match handle
            .or_else(|| ctx.current_buffer_handle())
            .and_then(|h| ctx.buffers.get(h))
            .and_then(|b| b.path())
            .and_then(|p| p.to_str())
            .map(|p| p.as_bytes())
        {
            Some(bytes) => Ok(glob.matches(bytes)),
            None => Ok(false),
        }
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
                ctx.target_client,
                ctx.current_directory,
                path,
                line_number.map(|l| l.saturating_sub(1)),
                ctx.editor_events,
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
                    "there are unsaved changes in buffer. try 'buffer.force_close' to force close",
                );
                return Ok(());
            }

            if let Some(path) = ctx
                .buffers
                .get(handle)
                .and_then(|b| b.path())
                .and_then(|p| p.to_str())
            {
                ctx.status_message
                    .write_fmt(StatusMessageKind::Info, format_args!("closed '{}'", path));
            } else {
                ctx.status_message
                    .write_str(StatusMessageKind::Info, "closed buffer");
            }

            ctx.buffer_views
                .defer_remove_where(ctx.buffers, ctx.editor_events, |view| {
                    view.buffer_handle == handle
                });
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
            if let Some(path) = ctx
                .buffers
                .get(handle)
                .and_then(|b| b.path())
                .and_then(|p| p.to_str())
            {
                ctx.status_message
                    .write_fmt(StatusMessageKind::Info, format_args!("closed '{}'", path));
            } else {
                ctx.status_message
                    .write_str(StatusMessageKind::Info, "closed buffer");
            }

            ctx.buffer_views
                .defer_remove_where(ctx.buffers, ctx.editor_events, |view| {
                    view.buffer_handle == handle
                });
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
                "there are unsaved changes in buffers. try 'buffer.force_close_all' to force close all",
            );
            Ok(())
        } else {
            let buffer_count = ctx.buffers.iter().count();
            ctx.status_message.write_fmt(
                StatusMessageKind::Info,
                format_args!("{} buffers closed", buffer_count),
            );

            ctx.buffer_views
                .defer_remove_where(ctx.buffers, ctx.editor_events, |_| true);
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
        let buffer_count = ctx.buffers.iter().count();
        ctx.status_message.write_fmt(
            StatusMessageKind::Info,
            format_args!("{} buffers closed", buffer_count),
        );

        ctx.buffer_views
            .defer_remove_where(ctx.buffers, ctx.editor_events, |_| true);
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
        let current_buffer_handle = ctx.current_buffer_handle();
        let buffers = &mut *ctx.buffers;
        let buffer = match handle
            .or_else(|| current_buffer_handle)
            .and_then(|h| buffers.get_mut(h))
        {
            Some(buffer) => buffer,
            None => return Err(ScriptError::from("no buffer opened")),
        };
        let path = match path {
            Some(ref path) => Some(Path::new(path.to_str()?)),
            None => None,
        };

        if let Err(e) = buffer.save_to_file(path, ctx.editor_events) {
            return Err(ScriptError::from(e.display(buffer).to_string()));
        }

        let path = buffer.path().unwrap_or(Path::new(""));
        ctx.status_message.write_fmt(
            StatusMessageKind::Info,
            format_args!("saved to '{:?}'", path),
        );
        Ok(())
    }

    pub fn save_all(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<()> {
        let mut buffer_count = 0;
        for buffer in ctx.buffers.iter_mut() {
            if let Err(e) = buffer.save_to_file(None, ctx.editor_events) {
                return Err(ScriptError::from(e.display(buffer).to_string()));
            }

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
        let current_handle = ctx.current_buffer_handle();
        let buffers = &mut ctx.buffers;
        if let Some(buffer) = handle
            .or_else(|| current_handle)
            .and_then(|h| buffers.get_mut(h))
        {
            if buffer.needs_save() {
                ctx.status_message.write_str(
                    StatusMessageKind::Error,
                    "there are unsaved changes in buffer. try 'buffer.force_reload' to force reload",
                );
                return Ok(());
            }

            match buffer.discard_and_reload_from_file(ctx.word_database, ctx.editor_events) {
                Ok(()) => ctx
                    .status_message
                    .write_str(StatusMessageKind::Info, "reloaded"),
                Err(error) => ctx.status_message.write_fmt(
                    StatusMessageKind::Error,
                    format_args!("{}", error.display(buffer)),
                ),
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
        let current_handle = ctx.current_buffer_handle();
        let buffers = &mut ctx.buffers;
        if let Some(buffer) = handle
            .or_else(|| current_handle)
            .and_then(|h| buffers.get_mut(h))
        {
            match buffer.discard_and_reload_from_file(ctx.word_database, ctx.editor_events) {
                Ok(()) => ctx
                    .status_message
                    .write_str(StatusMessageKind::Info, "reloaded"),
                Err(error) => ctx.status_message.write_fmt(
                    StatusMessageKind::Error,
                    format_args!("{}", error.display(buffer)),
                ),
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
                "there are unsaved changes in buffers. try 'buffer.force_reload_all' to force reload all",
            );
            Ok(())
        } else {
            let mut had_error = false;
            let mut buffer_count = 0;
            for buffer in ctx.buffers.iter_mut() {
                if let Err(error) =
                    buffer.discard_and_reload_from_file(ctx.word_database, ctx.editor_events)
                {
                    had_error = true;
                    ctx.status_message.write_fmt(
                        StatusMessageKind::Error,
                        format_args!("{}", error.display(buffer)),
                    );
                }
                buffer_count += 1;
            }
            if !had_error {
                ctx.status_message.write_fmt(
                    StatusMessageKind::Info,
                    format_args!("{} buffers reloaded", buffer_count),
                );
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
        let mut had_error = false;
        let mut buffer_count = 0;
        for buffer in ctx.buffers.iter_mut() {
            if let Err(error) =
                buffer.discard_and_reload_from_file(ctx.word_database, ctx.editor_events)
            {
                had_error = true;
                ctx.status_message.write_fmt(
                    StatusMessageKind::Error,
                    format_args!("{}", error.display(buffer)),
                );
            }
            buffer_count += 1;
        }
        if !had_error {
            ctx.status_message.write_fmt(
                StatusMessageKind::Info,
                format_args!("{} buffers reloaded", buffer_count),
            );
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

    pub fn all_handles<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        _: (),
    ) -> ScriptResult<ScriptValue<'script>> {
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
            ctx.target_client,
            ctx.current_directory,
            Path::new(path),
            None,
            ctx.editor_events,
        ) {
            Ok(handle) => Ok(Some(handle)),
            Err(_) => Ok(None),
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
                handle,
                text,
                ctx.editor_events,
            );
            ctx.edited_buffers = true;
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
                handle,
                BufferPosition::line_col(line, column),
                text,
                ctx.editor_events,
            );
            ctx.edited_buffers = true;
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
                handle,
                ctx.editor_events,
            );
            ctx.edited_buffers = true;
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
                handle,
                BufferRange::between(
                    BufferPosition::line_col(from_line, from_column),
                    BufferPosition::line_col(to_line, to_column),
                ),
                ctx.editor_events,
            );
            ctx.edited_buffers = true;
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
                .undo(ctx.buffers, ctx.word_database, ctx.editor_events, handle);
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
                .redo(ctx.buffers, ctx.word_database, ctx.editor_events, handle);
        }
        Ok(())
    }
}

mod cursors {
    use super::*;

    pub fn len<'script>(
        _: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<Option<usize>> {
        Ok(handle
            .or_else(|| ctx.current_buffer_view_handle())
            .and_then(|h| ctx.buffer_views.get(h))
            .map(|v| v.cursors[..].len()))
    }

    pub fn all<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        handle: Option<BufferViewHandle>,
    ) -> ScriptResult<ScriptValue<'script>> {
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

    pub fn get(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (index, handle): (usize, Option<BufferViewHandle>),
    ) -> ScriptResult<Option<Cursor>> {
        if let Some(cursors) = handle
            .or_else(|| ctx.current_buffer_view_handle())
            .and_then(|h| ctx.buffer_views.get_mut(h))
            .map(|v| &mut v.cursors)
        {
            Ok(cursors[..].get(index).cloned())
        } else {
            Ok(None)
        }
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
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        prompt: ScriptString,
    ) -> ScriptResult<()> {
        ctx.read_line.set_prompt(prompt.to_str()?);
        Ok(())
    }

    pub fn read(
        engine: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        callback: ScriptFunction,
    ) -> ScriptResult<()> {
        let callback = engine.create_callback(callback)?;
        ctx.next_mode = mode::read_line::custom::mode(ctx, callback)?;
        Ok(())
    }
}

mod picker {
    use super::*;

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
        let callback = engine.create_callback(callback)?;
        ctx.next_mode = mode::picker::custom::mode(ctx, callback)?;
        Ok(())
    }
}

mod process {
    use super::*;

    pub fn pipe<'script>(
        engine: ScriptEngineRef<'script>,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        (name, args, input): (ScriptString, Option<ScriptArray>, Option<ScriptString>),
    ) -> ScriptResult<(ScriptValue<'script>, ScriptValue<'script>, bool)> {
        let child = match args {
            Some(args) => {
                let args = args.iter().filter_map(|i| match i {
                    Ok(i) => Some(i),
                    Err(_) => None,
                });
                run_process(name, args, input, Stdio::piped(), Stdio::piped())?
            }
            None => {
                let args = std::iter::empty();
                run_process(name, args, input, Stdio::piped(), Stdio::piped())?
            }
        };

        let output = child.wait_with_output().map_err(ScriptError::from)?;
        let stdout = engine
            .create_string(&output.stdout)
            .map(ScriptValue::String)?;
        let stderr = engine
            .create_string(&output.stderr)
            .map(ScriptValue::String)?;
        Ok((stdout, stderr, output.status.success()))
    }

    pub fn spawn(
        engine: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (name, args, input, callback): (
            ScriptString,
            Option<ScriptArray>,
            Option<ScriptString>,
            Option<ScriptFunction>,
        ),
    ) -> ScriptResult<()> {
        let stdout = match callback {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        };

        let child = match args {
            Some(args) => {
                let args = args.iter().filter_map(|i| match i {
                    Ok(i) => Some(i),
                    Err(_) => None,
                });
                run_process(name, args, input, stdout, Stdio::null())?
            }
            None => {
                let args = std::iter::empty();
                run_process(name, args, input, stdout, Stdio::null())?
            }
        };

        if let Some(callback) = callback {
            let task_handle = ctx
                .tasks
                .request(ctx.target_client, TaskRequest::ChildStream(child));
            engine.add_task_callback(ctx, task_handle, callback)?;
        }

        Ok(())
    }

    fn run_process<'script, I>(
        name: ScriptString,
        args: I,
        input: Option<ScriptString>,
        stdout: Stdio,
        stderr: Stdio,
    ) -> ScriptResult<Child>
    where
        I: Iterator<Item = ScriptString<'script>>,
    {
        let mut command = Command::new(name.to_str()?);
        command.stdin(match input {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        });
        command.stdout(stdout);
        command.stderr(stderr);
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

    pub fn read_line(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (from, to): (ScriptString, ScriptString),
    ) -> ScriptResult<()> {
        map_mode(ctx, Mode::ReadLine(Default::default()), from, to)
    }

    pub fn picker(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (from, to): (ScriptString, ScriptString),
    ) -> ScriptResult<()> {
        map_mode(ctx, Mode::Picker(Default::default()), from, to)
    }

    pub fn script(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (from, to): (ScriptString, ScriptString),
    ) -> ScriptResult<()> {
        map_mode(ctx, Mode::Script(Default::default()), from, to)
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
        (extensions, rules): (ScriptString, ScriptObject),
    ) -> ScriptResult<()> {
        fn try_add_rules(
            syntax: &mut Syntax,
            token_kind: TokenKind,
            token_kind_key: &str,
            rules: &ScriptObject,
        ) -> ScriptResult<()> {
            if let Ok(pattern) = rules.get::<ScriptString>(token_kind_key) {
                let pattern = pattern.to_str()?;
                syntax.set_rule(token_kind, pattern).map_err(|e| {
                    let message = helper::parsing_error(e, pattern, 0);
                    ScriptError::from(message)
                })?;
            }
            Ok(())
        }

        let mut syntax = Syntax::new();
        syntax.set_glob(extensions.as_bytes());
        try_add_rules(&mut syntax, TokenKind::Keyword, "keyword", &rules)?;
        try_add_rules(&mut syntax, TokenKind::Type, "type", &rules)?;
        try_add_rules(&mut syntax, TokenKind::Symbol, "symbol", &rules)?;
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

mod glob {
    use super::*;

    pub fn compile(
        _: ScriptEngineRef,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        pattern: ScriptString,
    ) -> ScriptResult<Glob> {
        let mut glob = Glob::default();
        glob.compile(pattern.as_bytes())
            .map_err(ScriptError::from)?;
        Ok(glob)
    }

    pub fn matches(
        _: ScriptEngineRef,
        _: &mut ScriptContext,
        _: ScriptContextGuard,
        (glob, path): (ScriptUserData<Glob>, ScriptString),
    ) -> ScriptResult<bool> {
        Ok(glob.borrow()?.matches(path.as_bytes()))
    }
}

mod config {
    use super::*;

    pub fn config<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (key, value): (Option<ScriptString>, ScriptValue),
    ) -> ScriptResult<ScriptValue<'script>> {
        match key {
            Some(key) => match value {
                ScriptValue::Nil => index(engine, ctx, key),
                _ => {
                    newindex(ctx, key, value)?;
                    Ok(ScriptValue::Nil)
                }
            },
            None => {
                let keys = [
                    "tab_size",
                    "indent_with_tabs",
                    "visual_empty",
                    "visual_space",
                    "visual_tab_first",
                    "visual_tab_repeat",
                    "picker_max_height",
                ];
                let array = engine.create_array()?;
                for key in keys.iter() {
                    let key = engine.create_string(key.as_bytes())?;
                    array.push(ScriptValue::String(key))?;
                }
                Ok(ScriptValue::Array(array))
            }
        }
    }

    fn index<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        index: ScriptString,
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
            _ => helper::no_such_property_error(index),
        }
    }

    fn newindex(
        ctx: &mut ScriptContext,
        index: ScriptString,
        value: ScriptValue,
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
            _ => return helper::no_such_property_error(index),
        }

        Ok(())
    }
}

mod theme {
    use super::*;

    pub fn theme<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (key, value): (Option<ScriptString>, Option<u32>),
    ) -> ScriptResult<ScriptValue<'script>> {
        match key {
            Some(key) => {
                let theme = &mut ctx.config.theme;
                let key = key.to_str()?;
                let color = match theme.color_from_name(key) {
                    Some(color) => color,
                    None => return helper::no_such_property_error(key),
                };
                match value {
                    Some(value) => {
                        *color = Color::from_u32(value);
                        Ok(ScriptValue::Nil)
                    }
                    None => Ok(ScriptValue::Integer(color.into_u32() as _)),
                }
            }
            None => {
                let array = engine.create_array()?;
                for key in THEME_COLOR_NAMES {
                    let key = engine.create_string(key.as_bytes())?;
                    array.push(ScriptValue::String(key))?;
                }
                Ok(ScriptValue::Array(array))
            }
        }
    }
}

mod registers {
    use super::*;

    pub fn registers<'script>(
        engine: ScriptEngineRef<'script>,
        ctx: &mut ScriptContext,
        _: ScriptContextGuard,
        (key, value): (Option<ScriptString>, Option<ScriptString>),
    ) -> ScriptResult<ScriptValue<'script>> {
        match key {
            Some(key) => {
                let key = key.to_str()?;
                let key = match key.as_bytes() {
                    [b] => match RegisterKey::from_char(*b as _) {
                        Some(key) => key,
                        None => return helper::no_such_property_error(key),
                    },
                    _ => return helper::no_such_property_error(key),
                };
                match value {
                    Some(value) => {
                        let value = value.to_str()?;
                        ctx.registers.set(key, value);
                        Ok(ScriptValue::Nil)
                    }
                    None => {
                        let register = ctx.registers.get(key);
                        let register = engine.create_string(register.as_bytes())?;
                        Ok(ScriptValue::String(register))
                    }
                }
            }
            None => {
                let array = engine.create_array()?;
                for key in b'a'..=b'z' {
                    let key = engine.create_string(std::slice::from_ref(&key))?;
                    array.push(ScriptValue::String(key))?;
                }
                Ok(ScriptValue::Array(array))
            }
        }
    }
}

mod helper {
    use super::*;

    pub fn no_such_property_error<T>(property_name: &str) -> ScriptResult<T> {
        Err(ScriptError::from(format!(
            "no such property '{}'",
            property_name
        )))
    }

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
