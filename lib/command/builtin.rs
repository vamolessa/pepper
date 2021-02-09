use std::{fmt, path::Path};

use crate::{
    buffer::{Buffer, BufferHandle},
    client::TargetClient,
    command::{
        BuiltinCommand, CommandArgs, CommandContext, CommandManager, CommandOperation,
        CompletionSource,
    },
    config::{ParseConfigError, CONFIG_NAMES},
    editor::{Editor, StatusMessageKind},
    keymap::ParseKeyMapError,
    mode::ModeKind,
    navigation_history::NavigationHistory,
    register::RegisterKey,
    syntax::{Syntax, TokenKind},
    theme::{Color, THEME_COLOR_NAMES},
};

pub fn register_all(commands: &mut CommandManager) {
    const INVALID_BUFFER_HANDLE: &str = "invalid buffer handle";
    const NO_BUFFER_OPENED_ERROR: &str = "no buffer opened";

    fn unsaved_changes_error(ctx: &mut CommandContext, command_name: &str) -> Option<()> {
        ctx.output.write_fmt(format_args!(
            "there are unsaved changes in buffer. try appending a '!' to '{}' to force execute",
            command_name
        ));
        None
    }

    fn any_buffer_needs_save(editor: &Editor) -> bool {
        editor.buffers.iter().any(|b| b.needs_save())
    }

    fn parsing_error(
        ctx: &mut CommandContext,
        parsed: &str,
        message: &dyn fmt::Display,
        error_index: usize,
    ) -> Option<()> {
        ctx.output.write_fmt(format_args!(
            "{}\n{:>index$} {}",
            parsed,
            message,
            index = error_index + 1
        ));
        None
    }

    macro_rules! expect_no_bang {
        ($ctx:expr) => {
            if $ctx.bang {
                $ctx.output.write_str("expected no bang");
                return Err(CommandError);
            }
        };
    }

    macro_rules! expect_empty_values {
        ($ctx:expr) => {
            if !$ctx.args.values().is_empty() {
                $ctx.output.write_str("expected no argument values");
                return Err(CommandError);
            }
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
                        $ctx.output.write_fmt(format_args!("invalid switch '{}'", switch));
                        return Err(CommandError);
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
                        $ctx.output.write_fmt(format_args!("invalid option '{}'", key));
                        return Err(CommandError);
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
                    $ctx.output.write_fmt(format_args!(
                        concat!(
                            "could not convert argument '",
                            stringify!($name),
                            "' value '{}' to {}"
                        ),
                        $name,
                        std::any::type_name::<$type>()
                    ));
                    return Err(CommandError);
                }
            }
        };
    }

    commands.register_builtin(BuiltinCommand {
        name: "quit",
        alias: Some("q"),
        help: "quits this client. append a '!' to force quit",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_empty_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            if ctx.bang || !any_buffer_needs_save(ctx.editor) {
                Ok(Some(CommandOperation::Quit))
            } else {
                unsaved_changes_error(&mut ctx, "quit")
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "quit-all",
        alias: Some("qa"),
        help: "quits all clients. append a '!' to force quit all",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_empty_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            if ctx.bang || !any_buffer_needs_save(ctx.editor) {
                Ok(Some(CommandOperation::QuitAll))
            } else {
                unsaved_changes_error(&mut ctx, "quit-all")
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "print",
        alias: None,
        help: "prints a message to the status bar",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            let mut w = ctx.editor.status_bar.write(StatusMessageKind::Info);
            for arg in ctx.args.values() {
                w.str(arg.as_str(ctx.args));
                w.str(" ");
            }
            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "source",
        alias: None,
        help: "load a source file and execute its commands",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            for path in ctx.args.values() {
                let path = path.as_str(ctx.args);
                match ctx.editor.load_config(ctx.clients, path) {
                    Ok(None) => (),
                    Ok(Some(CommandOperation::Quit)) | Ok(Some(CommandOperation::QuitAll)) => break,
                    Err(error) => return Err(error),
                }
            }
            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "open",
        alias: Some("o"),
        help: "open a buffer for editting",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            let target_client = match ctx.client_index {
                Some(i) => TargetClient(i),
                None => return Ok(None),
            };
            NavigationHistory::save_client_snapshot(
                ctx.clients,
                target_client,
                &ctx.editor.buffer_views,
            );

            let mut last_buffer_view_handle = None;
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

                let handle = ctx
                    .editor
                    .buffer_views
                    .buffer_view_handle_from_path(
                        target_client,
                        &mut ctx.editor.buffers,
                        &mut ctx.editor.word_database,
                        &ctx.editor.current_directory,
                        Path::new(path),
                        line_index,
                        &mut ctx.editor.events,
                    )
                    .unwrap();
                last_buffer_view_handle = Some(handle);
            }

            if let Some(handle) = last_buffer_view_handle {
                ctx.clients
                    .set_buffer_view_handle(ctx.editor, target_client, Some(handle));
            }

            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "save",
        alias: Some("s"),
        help: "save buffer",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx, handle);

            let new_path = match ctx.args.values() {
                [] => None,
                [path] => Some(Path::new(path.as_str(ctx.args))),
                _ => {
                    ctx.output.write_str("command expects 0 or 1 parameters");
                    return Err(CommandError);
                }
            };

            let handle = match handle {
                Some(handle) => parse_arg!(ctx, handle: BufferHandle),
                None => match ctx.current_buffer_view_handle() {
                    Some(handle) => match ctx.editor.buffer_views.get(handle) {
                        Some(view) => view.buffer_handle,
                        None => return Ok(None),
                    },
                    None => {
                        ctx.output.write_str(NO_BUFFER_OPENED_ERROR);
                        return Err(CommandError);
                    }
                },
            };
            let buffer = match ctx.editor.buffers.get_mut(handle) {
                Some(buffer) => buffer,
                None => {
                    ctx.output.write_str(INVALID_BUFFER_HANDLE);
                    return Err(CommandError);
                }
            };

            if let Err(error) = buffer.save_to_file(new_path, &mut ctx.editor.events) {
                ctx.editor
                    .status_bar
                    .write(StatusMessageKind::Error)
                    .fmt(format_args!("{}", error.display(buffer)));
                return Err(CommandError);
            }

            let path = buffer.path().unwrap_or(Path::new(""));
            ctx.editor
                .status_bar
                .write(StatusMessageKind::Info)
                .fmt(format_args!("saved to '{:?}'", path));

            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "save-all",
        alias: Some("sa"),
        help: "save all buffers",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            expect_empty_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            let mut count = 0;
            let mut had_error = false;
            let mut write = ctx.editor.status_bar.write(StatusMessageKind::Error);
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
                Err(CommandError)
            } else {
                ctx.editor
                    .status_bar
                    .write(StatusMessageKind::Info)
                    .fmt(format_args!("{} buffers saved", count));
                Ok(None)
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "reload",
        alias: Some("r"),
        help: "reload buffer from file",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_empty_values!(ctx);
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
                        ctx.output.write_str(NO_BUFFER_OPENED_ERROR);
                        return Err(CommandError);
                    }
                },
            };
            let buffer = match ctx.editor.buffers.get_mut(handle) {
                Some(buffer) => buffer,
                None => {
                    ctx.output.write_str(INVALID_BUFFER_HANDLE);
                    return Err(CommandError);
                }
            };

            if !ctx.bang && buffer.needs_save() {
                return unsaved_changes_error(&mut ctx, "reload");
            }

            if let Err(error) = buffer
                .discard_and_reload_from_file(&mut ctx.editor.word_database, &mut ctx.editor.events)
            {
                ctx.editor
                    .status_bar
                    .write(StatusMessageKind::Error)
                    .fmt(format_args!("{}", error.display(buffer)));
                return Err(CommandError);
            }

            ctx.editor
                .status_bar
                .write(StatusMessageKind::Info)
                .str("buffer reloaded");
            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "reload-all",
        alias: Some("ra"),
        help: "reload all buffers from file",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_empty_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            if !ctx.bang && ctx.editor.buffers.iter().any(Buffer::needs_save) {
                return unsaved_changes_error(&mut ctx, "reload-all");
            }

            let mut count = 0;
            let mut had_error = false;
            let mut write = ctx.editor.status_bar.write(StatusMessageKind::Error);
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
                Err(CommandError)
            } else {
                ctx.editor
                    .status_bar
                    .write(StatusMessageKind::Info)
                    .fmt(format_args!("{} buffers closed", count));
                Ok(None)
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "close",
        alias: Some("c"),
        help: "close buffer",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_empty_values!(ctx);
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
                        ctx.output.write_str(NO_BUFFER_OPENED_ERROR);
                        return Err(CommandError);
                    }
                },
            };
            let buffer = match ctx.editor.buffers.get(handle) {
                Some(buffer) => buffer,
                None => {
                    ctx.output.write_str(INVALID_BUFFER_HANDLE);
                    return Err(CommandError);
                }
            };

            if !ctx.bang && buffer.needs_save() {
                return unsaved_changes_error(&mut ctx, "close");
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
                    client.set_buffer_view_handle(editor, None);
                }
            }

            editor
                .status_bar
                .write(StatusMessageKind::Info)
                .str("buffer closed");
            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "close-all",
        alias: Some("ca"),
        help: "close all buffers",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_empty_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            if !ctx.bang && ctx.editor.buffers.iter().any(Buffer::needs_save) {
                return unsaved_changes_error(&mut ctx, "close-all");
            }

            let count = ctx.editor.buffers.iter().count();
            ctx.editor.buffer_views.defer_remove_buffer_where(
                &mut ctx.editor.buffers,
                &mut ctx.editor.events,
                |_| true,
            );

            for client in ctx.clients.iter_mut() {
                client.set_buffer_view_handle(ctx.editor, None);
            }

            ctx.editor
                .status_bar
                .write(StatusMessageKind::Info)
                .fmt(format_args!("{} buffers closed", count));
            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "config",
        alias: None,
        help: "change an editor config",
        completion_source: CompletionSource::Custom(CONFIG_NAMES),
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            match ctx.args.values() {
                [key] => {
                    let key = key.as_str(ctx.args);
                    match ctx.editor.config.display_config(key) {
                        Some(display) => {
                            ctx.editor
                                .status_bar
                                .write(StatusMessageKind::Info)
                                .fmt(format_args!("{}", display));
                            Ok(None)
                        }
                        None => {
                            ctx.output
                                .write_fmt(format_args!("no such config '{}'", key));
                            Err(CommandError)
                        }
                    }
                }
                [key, value] => {
                    let key = key.as_str(ctx.args);
                    let value = value.as_str(ctx.args);
                    match ctx.editor.config.parse_config(key, value) {
                        Ok(()) => Ok(None),
                        Err(ParseConfigError::NotFound) => {
                            ctx.output
                                .write_fmt(format_args!("no such config '{}'", key));
                            Err(CommandError)
                        }
                        Err(ParseConfigError::InvalidValue) => {
                            ctx.output.write_fmt(format_args!(
                                "invalid value '{}' for config '{}'",
                                value, key
                            ));
                            Err(CommandError)
                        }
                    }
                }
                _ => {
                    ctx.output.write_str("command expects 1 or 2 parameters");
                    Err(CommandError)
                }
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "theme",
        alias: None,
        help: "change editor theme color",
        completion_source: CompletionSource::Custom(THEME_COLOR_NAMES),
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            let (key, value) = match ctx.args.values() {
                [key] => (key, None),
                [key, value] => (key, Some(value)),
                _ => {
                    ctx.output.write_str("command expects 1 or 2 parameters");
                    return Err(CommandError);
                }
            };

            let key = key.as_str(ctx.args);
            let color = match ctx.editor.theme.color_from_name(key) {
                Some(color) => color,
                None => {
                    ctx.output
                        .write_fmt(format_args!("no such theme color '{}'", key));
                    return Err(CommandError);
                }
            };

            match value {
                Some(value) => {
                    let value = value.as_str(ctx.args);
                    match u32::from_str_radix(value, 16) {
                        Ok(parsed) => {
                            *color = Color::from_u32(parsed);
                            Ok(None)
                        }
                        Err(_) => {
                            ctx.output.write_fmt(format_args!(
                                "invalid value '{}' for color '{}'",
                                value, key
                            ));
                            Err(CommandError)
                        }
                    }
                }
                None => {
                    ctx.editor
                        .status_bar
                        .write(StatusMessageKind::Info)
                        .fmt(format_args!("{:x}", color.into_u32()));
                    Ok(None)
                }
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "syntax",
        alias: None,
        help: "create a syntax definition with patterns for files that match a glob",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);

            let glob = match ctx.args.values() {
                [glob] => glob.as_str(ctx.args),
                _ => {
                    ctx.output.write_str("command expects exactly 1 parameter");
                    return Err(CommandError);
                }
            };

            let mut syntax = Syntax::new();
            syntax.set_glob(glob.as_bytes());

            macro_rules! parse_syntax_rules {
                ($($rule:ident : $token_kind:expr),*) => {
                    parse_options!(ctx $(, $rule)*);
                    $(if let Some($rule) = $rule {
                        if let Err(error) = syntax.set_rule($token_kind, $rule) {
                            return parsing_error(&mut ctx, $rule, &error, 0);
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
            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "map",
        alias: None,
        help: "create a keyboard mapping for a mode",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |mut ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            let (mode, from, to) = match ctx.args.values() {
                [mode, from, to] => (
                    mode.as_str(ctx.args),
                    from.as_str(ctx.args),
                    to.as_str(ctx.args),
                ),
                _ => {
                    ctx.output.write_str("command expects exactly 3 parameters");
                    return Err(CommandError);
                }
            };

            let mode = match mode {
                "normal" => ModeKind::Normal,
                "insert" => ModeKind::Insert,
                "read-line" => ModeKind::ReadLine,
                "picker" => ModeKind::Picker,
                "command" => ModeKind::Command,
                _ => {
                    ctx.output
                        .write_fmt(format_args!("invalid mode '{}'", mode));
                    return Err(CommandError);
                }
            };

            match ctx.editor.keymaps.parse_and_map(mode, from, to) {
                Ok(()) => Ok(None),
                Err(ParseKeyMapError::From(e)) => parsing_error(&mut ctx, from, &e.error, e.index),
                Err(ParseKeyMapError::To(e)) => parsing_error(&mut ctx, to, &e.error, e.index),
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "register",
        alias: None,
        help: "change an editor register",
        completion_source: CompletionSource::None,
        flags: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);

            let (key, value) = match ctx.args.values() {
                [key] => (key.as_str(ctx.args), None),
                [key, value] => (key.as_str(ctx.args), Some(value.as_str(ctx.args))),
                _ => {
                    ctx.output.write_str("command expects 1 or 2 parameters");
                    return Err(CommandError);
                }
            };

            let key = match RegisterKey::from_str(key) {
                Some(key) => key,
                None => {
                    ctx.output
                        .write_fmt(format_args!("invalid register key '{}'", key));
                    return Err(CommandError);
                }
            };

            match value {
                Some(value) => ctx.editor.registers.set(key, value),
                None => ctx
                    .editor
                    .status_bar
                    .write(StatusMessageKind::Info)
                    .str(ctx.editor.registers.get(key)),
            }

            Ok(None)
        },
    });
}

// lsp:
// - lsp-start
// - lsp-stop
// - lsp-hover
// - lsp-signature-help
// - lsp-open-log
