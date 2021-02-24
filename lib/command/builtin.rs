use std::{
    fmt,
    path::Path,
    process::{Command, Stdio},
};

use crate::{
    application::ProcessTag,
    buffer::{Buffer, BufferCapabilities, BufferHandle},
    buffer_position::BufferPosition,
    buffer_view::BufferViewError,
    command::{BuiltinCommand, CommandContext, CommandOperation, CompletionSource},
    config::{ParseConfigError, CONFIG_NAMES},
    editor::{Editor, EditorOutputKind},
    json::Json,
    keymap::ParseKeyMapError,
    lsp,
    mode::ModeKind,
    navigation_history::NavigationHistory,
    platform::{Platform, PlatformRequest},
    register::RegisterKey,
    syntax::{Syntax, TokenKind},
    theme::{Color, THEME_COLOR_NAMES},
};

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
        let mut values = $ctx.editor.commands.args().values().iter();
        $(let $name = values.next().map(|v| v.as_str($ctx.editor.commands.args()));)*
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
            for switch in $ctx.editor.commands.args().switches() {
                let switch = switch.as_str($ctx.editor.commands.args());
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
            for (key, value) in $ctx.editor.commands.args().options() {
                let key = key.as_str($ctx.editor.commands.args());
                match key {
                    $(stringify!($name) => $name = Some(value.as_str($ctx.editor.commands.args())),)*
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

fn parse_arg2<T>(ctx: &mut CommandContext, name: &'static str, value: &str) -> Option<T>
where
    T: 'static + std::str::FromStr,
{
    match value.parse() {
        Ok(value) => Some(value),
        Err(_) => {
            ctx.editor
                .output
                .write(EditorOutputKind::Error)
                .fmt(format_args!(
                    "could not convert argument '{}' value '{}' to {}",
                    name,
                    value,
                    std::any::type_name::<T>()
                ));
            None
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
        values_completion_source: None,
        switches: &[],
        options: &[],
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
        values_completion_source: None,
        switches: &[],
        options: &[],
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
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            let mut w = ctx.editor.output.write(EditorOutputKind::Info);
            let args = ctx.editor.commands.args();
            for arg in args.values() {
                w.str(arg.as_str(args));
                w.str(" ");
            }
            None
        },
    },
    /*
    BuiltinCommand {
        names: &["source"],
        help: "load a source file and execute its commands",
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx);
            let args = ctx.editor.commands.args();
            for path in args.values() {
                let path = path.as_str(args);
                if let Some(CommandOperation::Quit) | Some(CommandOperation::QuitAll) =
                    ctx.editor.load_config(ctx.platform, ctx.clients, path)
                {
                    break;
                }
            }
            None
        },
    },
    */
    BuiltinCommand {
        names: &["open", "o"],
        help: "open a buffer for editting",
        values_completion_source: None,
        switches: &[],
        options: &[],
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
            let args = ctx.editor.commands.args();

            for path in args.values() {
                let mut path = path.as_str(args);

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
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |mut ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx, path);
            parse_switches!(ctx);
            parse_options!(ctx, buffer);

            let handle = match buffer {
                Some(buffer) => parse_arg!(ctx, buffer: BufferHandle),
                None => ctx.current_buffer_handle_or_error()?,
            };
            let handle = ctx.validate_buffer_handle(handle)?;
            let buffer = ctx.editor.buffers.get_mut(handle)?;

            let path = path.map(Path::new);
            /*
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
            */

            None
        },
    },
    BuiltinCommand {
        names: &["save-all", "sa"],
        help: "save all buffers",
        values_completion_source: None,
        switches: &[],
        options: &[],
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
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |mut ctx| {
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx, buffer);

            let handle = match buffer {
                Some(buffer) => parse_arg!(ctx, buffer: BufferHandle),
                None => ctx.current_buffer_handle_or_error()?,
            };
            let handle = ctx.validate_buffer_handle(handle)?;
            let buffer = ctx.editor.buffers.get_mut(handle)?;

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
        values_completion_source: None,
        switches: &[],
        options: &[],
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
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |mut ctx| {
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx, buffer);

            let handle = match buffer {
                Some(buffer) => parse_arg!(ctx, buffer: BufferHandle),
                None => ctx.current_buffer_handle_or_error()?,
            };
            let handle = ctx.validate_buffer_handle(handle)?;
            let buffer = ctx.editor.buffers.get(handle)?;

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
        values_completion_source: None,
        switches: &[],
        options: &[],
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
    /*
    BuiltinCommand {
        names: &["config"],
        help: "change an editor config",
        values_completion_source: CompletionSource::Custom(CONFIG_NAMES),
        switches: &[],
        options: &[],
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
        values_completion_source: CompletionSource::Custom(THEME_COLOR_NAMES),
        switches: &[],
        options: &[],
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
        values_completion_source: None,
        switches: &[],
        options: &[],
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
        values_completion_source: None,
        switches: &[],
        options: &[],
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
        values_completion_source: None,
        switches: &[],
        options: &[],
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
                Some(value) => {
                    let register = ctx.editor.registers.get_mut(key);
                    register.clear();
                    register.push_str(value);
                }
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
        names: &["run"],
        help: "",
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx, command);
            parse_switches!(ctx);
            parse_options!(ctx);

            require_value!(ctx, command);
            eprintln!("request spawn process '{}'", command);

            let mut command = Command::new(command);
            command.stdin(Stdio::null());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::null());

            ctx.platform.enqueue_request(PlatformRequest::SpawnProcess {
                tag: ProcessTag::Command(0),
                command,
                stdout_buf_len: 4 * 1024,
                stderr_buf_len: 0,
            });

            None
        },
    },
    BuiltinCommand {
        names: &["lsp-start"],
        help: "start a lsp server",
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_switches!(ctx, log);
            parse_options!(ctx, root);

            let args = ctx.editor.commands.args();
            let (command_name, command_args) = match args.values() {
                [command, command_args @ ..] => (command.as_str(args), command_args),
                _ => {
                    ctx.editor
                        .output
                        .write(EditorOutputKind::Error)
                        .str("value 'command' is required");
                    return None;
                }
            };

            let mut command = Command::new(command_name);
            for arg in command_args {
                let arg = arg.as_str(args);
                command.arg(arg);
            }

            let root = match root {
                Some(root) => Path::new(root),
                None => ctx.editor.current_directory.as_path(),
            };

            let handle = ctx.editor.lsp.start(ctx.platform, command, root.into());
            if let (true, Some(client_handle)) = (log, ctx.client_handle) {
                let clients = ctx.clients;
                lsp::ClientManager::access(ctx.editor, handle, |editor, client, _| {
                    let buffer = editor.buffers.new(BufferCapabilities::log());
                    let buffer_handle = buffer.handle();
                    buffer.set_path(Some(Path::new(command_name)));
                    client.set_log_buffer(Some(buffer_handle));
                    let buffer_view_handle = editor
                        .buffer_views
                        .buffer_view_handle_from_buffer_handle(client_handle, buffer_handle);
                    if let Some(client) = clients.get_mut(client_handle) {
                        client.set_buffer_view_handle(Some(buffer_view_handle));
                    }
                });
            }
            None
        },
    },
    BuiltinCommand {
        names: &["lsp-stop"],
        help: "stop a lsp server",
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |ctx| {
            expect_no_bang!(ctx);
            parse_values!(ctx);
            parse_switches!(ctx);
            parse_options!(ctx, client);

            match client {
                Some(client) => {
                    let client = parse_arg!(ctx, client: lsp::ClientHandle);
                    ctx.editor.lsp.stop(ctx.platform, client);
                }
                None => ctx.editor.lsp.stop_all(ctx.platform),
            }

            None
        },
    },
    BuiltinCommand {
        names: &["lsp-hover"],
        help: "perform a lsp hover action",
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |mut ctx| {
            access_lsp_with_position(
                &mut ctx,
                |editor, client, platform, json, buffer_handle, position| {
                    client.hover(editor, platform, json, buffer_handle, position)
                },
            );
            None
        },
    },
    BuiltinCommand {
        names: &["lsp-signature-help"],
        help: "perform a lsp hover action",
        values_completion_source: None,
        switches: &[],
        options: &[],
        func: |mut ctx| {
            access_lsp_with_position(
                &mut ctx,
                |editor, client, platform, json, buffer_handle, position| {
                    client.signature_help(editor, platform, json, buffer_handle, position)
                },
            );
            None
        },
    },
    */
];

fn get_main_cursor_position(ctx: &mut CommandContext) -> Option<BufferPosition> {
    let handle = ctx.current_buffer_view_handle_or_error()?;
    let position = ctx
        .editor
        .buffer_views
        .get(handle)?
        .cursors
        .main_cursor()
        .position;
    Some(position)
}

fn access_lsp<A>(
    editor: &mut Editor,
    client_handle: Option<lsp::ClientHandle>,
    buffer_handle: Option<BufferHandle>,
    accessor: A,
) where
    A: FnOnce(&mut Editor, &mut lsp::Client, &mut Json),
{
    fn find_client_for_buffer(
        editor: &Editor,
        buffer_handle: Option<BufferHandle>,
    ) -> Option<lsp::ClientHandle> {
        let buffer_handle = buffer_handle?;
        let buffer_path_bytes = editor
            .buffers
            .get(buffer_handle)?
            .path()?
            .to_str()?
            .as_bytes();
        let (client_handle, _) = editor
            .lsp
            .client_with_handles()
            .find(|(_, c)| c.handles_path(buffer_path_bytes))?;
        Some(client_handle)
    }

    if client_handle
        .or_else(|| find_client_for_buffer(editor, buffer_handle))
        .and_then(|h| lsp::ClientManager::access(editor, h, accessor))
        .is_none()
    {
        editor
            .output
            .write(EditorOutputKind::Error)
            .str("lsp server not running");
    }
}

/*
fn access_lsp_with_position<A>(ctx: &mut CommandContext, accessor: A) -> Option<()>
where
    A: FnOnce(
        &mut Editor,
        &mut lsp::Client,
        &mut Platform,
        &mut Json,
        BufferHandle,
        BufferPosition,
    ),
{
    expect_no_bang!(ctx);
    parse_values!(ctx);
    parse_switches!(ctx);
    parse_options!(ctx, client, buffer, position);

    let client_handle = match client {
        Some(client) => Some(parse_arg!(ctx, client: lsp::ClientHandle)),
        None => None,
    };
    let buffer_handle = match buffer {
        Some(buffer) => parse_arg!(ctx, buffer: BufferHandle),
        None => ctx.current_buffer_handle_or_error()?,
    };
    let position = match position {
        Some(position) => parse_arg!(ctx, position: BufferPosition),
        None => get_main_cursor_position(ctx)?,
    };

    let platform = &mut *ctx.platform;
    access_lsp(
        ctx.editor,
        client_handle,
        Some(buffer_handle),
        |editor, client, json| accessor(editor, client, platform, json, buffer_handle, position),
    );

    None
}
*/
