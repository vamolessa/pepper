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
    command::{BuiltinCommand, CommandContext, CommandError, CommandOperation, CompletionSource},
    config::{ParseConfigError, CONFIG_NAMES},
    editor::Editor,
    editor_utils::MessageKind,
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

pub const COMMANDS: &[BuiltinCommand] = &[
    BuiltinCommand {
        names: &["quit", "q"],
        help: "quits this client. append a '!' to force quit",
        accepts_bang: true,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            if ctx.clients.iter_mut().count() == 1 {
                ctx.assert_can_discard_all_buffers()?;
            }
            Ok(Some(CommandOperation::Quit))
        },
    },
    BuiltinCommand {
        names: &["quit-all", "qa"],
        help: "quits all clients. append a '!' to force quit all",
        accepts_bang: true,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            ctx.assert_can_discard_all_buffers()?;
            Ok(Some(CommandOperation::QuitAll))
        },
    },
    BuiltinCommand {
        names: &["print"],
        help: "prints a message to the status bar",
        accepts_bang: false,
        required_values: &[],
        optional_values: &[],
        extra_values: Some(None),
        flags: &[],
        func: |ctx| {
            let mut w = ctx.editor.status_bar.write(MessageKind::Info);
            for arg in ctx.args.other_values.iter().flatten() {
                w.str(arg);
                w.str(" ");
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["source"],
        help: "load a source file and execute its commands",
        accepts_bang: false,
        required_values: &[("path", Some(CompletionSource::Files))],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            let path = ctx.args.required_values[0];
            let op = ctx.editor.load_config(ctx.platform, ctx.clients, path);
            Ok(op)
        },
    },
    BuiltinCommand {
        names: &["open", "o"],
        help: "open a buffer for editting",
        accepts_bang: false,
        required_values: &[("path", Some(CompletionSource::Files))],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            let client_handle = ctx.client_handle.ok_or(CommandError::Aborted)?;
            NavigationHistory::save_client_snapshot(
                ctx.clients,
                client_handle,
                &ctx.editor.buffer_views,
            );

            let mut path = ctx.args.required_values[0];
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
                        .get_mut(client_handle)
                        .ok_or(CommandError::Aborted)?
                        .set_buffer_view_handle(Some(handle));
                    use fmt::Write;
                    let _ = write!(ctx.output, "{}", handle);
                    Ok(None)
                }
                Err(BufferViewError::InvalidPath) => Err(CommandError::InvalidPath(path)),
            }
        },
    },
    BuiltinCommand {
        names: &["save", "s"],
        help: "save buffer",
        accepts_bang: false,
        required_values: &[],
        optional_values: &[("path", Some(CompletionSource::Files))],
        extra_values: None,
        flags: &[("buffer", None)],
        func: |ctx| {
            let path = ctx.args.other_values[0];
            let handle = match ctx.args.parse_flag(0)? {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };
            let buffer = ctx
                .editor
                .buffers
                .get_mut(handle)
                .ok_or(CommandError::InvalidBufferHandle(handle))?;
            buffer
                .save_to_file(path.map(Path::new), &mut ctx.editor.events)
                .map_err(|e| CommandError::BufferError(handle, e))?;

            let path = buffer.path().unwrap_or(Path::new(""));
            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("saved to '{:?}'", path));
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["save-all", "sa"],
        help: "save all buffers",
        accepts_bang: false,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            let mut count = 0;
            for buffer in ctx.editor.buffers.iter_mut() {
                buffer
                    .save_to_file(None, &mut ctx.editor.events)
                    .map_err(|e| CommandError::BufferError(buffer.handle(), e))?;
                count += 1;
            }
            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("{} buffers saved", count));
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["reload", "r"],
        help: "reload buffer from file",
        accepts_bang: true,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[("buffer", None)],
        func: |ctx| {
            let handle = match ctx.args.parse_flag(0)? {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };
            ctx.assert_can_discard_buffer(handle)?;
            let buffer = ctx
                .editor
                .buffers
                .get_mut(handle)
                .ok_or(CommandError::InvalidBufferHandle(handle))?;

            buffer
                .discard_and_reload_from_file(&mut ctx.editor.word_database, &mut ctx.editor.events)
                .map_err(|e| CommandError::BufferError(handle, e))?;

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .str("buffer reloaded");
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["reload-all", "ra"],
        help: "reload all buffers from file",
        accepts_bang: true,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            ctx.assert_can_discard_all_buffers()?;
            let mut count = 0;
            for buffer in ctx.editor.buffers.iter_mut() {
                buffer
                    .discard_and_reload_from_file(
                        &mut ctx.editor.word_database,
                        &mut ctx.editor.events,
                    )
                    .map_err(|e| CommandError::BufferError(buffer.handle(), e))?;
                count += 1;
            }
            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("{} buffers reloaded", count));
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["close", "c"],
        help: "close buffer",
        accepts_bang: true,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[("buffer", None)],
        func: |ctx| {
            let handle = match ctx.args.parse_flag(0)? {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };
            ctx.assert_can_discard_buffer(handle)?;

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
                .status_bar
                .write(MessageKind::Info)
                .str("buffer closed");
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["close-all", "ca"],
        help: "close all buffers",
        accepts_bang: true,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            ctx.assert_can_discard_all_buffers()?;
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
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("{} buffers closed", count));
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["config"],
        help: "change an editor config",
        accepts_bang: false,
        required_values: &[("key", Some(CompletionSource::Custom(CONFIG_NAMES)))],
        optional_values: &[("value", None)],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            let key = ctx.args.required_values[0];
            let value = ctx.args.other_values[0];
            match value {
                Some(value) => match ctx.editor.config.parse_config(key, value) {
                    Ok(()) => Ok(None),
                    Err(ParseConfigError::NotFound) => Err(CommandError::ConfigNotFound(key)),
                    Err(ParseConfigError::InvalidValue) => {
                        Err(CommandError::InvalidConfigValue { key, value })
                    }
                },
                None => match ctx.editor.config.display_config(key) {
                    Some(display) => {
                        use fmt::Write;
                        let _ = write!(ctx.output, "{}", display);
                        Ok(None)
                    }
                    None => Err(CommandError::ConfigNotFound(key)),
                },
            }
        },
    },
    BuiltinCommand {
        names: &["theme"],
        help: "change editor theme color",
        accepts_bang: false,
        required_values: &[("key", Some(CompletionSource::Custom(THEME_COLOR_NAMES)))],
        optional_values: &[("value", None)],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            let key = ctx.args.required_values[0];
            let value = ctx.args.other_values[0];
            let color = ctx
                .editor
                .theme
                .color_from_name(key)
                .ok_or(CommandError::ConfigNotFound(key))?;
            match value {
                Some(value) => {
                    let encoded = u32::from_str_radix(value, 16)
                        .map_err(|_| CommandError::InvalidColorValue { key, value })?;
                    *color = Color::from_u32(encoded);
                }
                None => {
                    use fmt::Write;
                    let _ = write!(ctx.output, "{:x}", color.into_u32());
                }
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["syntax"],
        help: "create a syntax definition with patterns for files that match a glob",
        accepts_bang: false,
        required_values: &[("glob", None)],
        optional_values: &[],
        extra_values: None,
        flags: &[
            ("keywords", None),
            ("types", None),
            ("symbols", None),
            ("literals", None),
            ("strings", None),
            ("comments", None),
            ("texts", None),
        ],
        func: |ctx| {
            let glob = ctx.args.required_values[0];

            let mut syntax = Syntax::new();
            syntax
                .set_glob(glob.as_bytes())
                .map_err(|_| CommandError::InvalidGlob(glob))?;

            let kinds = [
                TokenKind::Keyword,
                TokenKind::Type,
                TokenKind::Symbol,
                TokenKind::Literal,
                TokenKind::String,
                TokenKind::Comment,
                TokenKind::Text,
            ];
            for (&kind, &flag) in kinds.iter().zip(ctx.args.flags.iter()) {
                if let Some(flag) = flag {
                    syntax
                        .set_rule(kind, flag)
                        .map_err(|e| CommandError::PatternError(flag, e))?;
                }
            }

            ctx.editor.syntaxes.add(syntax);
            for buffer in ctx.editor.buffers.iter_mut() {
                buffer.refresh_syntax(&ctx.editor.syntaxes);
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["map"],
        help: "create a keyboard mapping for a mode",
        accepts_bang: false,
        required_values: &[("from", None), ("to", None)],
        optional_values: &[],
        extra_values: None,
        flags: &[
            ("normal", None),
            ("insert", None),
            ("read-line", None),
            ("picker", None),
            ("command", None),
        ],
        func: |ctx| {
            let from = ctx.args.required_values[0];
            let to = ctx.args.required_values[0];

            let kinds = [
                ModeKind::Normal,
                ModeKind::Insert,
                ModeKind::ReadLine,
                ModeKind::Picker,
                ModeKind::Command,
            ];
            for (&kind, flag) in kinds.iter().zip(ctx.args.flags.iter()) {
                if flag.is_some() {
                    ctx.editor
                        .keymaps
                        .parse_and_map(kind, from, to)
                        .map_err(|e| match e {
                            ParseKeyMapError::From(e) => {
                                CommandError::KeyParseError(&from[e.index..], e.error)
                            }
                            ParseKeyMapError::To(e) => {
                                CommandError::KeyParseError(&to[e.index..], e.error)
                            }
                        })?;
                }
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["register"],
        help: "change an editor register",
        accepts_bang: false,
        required_values: &[("key", None)],
        optional_values: &[("value", None)],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            let key = ctx.args.required_values[0];
            let value = ctx.args.other_values[0];
            let register = match RegisterKey::from_str(key) {
                Some(key) => ctx.editor.registers.get_mut(key),
                None => return Err(CommandError::InvalidRegisterKey(key)),
            };
            match value {
                Some(value) => {
                    register.clear();
                    register.push_str(value);
                }
                None => ctx.output.push_str(register),
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["run"],
        help: "",
        accepts_bang: false,
        required_values: &[("command", None)],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
            let command = ctx.args.required_values[0];
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
            Ok(None)
        },
    },
    /*
    BuiltinCommand {
        names: &["lsp-start"],
        help: "start a lsp server",
        accepts_bang: false,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
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
        accepts_bang: false,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[],
        func: |ctx| {
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
        accepts_bang: false,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[],
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
        accepts_bang: false,
        required_values: &[],
        optional_values: &[],
        extra_values: None,
        flags: &[],
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

/*
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
*/

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
