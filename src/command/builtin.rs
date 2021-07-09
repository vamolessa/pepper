use std::path::Path;

use crate::{
    buffer::{parse_path_and_position, BufferHandle},
    buffer_position::BufferPosition,
    client::ClientManager,
    command::{BuiltinCommand, CommandContext, CommandError, CommandOperation, CompletionSource},
    config::{ParseConfigError, CONFIG_NAMES},
    cursor::Cursor,
    editor::Editor,
    editor_utils::MessageKind,
    help, lsp,
    mode::ModeKind,
    navigation_history::NavigationHistory,
    platform::Platform,
    theme::{Color, THEME_COLOR_NAMES},
};

pub static COMMANDS: &[BuiltinCommand] = &[
    BuiltinCommand {
        name: "help",
        completions: &[CompletionSource::Commands],
        func: |ctx| {
            let keyword = ctx.args.try_next();
            ctx.args.assert_empty()?;

            let (path, position) = match keyword.and_then(|k| help::search(k)) {
                Some((path, line_index)) => (path, BufferPosition::line_col(line_index as _, 0)),
                None => (help::main_help_path(), BufferPosition::zero()),
            };

            if let Some(client_handle) = ctx.client_handle {
                let handle = ctx.editor.buffer_view_handle_from_path(client_handle, path);
                if let Some(buffer_view) = ctx.editor.buffer_views.get_mut(handle) {
                    let mut cursors = buffer_view.cursors.mut_guard();
                    cursors.clear();
                    cursors.add(Cursor {
                        anchor: position,
                        position,
                    });
                }

                if let Some(client) = ctx.clients.get_mut(client_handle) {
                    client.set_buffer_view_handle(Some(handle), &mut ctx.editor.events);
                    client.scroll.0 = 0;
                    client.scroll.1 = position.line_index.saturating_sub((client.height / 2) as _);
                }
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "quit",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            if ctx.clients.iter().count() == 1 {
                ctx.assert_can_discard_all_buffers()?;
            }
            Ok(Some(CommandOperation::Quit))
        },
    },
    BuiltinCommand {
        name: "quit-all",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            ctx.assert_can_discard_all_buffers()?;
            Ok(Some(CommandOperation::QuitAll))
        },
    },
    BuiltinCommand {
        name: "open",
        completions: &[CompletionSource::Files],
        func: |ctx| {
            let path = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };

            let (path, position) = parse_path_and_position(path);

            NavigationHistory::save_client_snapshot(
                ctx.clients,
                client_handle,
                &ctx.editor.buffer_views,
            );

            let path = ctx.editor.string_pool.acquire_with(path);
            let handle = ctx
                .editor
                .buffer_view_handle_from_path(client_handle, Path::new(&path));
            ctx.editor.string_pool.release(path);

            if let Some(buffer_view) = ctx.editor.buffer_views.get_mut(handle) {
                if let Some(position) = position {
                    let mut cursors = buffer_view.cursors.mut_guard();
                    cursors.clear();
                    cursors.add(Cursor {
                        anchor: position,
                        position,
                    });
                }
            }

            if let Some(client) = ctx.clients.get_mut(client_handle) {
                client.set_buffer_view_handle(Some(handle), &mut ctx.editor.events);
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "save",
        completions: &[],
        func: |ctx| {
            let path = ctx.args.try_next().map(|p| Path::new(p));
            ctx.args.assert_empty()?;

            let buffer_handle = ctx.current_buffer_handle()?;
            let buffer = ctx
                .editor
                .buffers
                .get_mut(buffer_handle)
                .ok_or(CommandError::NoBufferOpened)?;

            buffer
                .save_to_file(path, &mut ctx.editor.events)
                .map_err(CommandError::IoError)?;

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("buffer saved to {:?}", &buffer.path));
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "save-all",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            let mut count = 0;
            for buffer in ctx.editor.buffers.iter_mut() {
                if buffer.capabilities.can_save {
                    buffer
                        .save_to_file(None, &mut ctx.editor.events)
                        .map_err(CommandError::IoError)?;
                    count += 1;
                }
            }

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("{} buffers saved", count));
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "reopen",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            let buffer_handle = ctx.current_buffer_handle()?;
            ctx.assert_can_discard_buffer(buffer_handle)?;
            let buffer = ctx
                .editor
                .buffers
                .get_mut(buffer_handle)
                .ok_or(CommandError::NoBufferOpened)?;

            buffer
                .discard_and_reload_from_file(&mut ctx.editor.word_database, &mut ctx.editor.events)
                .map_err(CommandError::IoError)?;

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .str("buffer reopened");
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "reopen-all",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            ctx.assert_can_discard_all_buffers()?;
            let mut count = 0;
            for buffer in ctx.editor.buffers.iter_mut() {
                buffer
                    .discard_and_reload_from_file(
                        &mut ctx.editor.word_database,
                        &mut ctx.editor.events,
                    )
                    .map_err(CommandError::IoError)?;
                count += 1;
            }

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("{} buffers reopened", count));
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "close",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            let buffer_handle = ctx.current_buffer_handle()?;
            ctx.assert_can_discard_buffer(buffer_handle)?;
            ctx.editor
                .buffers
                .defer_remove(buffer_handle, &mut ctx.editor.events);

            let clients = &mut *ctx.clients;
            if let Some(client) = ctx.client_handle.and_then(|h| clients.get_mut(h)) {
                client.set_buffer_view_handle(
                    client.previous_buffer_view_handle(),
                    &mut ctx.editor.events,
                );
            }

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .str("buffer closed");

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "close-all",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            ctx.assert_can_discard_all_buffers()?;
            let mut count = 0;
            for buffer in ctx.editor.buffers.iter() {
                ctx.editor
                    .buffers
                    .defer_remove(buffer.handle(), &mut ctx.editor.events);
                count += 1;
            }

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("{} buffers closed", count));
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "config",
        completions: &[(CompletionSource::Custom(CONFIG_NAMES))],
        func: |ctx| {
            let key = ctx.args.next()?;
            let value = ctx.args.try_next();
            ctx.args.assert_empty()?;

            match value {
                Some(value) => match ctx.editor.config.parse_config(key, value) {
                    Ok(()) => Ok(None),
                    Err(error) => Err(CommandError::ConfigError(error)),
                },
                None => match ctx.editor.config.display_config(key) {
                    Some(display) => {
                        ctx.editor
                            .status_bar
                            .write(MessageKind::Info)
                            .fmt(format_args!("{}", display));
                        Ok(None)
                    }
                    None => Err(CommandError::ConfigError(ParseConfigError::NoSuchConfig)),
                },
            }
        },
    },
    BuiltinCommand {
        name: "color",
        completions: &[CompletionSource::Custom(THEME_COLOR_NAMES)],
        func: |ctx| {
            let key = ctx.args.next()?;
            let value = ctx.args.try_next();
            ctx.args.assert_empty()?;

            let color = ctx
                .editor
                .theme
                .color_from_name(key)
                .ok_or(CommandError::NoSuchColor)?;

            match value {
                Some(value) => {
                    let encoded =
                        u32::from_str_radix(value, 16).map_err(|_| CommandError::NoSuchColor)?;
                    *color = Color::from_u32(encoded);
                }
                None => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Info)
                    .fmt(format_args!("0x{:0<6x}", color.into_u32())),
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "map-normal",
        completions: &[],
        func: |ctx| {
            map(ctx, ModeKind::Normal)?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "map-insert",
        completions: &[],
        func: |ctx| {
            map(ctx, ModeKind::Insert)?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "map-command",
        completions: &[],
        func: |ctx| {
            map(ctx, ModeKind::Command)?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "map-readline",
        completions: &[],
        func: |ctx| {
            map(ctx, ModeKind::Command)?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "map-picker",
        completions: &[],
        func: |ctx| {
            map(ctx, ModeKind::Picker)?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-stop",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            let buffer_handle = ctx.current_buffer_handle()?;
            match find_lsp_client_for_buffer(ctx.editor, buffer_handle) {
                Some(client) => ctx.editor.lsp.stop(ctx.platform, client),
                None => ctx.editor.lsp.stop_all(ctx.platform),
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-stop-all",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            ctx.editor.lsp.stop_all(ctx.platform);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-hover",
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_empty()?;
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.hover(editor, platform, buffer_handle, cursor.position)
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-definition",
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_empty()?;
            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.definition(
                    editor,
                    platform,
                    buffer_handle,
                    cursor.position,
                    client_handle,
                )
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-references",
        completions: &[],
        func: |mut ctx| {
            let context_len = 2;
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;

            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.references(
                    editor,
                    platform,
                    buffer_handle,
                    cursor.position,
                    context_len,
                    false,
                    client_handle,
                )
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-rename",
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;

            access_lsp(
                &mut ctx,
                buffer_handle,
                |editor, platform, clients, client| {
                    client.rename(
                        editor,
                        platform,
                        clients,
                        client_handle,
                        buffer_handle,
                        cursor.position,
                    )
                },
            )?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-code-action",
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;

            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.code_action(
                    editor,
                    platform,
                    client_handle,
                    buffer_handle,
                    cursor.to_range(),
                )
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-document-symbols",
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let view_handle = ctx.current_buffer_view_handle()?;
            let buffer_view = ctx
                .editor
                .buffer_views
                .get(view_handle)
                .ok_or(CommandError::NoBufferOpened)?;
            let buffer_handle = buffer_view.buffer_handle;

            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.document_symbols(editor, platform, client_handle, view_handle)
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-workspace-symbols",
        completions: &[],
        func: |mut ctx| {
            let query = ctx.args.try_next().unwrap_or("");
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let buffer_handle = ctx.current_buffer_handle()?;

            let query = ctx.editor.string_pool.acquire_with(query);
            let result = access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.workspace_symbols(editor, platform, client_handle, &query)
            });
            ctx.editor.string_pool.release(query);

            result?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-format",
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_empty()?;
            let buffer_handle = ctx.current_buffer_handle()?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.formatting(editor, platform, buffer_handle)
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-debug",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            let mut output = ctx.editor.status_bar.write(MessageKind::Info);
            for client in ctx.editor.lsp.clients() {
                output.fmt(format_args!(
                    "handle [{}] log buffer handle: {:?}",
                    client.handle(),
                    client.log_buffer_handle,
                ));
            }
            output.fmt(format_args!(
                "\nbuffer count: {}",
                ctx.editor.buffers.iter().count()
            ));
            Ok(None)
        },
    },
];

fn map(ctx: &mut CommandContext, mode: ModeKind) -> Result<(), CommandError> {
    let from = ctx.args.next()?;
    let to = ctx.args.next()?;
    ctx.args.assert_empty()?;

    ctx.editor
        .keymaps
        .parse_and_map(mode, from, to)
        .map_err(CommandError::KeyMapError)
}

fn current_buffer_and_main_cursor<'state, 'command>(
    ctx: &CommandContext<'state, 'command>,
) -> Result<(BufferHandle, Cursor), CommandError> {
    let view_handle = ctx.current_buffer_view_handle()?;
    let buffer_view = ctx
        .editor
        .buffer_views
        .get(view_handle)
        .ok_or(CommandError::NoBufferOpened)?;

    let buffer_handle = buffer_view.buffer_handle;
    let cursor = buffer_view.cursors.main_cursor().clone();
    Ok((buffer_handle, cursor))
}

fn find_lsp_client_for_buffer(
    editor: &Editor,
    buffer_handle: BufferHandle,
) -> Option<lsp::ClientHandle> {
    let buffer_path = editor.buffers.get(buffer_handle)?.path.to_str()?;
    let client = editor.lsp.clients().find(|c| c.handles_path(buffer_path))?;
    Some(client.handle())
}

fn access_lsp<'command, A>(
    ctx: &mut CommandContext,
    buffer_handle: BufferHandle,
    accessor: A,
) -> Result<(), CommandError>
where
    A: FnOnce(&mut Editor, &mut Platform, &mut ClientManager, &mut lsp::Client),
{
    let editor = &mut *ctx.editor;
    let platform = &mut *ctx.platform;
    let clients = &mut *ctx.clients;
    match find_lsp_client_for_buffer(editor, buffer_handle).and_then(|h| {
        lsp::ClientManager::access(editor, h, |e, c| accessor(e, platform, clients, c))
    }) {
        Some(()) => Ok(()),
        None => Err(CommandError::LspServerNotRunning),
    }
}

