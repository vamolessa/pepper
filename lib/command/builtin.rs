use std::{fmt, path::Path, process::Command};

use crate::{
    buffer::{Buffer, BufferHandle},
    buffer_view::BufferViewError,
    client::ClientHandle,
    command::{
        BuiltinCommand, CommandArgs, CommandContext, CommandManager, CommandOperation,
        CompletionSource,
    },
    config::{ParseConfigError, CONFIG_NAMES},
    editor::{Editor, EditorOutputKind},
    keymap::ParseKeyMapError,
    mode::ModeKind,
    navigation_history::NavigationHistory,
    register::RegisterKey,
    syntax::{Syntax, TokenKind},
    theme::{Color, THEME_COLOR_NAMES},
};

const INVALID_BUFFER_HANDLE_ERROR: &str = "invalid buffer handle";
const NO_BUFFER_OPENED_ERROR: &str = "no buffer opened";
const UNSAVED_CHANGES_ERROR: &str =
    "there are unsaved changes in buffer. try appending a '!' to force execute";

fn parsing_error(
    ctx: &mut CommandContext,
    parsed: &str,
    message: &dyn fmt::Display,
    error_index: usize,
) {
    ctx.editor
        .output
        .write(EditorOutputKind::Error)
        .fmt(format_args!(
            "{}\n{:>index$} {}",
            parsed,
            message,
            index = error_index + 1
        ));
}

macro_rules! expect_no_bang {
    ($ctx:expr) => {
        if $ctx.bang {
            $ctx.editor
                .output
                .write(EditorOutputKind::Error)
                .str("command expects no bang");
            return None;
        }
    };
}

macro_rules! parse_values {
    ($ctx:expr $(, $name:ident)*) => {
        let mut values = $ctx.args.values().iter();
        $(let $name = values.next().map(|v| v.as_str($ctx.args));)*
            if values.next().is_some() {
                $ctx.editor.output.write(EditorOutputKind::Error).str("too many values passed to command");
                return None;
            }
        drop(values);
    }
}

macro_rules! require_value {
    ($ctx:expr, $name:ident) => {
        let $name = match $name {
            Some(value) => value,
            None => {
                $ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .str(concat!("value '", stringify!($name), "' is required"));
                return None;
            }
        };
    };
}

macro_rules! parse_switches {
    ($ctx:expr $(, $name:ident)*) => {
        $(let mut $name = false;)*
            for switch in $ctx.args.switches() {
                let switch = switch.as_str($ctx.args);
                match switch {
                    $(stringify!($name) => $name = true,)*
                        _ => {
                            $ctx.editor.output.write(EditorOutputKind::Error).fmt(format_args!(
                                    "invalid switch '{}'", switch
                                    ));
                            return None;
                        }
                }
            }
    }
}

macro_rules! parse_options {
    ($ctx:expr $(, $name:ident)*) => {
        $(let mut $name = None;)*
            for (key, value) in $ctx.args.options() {
                let key = key.as_str($ctx.args);
                match key {
                    $(stringify!($name) => $name = Some(value.as_str($ctx.args)),)*
                        _ => {
                            drop(value);
                            $ctx.editor.output.write(EditorOutputKind::Error).fmt(format_args!(
                                    "invalid option '{}'", key
                                    ));
                            return None;
                        }
                }
            }
    }
}

macro_rules! parse_arg {
    ($ctx:expr, $name:ident : $type:ty) => {
        match $name.parse::<$type>() {
            Ok(value) => value,
            Err(_) => {
                $ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .fmt(format_args!(
                        concat!(
                            "could not convert argument '",
                            stringify!($name),
                            "' value '{}' to {}"
                        ),
                        $name,
                        std::any::type_name::<$type>()
                    ));
                return None;
            }
        }
    };
}

pub const COMMANDS: &[BuiltinCommand] = &[
    BuiltinCommand {
        names: &["quit", "q"],
        help: "quits this client. append a '!' to force quit",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            if ctx.bang || !ctx.editor.buffers.iter().any(Buffer::needs_save) {
                Some(CommandOperation::Quit)
            } else {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .str(UNSAVED_CHANGES_ERROR);
                None
            }
        },
    },
    BuiltinCommand {
        names: &["quit-all", "qa"],
        help: "quits all clients. append a '!' to force quit all",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            if ctx.bang || !ctx.editor.buffers.iter().any(Buffer::needs_save) {
                Some(CommandOperation::QuitAll)
            } else {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .str(UNSAVED_CHANGES_ERROR);
                None
            }
        },
    },
    BuiltinCommand {
        names: &["print"],
        help: "prints a message to the status bar",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            let mut w = ctx.editor.output.write(EditorOutputKind::Info);
            for arg in ctx.args.values() {
                w.str(arg.as_str(ctx.args));
                w.str(" ");
            }
            None
        },
    },
    BuiltinCommand {
        names: &["source"],
        help: "load a source file and execute its commands",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            for path in ctx.args.values() {
                let path = path.as_str(ctx.args);
                if let Some(CommandOperation::Quit) | Some(CommandOperation::QuitAll) =
                    ctx.editor.load_config(ctx.clients, path)
                {
                    break;
                }
            }
            None
        },
    },
    BuiltinCommand {
        names: &["open", "o"],
        help: "open a buffer for editting",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            let client_handle = ctx.client_handle?;
            NavigationHistory::save_client_snapshot(
                ctx.clients,
                client_handle,
                &ctx.editor.buffer_views,
            );

            let mut had_error = false;
            let mut w = ctx.editor.output.write(EditorOutputKind::Info);

            for path in ctx.args.values() {
                let mut path = path.as_str(ctx.args);

                let mut line_index = None;
                if let Some(separator_index) = path.rfind(':') {
                    if let Ok(n) = path[(separator_index + 1)..].parse() {
                        let n: usize = n;
                        line_index = Some(n.saturating_sub(1));
                        path = &path[..separator_index];
                    }
                }

                match ctx.editor.buffer_views.buffer_view_handle_from_path(
                    client_handle,
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &ctx.editor.current_directory,
                    Path::new(path),
                    line_index,
                    &mut ctx.editor.events,
                ) {
                    Ok(handle) => {
                        ctx.clients
                            .get_mut(client_handle)?
                            .set_buffer_view_handle(Some(handle));
                        if !had_error {
                            w.fmt(format_args!("{}\n", handle));
                        }
                    }
                    Err(BufferViewError::InvalidPath) => {
                        if !had_error {
                            had_error = true;
                            w = ctx.editor.output.write(EditorOutputKind::Error);
                            w.fmt(format_args!("invalid path '{}'", path));
                        } else {
                            w.fmt(format_args!("\ninvalid path '{}'", path));
                        }
                    }
                }
            }

            None
        },
    },
    BuiltinCommand {
        names: &["save", "s"],
        help: "save buffer",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx, path);
            parse_switches!(ctx);
            parse_options!(ctx, handle);

            let handle = match handle {
                Some(handle) => parse_arg!(ctx, handle: BufferHandle),
                None => match ctx.current_buffer_view_handle() {
                    Some(handle) => ctx.editor.buffer_views.get(handle)?.buffer_handle,
                    None => {
                        ctx.editor
                            .output
                            .write(EditorOutputKind::Error)
                            .str(NO_BUFFER_OPENED_ERROR);
                        return None;
                    }
                },
            };
            let buffer = match ctx.editor.buffers.get_mut(handle) {
                Some(buffer) => buffer,
                None => {
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Error)
                        .str(INVALID_BUFFER_HANDLE_ERROR);
                    return None;
                }
            };

            let path = path.map(Path::new);
            if let Err(error) = buffer.save_to_file(path, &mut ctx.editor.events) {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .fmt(format_args!("{}", error.display(buffer)));
                return None;
            }

            let path = buffer.path().unwrap_or(Path::new(""));
            ctx.editor
                .output
                .write(EditorOutputKind::Info)
                .fmt(format_args!("saved to '{:?}'", path));

            None
        },
    },
    BuiltinCommand {
        names: &["save-all", "sa"],
        help: "save all buffers",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            let mut count = 0;
            let mut had_error = false;
            let mut write = ctx.editor.output.write(EditorOutputKind::Error);
            for buffer in ctx.editor.buffers.iter_mut() {
                if let Err(error) = buffer.save_to_file(None, &mut ctx.editor.events) {
                    if had_error {
                        write.str("\n");
                    }
                    write.fmt(format_args!("{}", error.display(buffer)));
                    had_error = true;
                }
                count += 1;
            }

            if had_error {
                None
            } else {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Info)
                    .fmt(format_args!("{} buffers saved", count));
                None
            }
        },
    },
    BuiltinCommand {
        names: &["reload", "r"],
        help: "reload buffer from file",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx, handle);

            let handle = match handle {
                Some(handle) => parse_arg!(ctx, handle: BufferHandle),
                None => match ctx
                    .current_buffer_view_handle()
                    .and_then(|h| ctx.editor.buffer_views.get(h))
                    .map(|v| v.buffer_handle)
                {
                    Some(handle) => handle,
                    None => {
                        ctx.editor
                            .output
                            .write(EditorOutputKind::Error)
                            .str(NO_BUFFER_OPENED_ERROR);
                        return None;
                    }
                },
            };
            let buffer = match ctx.editor.buffers.get_mut(handle) {
                Some(buffer) => buffer,
                None => {
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Error)
                        .str(INVALID_BUFFER_HANDLE_ERROR);
                    return None;
                }
            };

            if !ctx.bang && buffer.needs_save() {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .str(UNSAVED_CHANGES_ERROR);
                return None;
            }

            if let Err(error) = buffer
                .discard_and_reload_from_file(&mut ctx.editor.word_database, &mut ctx.editor.events)
            {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .fmt(format_args!("{}", error.display(buffer)));
                return None;
            }

            ctx.editor
                .output
                .write(EditorOutputKind::Info)
                .str("buffer reloaded");
            None
        },
    },
    BuiltinCommand {
        names: &["reload-all", "ra"],
        help: "reload all buffers from file",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            if !ctx.bang && ctx.editor.buffers.iter().any(Buffer::needs_save) {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .str(UNSAVED_CHANGES_ERROR);
                return None;
            }

            let mut count = 0;
            let mut had_error = false;
            let mut write = ctx.editor.output.write(EditorOutputKind::Error);
            for buffer in ctx.editor.buffers.iter_mut() {
                if let Err(error) = buffer.discard_and_reload_from_file(
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                ) {
                    if had_error {
                        write.str("\n");
                    }
                    write.fmt(format_args!("{}", error.display(buffer)));
                    had_error = true;
                }
                count += 1;
            }

            if had_error {
                None
            } else {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Info)
                    .fmt(format_args!("{} buffers closed", count));
                None
            }
        },
    },
    BuiltinCommand {
        names: &["close", "c"],
        help: "close buffer",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx, handle);

            let handle = match handle {
                Some(handle) => parse_arg!(ctx, handle: BufferHandle),
                None => match ctx
                    .current_buffer_view_handle()
                    .and_then(|h| ctx.editor.buffer_views.get(h))
                    .map(|v| v.buffer_handle)
                {
                    Some(handle) => handle,
                    None => {
                        ctx.editor
                            .output
                            .write(EditorOutputKind::Error)
                            .str(NO_BUFFER_OPENED_ERROR);
                        return None;
                    }
                },
            };
            let buffer = match ctx.editor.buffers.get(handle) {
                Some(buffer) => buffer,
                None => {
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Error)
                        .str(INVALID_BUFFER_HANDLE_ERROR);
                    return None;
                }
            };

            if !ctx.bang && buffer.needs_save() {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .str(UNSAVED_CHANGES_ERROR);
                return None;
            }

            ctx.editor.buffer_views.defer_remove_buffer_where(
                &mut ctx.editor.buffers,
                &mut ctx.editor.events,
                |view| view.buffer_handle == handle,
            );

            let clients = ctx.clients;
            let editor = ctx.editor;
            for client in clients.iter_mut() {
                let maybe_buffer_handle = client
                    .buffer_view_handle()
                    .and_then(|h| editor.buffer_views.get(h))
                    .map(|v| v.buffer_handle);
                if maybe_buffer_handle == Some(handle) {
                    client.set_buffer_view_handle(None);
                }
            }

            editor
                .output
                .write(EditorOutputKind::Info)
                .str("buffer closed");
            None
        },
    },
    BuiltinCommand {
        names: &["close-all", "ca"],
        help: "close all buffers",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            if !ctx.bang && ctx.editor.buffers.iter().any(Buffer::needs_save) {
                ctx.editor
                    .output
                    .write(EditorOutputKind::Error)
                    .str(UNSAVED_CHANGES_ERROR);
                return None;
            }

            let count = ctx.editor.buffers.iter().count();
            ctx.editor.buffer_views.defer_remove_buffer_where(
                &mut ctx.editor.buffers,
                &mut ctx.editor.events,
                |_| true,
            );

            for client in ctx.clients.iter_mut() {
                client.set_buffer_view_handle(None);
            }

            ctx.editor
                .output
                .write(EditorOutputKind::Info)
                .fmt(format_args!("{} buffers closed", count));
            None
        },
    },
    BuiltinCommand {
        names: &["config"],
        help: "change an editor config",
        completion_source: CompletionSource::Custom(CONFIG_NAMES),
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx, key, value);
            parse_switches!(ctx);
            parse_options!(ctx);

            require_value!(ctx, key);
            match value {
                Some(value) => match ctx.editor.config.parse_config(key, value) {
                    Ok(()) => (),
                    Err(ParseConfigError::NotFound) => ctx
                        .editor
                        .output
                        .write(EditorOutputKind::Error)
                        .fmt(format_args!("no such config '{}'", key)),
                    Err(ParseConfigError::InvalidValue) => ctx
                        .editor
                        .output
                        .write(EditorOutputKind::Error)
                        .fmt(format_args!(
                            "invalid value '{}' for config '{}'",
                            value, key
                        )),
                },
                None => match ctx.editor.config.display_config(key) {
                    Some(display) => ctx
                        .editor
                        .output
                        .write(EditorOutputKind::Info)
                        .fmt(format_args!("{}", display)),
                    None => ctx
                        .editor
                        .output
                        .write(EditorOutputKind::Error)
                        .fmt(format_args!("no such config '{}'", key)),
                },
            }

            None
        },
    },
    BuiltinCommand {
        names: &["theme"],
        help: "change editor theme color",
        completion_source: CompletionSource::Custom(THEME_COLOR_NAMES),
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx, key, value);
            parse_switches!(ctx);
            parse_options!(ctx);

            require_value!(ctx, key);
            let color = match ctx.editor.theme.color_from_name(key) {
                Some(color) => color,
                None => {
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Error)
                        .fmt(format_args!("no such theme color '{}'", key));
                    return None;
                }
            };
            match value {
                Some(value) => match u32::from_str_radix(value, 16) {
                    Ok(parsed) => *color = Color::from_u32(parsed),
                    Err(_) => ctx
                        .editor
                        .output
                        .write(EditorOutputKind::Error)
                        .fmt(format_args!(
                            "invalid value '{}' for color '{}'",
                            value, key
                        )),
                },
                None => ctx
                    .editor
                    .output
                    .write(EditorOutputKind::Info)
                    .fmt(format_args!("{:x}", color.into_u32())),
            }

            None
        },
    },
    BuiltinCommand {
        names: &["syntax"],
        help: "create a syntax definition with patterns for files that match a glob",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx, glob);
            parse_switches!(ctx);

            require_value!(ctx, glob);

            let mut syntax = Syntax::new();
            syntax.set_glob(glob.as_bytes());

            macro_rules! parse_syntax_rules {
                ($($rule:ident : $token_kind:expr),*) => {
                    parse_options!(ctx $(, $rule)*);
                    $(if let Some($rule) = $rule {
                        if let Err(error) = syntax.set_rule($token_kind, $rule) {
                            parsing_error(&mut ctx, $rule, &error, 0);
                            return None;
                        }
                    })*
                }
            }
            parse_syntax_rules! {
                keywords: TokenKind::Keyword,
                types: TokenKind::Type,
                symbols: TokenKind::Symbol,
                literals: TokenKind::Literal,
                strings: TokenKind::String,
                comments: TokenKind::Comment,
                texts: TokenKind::Text
            };

            ctx.editor.syntaxes.add(syntax);
            for buffer in ctx.editor.buffers.iter_mut() {
                buffer.refresh_syntax(&ctx.editor.syntaxes);
            }

            None
        },
    },
    BuiltinCommand {
        names: &["map"],
        help: "create a keyboard mapping for a mode",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx, mode, from, to);
            parse_switches!(ctx);
            parse_options!(ctx);

            require_value!(ctx, mode);
            require_value!(ctx, from);
            require_value!(ctx, to);

            let mode = match mode {
                "normal" => ModeKind::Normal,
                "insert" => ModeKind::Insert,
                "read-line" => ModeKind::ReadLine,
                "picker" => ModeKind::Picker,
                "command" => ModeKind::Command,
                _ => {
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Error)
                        .fmt(format_args!("invalid mode '{}'", mode));
                    return None;
                }
            };

            match ctx.editor.keymaps.parse_and_map(mode, from, to) {
                Ok(()) => (),
                Err(ParseKeyMapError::From(e)) => parsing_error(&mut ctx, from, &e.error, e.index),
                Err(ParseKeyMapError::To(e)) => parsing_error(&mut ctx, to, &e.error, e.index),
            }

            None
        },
    },
    BuiltinCommand {
        names: &["register"],
        help: "change an editor register",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx, key, value);
            parse_switches!(ctx);
            parse_options!(ctx);

            require_value!(ctx, key);
            let key = match RegisterKey::from_str(key) {
                Some(key) => key,
                None => {
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Error)
                        .fmt(format_args!("invalid register key '{}'", key));
                    return None;
                }
            };

            match value {
                Some(value) => ctx.editor.registers.set(key, value),
                None => ctx
                    .editor
                    .output
                    .write(EditorOutputKind::Info)
                    .str(ctx.editor.registers.get(key)),
            }

            None
        },
    },
    BuiltinCommand {
        names: &["lsp-start"],
        help: "start a lsp server",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx, root);

            let (command, args) = match ctx.args.values() {
                [command, args @ ..] => (command.as_str(ctx.args), args),
                _ => {
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Error)
                        .str("value 'command' is required");
                    return None;
                }
            };

            let mut command = Command::new(command);
            for arg in args {
                let arg = arg.as_str(ctx.args);
                command.arg(arg);
            }

            let root = match root {
                Some(root) => Path::new(root),
                None => ctx.editor.current_directory.as_path(),
            };

            match ctx.editor.lsp.start(command, root) {
                Ok(handle) => {
                    let _ = handle;
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Info)
                        .fmt(format_args!("{}", 87));
                }
                Err(error) => ctx
                    .editor
                    .output
                    .write(EditorOutputKind::Error)
                    .fmt(format_args!("{}", &error)),
            }

            None
        },
    },
];

// lsp:
// - lsp-start
// - lsp-stop
// - lsp-hover
// - lsp-signature-help
// - lsp-open-log
