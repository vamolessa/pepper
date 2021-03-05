use std::{
    fmt,
    path::Path,
    process::{Command, Stdio},
};

use crate::{
    application::ProcessTag,
    buffer::{BufferCapabilities, BufferHandle},
    buffer_position::BufferPosition,
    buffer_view::BufferViewError,
    command::{
        BuiltinCommand, CommandContext, CommandError, CommandOperation, CommandSource,
        CompletionSource,
    },
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
        names: &["help", "h"],
        description: "prints help about command",
        bang_usage: None,
        required_values: &[("command-name", Some(CompletionSource::Commands))],
        optional_values: &[],
        flags: &[],
        func: |ctx| {
            let command_name = ctx.args.values[0];
            let commands = &ctx.editor.commands;
            let source = match commands.find_command(command_name) {
                Some(source) => source,
                None => return Err(CommandError::CommandNotFound(command_name)),
            };

            let name;
            let aliases;
            let description;
            let bang_usage;
            let flags;
            let required_values;
            let optional_values;

            match source {
                CommandSource::Builtin(i) => {
                    let command = &commands.builtin_commands()[i];
                    name = command.names[0];
                    aliases = &command.names[1..];
                    description = command.description;
                    bang_usage = command.bang_usage;
                    flags = command.flags;
                    required_values = command.required_values;
                    optional_values = command.optional_values;
                }
            }

            let mut write = ctx.editor.status_bar.write(MessageKind::Info);

            write.fmt(format_args!("{}\nusage: {}", name, name));
            if bang_usage.is_some() {
                write.str("[!]");
            }
            for (flag, _) in flags {
                write.fmt(format_args!(" [-{}]", flag));
            }
            for (value, _) in required_values {
                write.fmt(format_args!(" {}", value));
            }
            for (value, _) in optional_values {
                write.fmt(format_args!(" [{}]", value));
            }

            write.fmt(format_args!("\ndescription: {}\n", description));
            if let Some(usage) = bang_usage {
                write.fmt(format_args!("with '!': {}\n", usage));
            }

            if !aliases.is_empty() {
                write.str("aliases: ");
                write.fmt(format_args!("{}", aliases[0]));
                for alias in &aliases[1..] {
                    write.fmt(format_args!(", {}", alias));
                }
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["quit", "q"],
        description: "quits this client",
        bang_usage: Some("ignore unsaved changes"),
        required_values: &[],
        optional_values: &[],
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
        description: "quits all clients",
        bang_usage: Some("ignore unsaved changes"),
        required_values: &[],
        optional_values: &[],
        flags: &[],
        func: |ctx| {
            ctx.assert_can_discard_all_buffers()?;
            Ok(Some(CommandOperation::QuitAll))
        },
    },
    BuiltinCommand {
        names: &["print", "p"],
        description: "prints values to the status bar",
        bang_usage: None,
        required_values: &[("message", None)],
        optional_values: &[],
        flags: &[],
        func: |ctx| {
            let message = ctx.args.values[0];
            ctx.editor.status_bar.write(MessageKind::Info).str(message);
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["source"],
        description: "load a source file and execute its commands",
        bang_usage: None,
        required_values: &[("path", Some(CompletionSource::Files))],
        optional_values: &[],
        flags: &[],
        func: |ctx| {
            let path = ctx.args.values[0];
            let op = ctx.editor.load_config(ctx.platform, ctx.clients, path);
            Ok(op)
        },
    },
    BuiltinCommand {
        names: &["open", "o"],
        description: "open a buffer for editting",
        bang_usage: None,
        required_values: &[("path", Some(CompletionSource::Files))],
        optional_values: &[],
        flags: &[],
        func: |ctx| {
            let client_handle = ctx.client_handle.ok_or(CommandError::Aborted)?;
            NavigationHistory::save_client_snapshot(
                ctx.clients,
                client_handle,
                &ctx.editor.buffer_views,
            );

            let mut path = ctx.args.values[0];
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
        description: "save buffer",
        bang_usage: None,
        required_values: &[],
        optional_values: &[("path", Some(CompletionSource::Files))],
        flags: &[("buffer", None)],
        func: |ctx| {
            let path = ctx.args.values[0];
            let path = if path.is_empty() {
                None
            } else {
                Some(Path::new(path))
            };

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
                .save_to_file(path, &mut ctx.editor.events)
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
        description: "save all buffers",
        bang_usage: None,
        required_values: &[],
        optional_values: &[],
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
        description: "reload buffer from file",
        bang_usage: Some("ignore unsaved changes"),
        required_values: &[],
        optional_values: &[],
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
        description: "reload all buffers from file",
        bang_usage: Some("ignore unsaved changes"),
        required_values: &[],
        optional_values: &[],
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
        description: "close buffer",
        bang_usage: Some("ignore unsaved changes"),
        required_values: &[],
        optional_values: &[],
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
        description: "close all buffers",
        bang_usage: Some("ignore unsaved changes"),
        required_values: &[],
        optional_values: &[],
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
        description: "change an editor config",
        bang_usage: None,
        required_values: &[("key", Some(CompletionSource::Custom(CONFIG_NAMES)))],
        optional_values: &[("value", None)],
        flags: &[],
        func: |ctx| {
            let key = ctx.args.values[0];
            let value = ctx.args.values[1];
            if value.is_empty() {
                match ctx.editor.config.display_config(key) {
                    Some(display) => {
                        use fmt::Write;
                        let _ = write!(ctx.output, "{}", display);
                        Ok(None)
                    }
                    None => Err(CommandError::ConfigNotFound(key)),
                }
            } else {
                match ctx.editor.config.parse_config(key, value) {
                    Ok(()) => Ok(None),
                    Err(ParseConfigError::NotFound) => Err(CommandError::ConfigNotFound(key)),
                    Err(ParseConfigError::InvalidValue) => {
                        Err(CommandError::InvalidConfigValue { key, value })
                    }
                }
            }
        },
    },
    BuiltinCommand {
        names: &["theme"],
        description: "change editor theme color",
        bang_usage: None,
        required_values: &[("key", Some(CompletionSource::Custom(THEME_COLOR_NAMES)))],
        optional_values: &[("value", None)],
        flags: &[],
        func: |ctx| {
            let key = ctx.args.values[0];
            let value = ctx.args.values[1];
            let color = ctx
                .editor
                .theme
                .color_from_name(key)
                .ok_or(CommandError::ConfigNotFound(key))?;
            if value.is_empty() {
                use fmt::Write;
                let _ = write!(ctx.output, "{:x}", color.into_u32());
            } else {
                let encoded = u32::from_str_radix(value, 16)
                    .map_err(|_| CommandError::InvalidColorValue { key, value })?;
                *color = Color::from_u32(encoded);
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["syntax"],
        description: "create a syntax definition with patterns for files that match a glob",
        bang_usage: None,
        required_values: &[("glob", None)],
        optional_values: &[],
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
            let glob = ctx.args.values[0];

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
                if !flag.is_empty() {
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
        description: "create a keyboard mapping for a mode",
        bang_usage: None,
        required_values: &[("from", None), ("to", None)],
        optional_values: &[],
        flags: &[
            ("normal", None),
            ("insert", None),
            ("read-line", None),
            ("picker", None),
            ("command", None),
        ],
        func: |ctx| {
            let from = ctx.args.values[0];
            let to = ctx.args.values[1];

            let kinds = [
                ModeKind::Normal,
                ModeKind::Insert,
                ModeKind::ReadLine,
                ModeKind::Picker,
                ModeKind::Command,
            ];

            for (&kind, flag) in kinds.iter().zip(ctx.args.flags.iter()) {
                if flag.is_empty() {
                    continue;
                }

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
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["register"],
        description: "change an editor register",
        bang_usage: None,
        required_values: &[("key", None)],
        optional_values: &[("value", None)],
        flags: &[],
        func: |ctx| {
            let key = ctx.args.values[0];
            let value = ctx.args.values[1];
            let register = match RegisterKey::from_str(key) {
                Some(key) => ctx.editor.registers.get_mut(key),
                None => return Err(CommandError::InvalidRegisterKey(key)),
            };
            if value.is_empty() {
                ctx.output.push_str(register);
            } else {
                register.clear();
                register.push_str(value);
            }
            Ok(None)
        },
    },
    // TODO: remove this command
    BuiltinCommand {
        names: &["run"],
        description: "",
        bang_usage: None,
        required_values: &[("command", None)],
        optional_values: &[],
        flags: &[],
        func: |ctx| {
            let command = ctx.args.values[0];
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
    BuiltinCommand {
        names: &["lsp-start"],
        description: "start a lsp server",
        bang_usage: None,
        required_values: &[("server-command", None)],
        optional_values: &[("server-args", None)],
        flags: &[("root", Some(CompletionSource::Files)), ("log", None)],
        func: |ctx| {
            let command_name = ctx.args.values[0];
            let root = ctx.args.flags[0];
            let log = !ctx.args.flags[1].is_empty();

            let mut command = Command::new(command_name);
            command.arg(ctx.args.values[1]);

            let root = if root.is_empty() {
                ctx.editor.current_directory.as_path()
            } else {
                Path::new(root)
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

            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["lsp-stop"],
        description: "stop a lsp server",
        bang_usage: None,
        required_values: &[],
        optional_values: &[],
        flags: &[("client", None)],
        func: |ctx| {
            match ctx.args.parse_flag(0)? {
                Some(client) => ctx.editor.lsp.stop(ctx.platform, client),
                None => ctx.editor.lsp.stop_all(ctx.platform),
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["lsp-hover"],
        description: "perform a lsp hover action",
        bang_usage: None,
        required_values: &[],
        optional_values: &[],
        flags: &[("buffer", None), ("position", None)],
        func: |mut ctx| {
            access_lsp_with_position(
                &mut ctx,
                |editor, client, platform, json, buffer_handle, position| {
                    client.hover(editor, platform, json, buffer_handle, position)
                },
            )?;
            Ok(None)
        },
    },
    BuiltinCommand {
        names: &["lsp-signature-help"],
        description: "perform a lsp hover action",
        bang_usage: None,
        required_values: &[],
        optional_values: &[],
        flags: &[("buffer", None), ("position", None)],
        func: |mut ctx| {
            access_lsp_with_position(
                &mut ctx,
                |editor, client, platform, json, buffer_handle, position| {
                    client.signature_help(editor, platform, json, buffer_handle, position)
                },
            )?;
            Ok(None)
        },
    },
];

fn get_main_cursor_position<'state, 'command>(
    ctx: &CommandContext<'state, 'command>,
) -> Result<BufferPosition, CommandError<'command>> {
    let handle = ctx.current_buffer_view_handle()?;
    let position = ctx
        .editor
        .buffer_views
        .get(handle)
        .ok_or(CommandError::Aborted)?
        .cursors
        .main_cursor()
        .position;
    Ok(position)
}

fn find_lsp_client_for_buffer(
    editor: &Editor,
    buffer_handle: BufferHandle,
) -> Option<lsp::ClientHandle> {
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

fn access_lsp<'command, A>(
    editor: &mut Editor,
    buffer_handle: BufferHandle,
    accessor: A,
) -> Result<(), CommandError<'command>>
where
    A: FnOnce(&mut Editor, &mut lsp::Client, &mut Json),
{
    match find_lsp_client_for_buffer(editor, buffer_handle)
        .and_then(|h| lsp::ClientManager::access(editor, h, accessor))
    {
        Some(()) => Ok(()),
        None => Err(CommandError::LspServerNotRunning),
    }
}

fn access_lsp_with_position<'state, 'command, A>(
    ctx: &mut CommandContext<'state, 'command>,
    accessor: A,
) -> Result<(), CommandError<'command>>
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
    let buffer_handle = match ctx.args.parse_flag(0)? {
        Some(handle) => handle,
        None => ctx.current_buffer_handle()?,
    };
    let position = match ctx.args.parse_flag(1)? {
        Some(position) => position,
        None => get_main_cursor_position(ctx)?,
    };

    let platform = &mut *ctx.platform;
    access_lsp(ctx.editor, buffer_handle, |editor, client, json| {
        accessor(editor, client, platform, json, buffer_handle, position)
    })
}
