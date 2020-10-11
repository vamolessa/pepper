use std::{
    fmt,
    io::Write,
    num::NonZeroUsize,
    path::Path,
    process::{Child, Command, Stdio},
};

use crate::{
    client::TargetClient,
    editor::{EditorLoop, StatusMessageKind},
    keymap::ParseKeyMapError,
    mode::{self, Mode, ModeOperation},
    navigation_history::NavigationHistory,
    pattern::Pattern,
    picker::CustomPickerEntry,
    script::{
        ScriptContext, ScriptEngineRef, ScriptError, ScriptFunction, ScriptObject, ScriptResult,
        ScriptString, ScriptValue,
    },
    theme::Color,
    word_database::WordDatabase,
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

    register!(client => index,);
    register!(editor => quit, quit_all, force_quit_all, print,
        selection, delete_selection, insert_text,
    );
    register!(buffer => open, close, close_all, force_close, force_close_all, save, save_all,);
    register!(picker => reset, entry, pick,);
    register!(process => pipe, spawn,);
    register!(keymap => normal, insert,);
    register!(syntax => extension, rule,);

    {
        let globals = scripts.globals_object();

        let editor = globals.get::<ScriptObject>("editor")?;
        globals.set("print", editor.get::<ScriptValue>("print")?)?;
        globals.set("q", editor.get::<ScriptValue>("quit")?)?;
        globals.set("qa", editor.get::<ScriptValue>("quit_all")?)?;
        globals.set("fqa", editor.get::<ScriptValue>("force_quit_all")?)?;

        let buffer = globals.get::<ScriptObject>("buffer")?;
        globals.set("o", buffer.get::<ScriptValue>("open")?)?;
        globals.set("c", buffer.get::<ScriptValue>("close")?)?;
        globals.set("ca", buffer.get::<ScriptValue>("close_all")?)?;
        globals.set("fc", buffer.get::<ScriptValue>("force_close")?)?;
        globals.set("fca", buffer.get::<ScriptValue>("force_close_all")?)?;
    }

    register_object!(config);
    register_object!(theme);

    Ok(())
}

mod client {
    use super::*;

    pub fn index(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<usize> {
        Ok(ctx.target_client.into_index())
    }
}

mod editor {
    use super::*;

    pub fn quit(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<()> {
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

    pub fn quit_all(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<()> {
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

    pub fn force_quit_all(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<()> {
        ctx.editor_loop = EditorLoop::QuitAll;
        Err(ScriptError::from(QuitError))
    }

    pub fn print(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        value: ScriptValue,
    ) -> ScriptResult<()> {
        let message = value.to_string();
        ctx.status_message
            .write_str(StatusMessageKind::Info, &message);
        Ok(())
    }

    pub fn selection(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<String> {
        let mut selection = String::new();
        ctx.current_buffer_view_handle()
            .and_then(|h| ctx.buffer_views.get(h))
            .map(|v| v.get_selection_text(ctx.buffers, &mut selection));
        Ok(selection)
    }

    pub fn delete_selection(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        _: (),
    ) -> ScriptResult<()> {
        if let Some(handle) = ctx.current_buffer_view_handle() {
            ctx.buffer_views.delete_in_cursor_ranges(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
            );
        }
        Ok(())
    }

    pub fn insert_text(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        text: ScriptString,
    ) -> ScriptResult<()> {
        if let Some(handle) = ctx.current_buffer_view_handle() {
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
}

mod buffer {
    use super::*;

    pub fn open(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        path: ScriptString,
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
            )
            .map_err(ScriptError::from)?;
        ctx.set_current_buffer_view_handle(Some(buffer_view_handle));
        Ok(())
    }

    pub fn close(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<()> {
        if let Some(handle) = ctx
            .current_buffer_view_handle()
            .and_then(|h| ctx.buffer_views.get(h))
            .map(|v| v.buffer_handle)
        {
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

            ctx.buffer_views
                .remove_where(ctx.buffers, ctx.clients, ctx.word_database, |view| {
                    view.buffer_handle == handle
                });
        }

        ctx.set_current_buffer_view_handle(None);
        Ok(())
    }

    pub fn force_close(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<()> {
        if let Some(handle) = ctx
            .current_buffer_view_handle()
            .and_then(|h| ctx.buffer_views.get(h))
            .map(|v| v.buffer_handle)
        {
            ctx.buffer_views
                .remove_where(ctx.buffers, ctx.clients, ctx.word_database, |view| {
                    view.buffer_handle == handle
                });
        }

        ctx.set_current_buffer_view_handle(None);
        Ok(())
    }

    pub fn close_all(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<()> {
        let unsaved_buffers = ctx.buffers.iter().any(|b| b.needs_save());
        if unsaved_buffers {
            ctx.status_message.write_str(
                StatusMessageKind::Error,
                "there are unsaved changes in buffers. try 'force_close_all' to force close all",
            );
            Ok(())
        } else {
            ctx.buffer_views
                .remove_where(ctx.buffers, ctx.clients, ctx.word_database, |_| true);
            for c in ctx.clients.client_refs() {
                c.client.current_buffer_view_handle = None;
            }
            Ok(())
        }
    }

    pub fn force_close_all(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<()> {
        ctx.buffer_views
            .remove_where(ctx.buffers, ctx.clients, ctx.word_database, |_| true);
        for c in ctx.clients.client_refs() {
            c.client.current_buffer_view_handle = None;
        }
        Ok(())
    }

    pub fn save(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        path: Option<ScriptString>,
    ) -> ScriptResult<()> {
        let buffer_handle = match ctx
            .current_buffer_view_handle()
            .and_then(|h| ctx.buffer_views.get(h))
            .map(|v| v.buffer_handle)
        {
            Some(handle) => handle,
            None => return Err(ScriptError::from("no buffer opened")),
        };

        let buffer = match ctx.buffers.get_mut(buffer_handle) {
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

    pub fn save_all(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<()> {
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
}

mod picker {
    use super::*;

    pub fn reset(_: ScriptEngineRef, ctx: &mut ScriptContext, _: ()) -> ScriptResult<()> {
        ctx.picker.reset();
        Ok(())
    }

    pub fn entry(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        (name, description): (ScriptString, Option<ScriptString>),
    ) -> ScriptResult<()> {
        ctx.picker.add_custom_entry(CustomPickerEntry {
            name: name.to_str()?.into(),
            description: match description {
                Some(d) => d.to_str()?.into(),
                None => String::new(),
            },
        });
        Ok(())
    }

    pub fn pick(
        engine: ScriptEngineRef,
        ctx: &mut ScriptContext,
        callback: ScriptFunction,
    ) -> ScriptResult<()> {
        const PICKER_CALLBACK_REGISTRY_KEY: &str = "picker_callback";
        engine.save_to_registry(
            PICKER_CALLBACK_REGISTRY_KEY,
            ScriptValue::Function(callback),
        )?;

        ctx.next_mode = Mode::Picker(mode::picker::State {
            on_pick: |ctx| {
                let current_entry_name = ctx
                    .picker
                    .current_entry_name(WordDatabase::empty())
                    .map(|e| String::from(e));

                let (engine, _, mut ctx) = ctx.script_context();
                let engine = engine.as_ref();

                match engine
                    .take_from_registry::<ScriptFunction>(PICKER_CALLBACK_REGISTRY_KEY)
                    .and_then(|c| c.call(&mut ctx, current_entry_name))
                {
                    Ok(()) => (),
                    Err(error) => {
                        ctx.status_message.write_error(&error);
                    }
                }

                ModeOperation::None
            },
        });
        Ok(())
    }
}

mod process {
    use super::*;

    pub fn pipe(
        _: ScriptEngineRef,
        _: &mut ScriptContext,
        (name, args, input): (
            ScriptString,
            Option<Vec<ScriptString>>,
            Option<ScriptString>,
        ),
    ) -> ScriptResult<String> {
        let args = args.unwrap_or(Vec::new());
        let child = run_process(name, args, input, Stdio::piped())?;
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
        (name, args, input): (
            ScriptString,
            Option<Vec<ScriptString>>,
            Option<ScriptString>,
        ),
    ) -> ScriptResult<()> {
        let args = args.unwrap_or(Vec::new());
        run_process(name, args, input, Stdio::null())?;
        Ok(())
    }

    fn run_process(
        name: ScriptString,
        args: Vec<ScriptString>,
        input: Option<ScriptString>,
        output: Stdio,
    ) -> ScriptResult<Child> {
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
            let bytes = match input.as_ref() {
                Some(input) => input.as_bytes(),
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
        macro_rules! try_non_zero_usize {
            ($value:expr) => {{
                let integer = match $value {
                    ScriptValue::Integer(i) if i > 0 => i,
                    _ => {
                        return Err(ScriptError::<NonZeroUsize>::convert_from_script(&$value));
                    }
                };
                NonZeroUsize::new(integer as _).unwrap()
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
            "tab_size" => config.tab_size = try_non_zero_usize!(value),
            "indent_with_tabs" => config.indent_with_tabs = try_bool!(value),
            "visual_empty" => config.visual_empty = try_char!(value),
            "visual_space" => config.visual_space = try_char!(value),
            "visual_tab_first" => config.visual_tab_first = try_char!(value),
            "visual_tab_repeat" => config.visual_tab_repeat = try_char!(value),
            "picker_max_height" => config.picker_max_height = try_non_zero_usize!(value),
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
        (from, to): (ScriptString, ScriptString),
    ) -> ScriptResult<()> {
        map_mode(ctx, Mode::Normal(Default::default()), from, to)
    }

    pub fn insert(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
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

mod theme {
    use super::*;

    pub fn index<'script>(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
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

mod syntax {
    use super::*;

    pub fn extension(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        (main_extension, other_extension): (ScriptString, ScriptString),
    ) -> ScriptResult<()> {
        let main_extension = main_extension.to_str()?;
        let other_extension = other_extension.to_str()?;
        ctx.config
            .syntaxes
            .get_by_extension(main_extension)
            .add_extension(other_extension.into());
        Ok(())
    }

    pub fn rule(
        _: ScriptEngineRef,
        ctx: &mut ScriptContext,
        (main_extension, token_kind, pattern): (ScriptString, ScriptString, ScriptString),
    ) -> ScriptResult<()> {
        let main_extension = main_extension.to_str()?;
        let token_kind = token_kind.to_str()?;
        let pattern = pattern.to_str()?;

        let token_kind = token_kind.parse().map_err(ScriptError::from)?;
        let pattern = Pattern::new(pattern).map_err(|e| {
            let message = helper::parsing_error(e, pattern, 0);
            ScriptError::from(message)
        })?;

        ctx.config
            .syntaxes
            .get_by_extension(main_extension)
            .add_rule(token_kind, pattern);
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
}
