use std::path::Path;

use crate::{
    buffer::{parse_path_and_position, BufferCapabilities, BufferHandle},
    buffer_position::BufferPosition,
    client::ClientManager,
    command::{BuiltinCommand, CommandContext, CommandError, CompletionSource},
    config::{ParseConfigError, CONFIG_NAMES},
    cursor::Cursor,
    editor::{Editor, EditorControlFlow},
    editor_utils::MessageKind,
    help, lsp,
    mode::{picker, ModeContext, ModeKind},
    platform::Platform,
    syntax::TokenKind,
    theme::{Color, THEME_COLOR_NAMES},
};

pub static COMMANDS: &[BuiltinCommand] = &[
    BuiltinCommand {
        name: "help",
        completions: &[CompletionSource::Commands],
        func: |ctx| {
            let keyword = ctx.args.try_next();
            ctx.args.assert_empty()?;

            let client_handle = ctx.client_handle()?;
            let (path, position) = match keyword.and_then(|k| help::search(k)) {
                Some((path, line_index)) => (path, BufferPosition::line_col(line_index as _, 0)),
                None => (help::main_help_path(), BufferPosition::zero()),
            };

            match ctx.editor.buffer_view_handle_from_path(
                client_handle,
                path,
                BufferCapabilities::log(),
            ) {
                Ok(handle) => {
                    {
                        let mut cursors =
                            ctx.editor.buffer_views.get_mut(handle).cursors.mut_guard();
                        cursors.clear();
                        cursors.add(Cursor {
                            anchor: position,
                            position,
                        });
                    }

                    let client = ctx.clients.get_mut(client_handle);
                    client.set_buffer_view_handle(
                        Some(handle),
                        &ctx.editor.buffer_views,
                        &mut ctx.editor.events,
                    );
                    client.scroll.0 = 0;
                    client.scroll.1 = position.line_index.saturating_sub((client.height / 2) as _);
                }
                Err(error) => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error)),
            }

            Ok(EditorControlFlow::Continue)
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
            Ok(EditorControlFlow::Quit)
        },
    },
    BuiltinCommand {
        name: "quit-all",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            ctx.assert_can_discard_all_buffers()?;
            Ok(EditorControlFlow::QuitAll)
        },
    },
    BuiltinCommand {
        name: "open",
        completions: &[CompletionSource::Files],
        func: |ctx| {
            let path = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let client_handle = ctx.client_handle()?;
            let (path, position) = parse_path_and_position(path);

            let path = ctx.editor.string_pool.acquire_with(path);
            match ctx.editor.buffer_view_handle_from_path(
                client_handle,
                Path::new(&path),
                BufferCapabilities::text(),
            ) {
                Ok(handle) => {
                    ctx.editor.string_pool.release(path);

                    if let Some(position) = position {
                        let mut cursors =
                            ctx.editor.buffer_views.get_mut(handle).cursors.mut_guard();
                        cursors.clear();
                        cursors.add(Cursor {
                            anchor: position,
                            position,
                        });
                    }

                    let client = ctx.clients.get_mut(client_handle);
                    client.set_buffer_view_handle(
                        Some(handle),
                        &ctx.editor.buffer_views,
                        &mut ctx.editor.events,
                    );
                }
                Err(error) => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error)),
            }

            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "save",
        completions: &[],
        func: |ctx| {
            let path = ctx.args.try_next().map(|p| Path::new(p));
            ctx.args.assert_empty()?;

            let buffer_handle = ctx.current_buffer_handle()?;
            let buffer = ctx.editor.buffers.get_mut(buffer_handle);

            buffer
                .write_to_file(path, &mut ctx.editor.events)
                .map_err(CommandError::BufferWriteError)?;

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("buffer saved to {:?}", &buffer.path));
            Ok(EditorControlFlow::Continue)
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
                        .write_to_file(None, &mut ctx.editor.events)
                        .map_err(CommandError::BufferWriteError)?;
                    count += 1;
                }
            }

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("{} buffers saved", count));
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "reopen",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            let buffer_handle = ctx.current_buffer_handle()?;
            ctx.assert_can_discard_buffer(buffer_handle)?;
            let buffer = ctx.editor.buffers.get_mut(buffer_handle);

            buffer
                .read_from_file(&mut ctx.editor.word_database, &mut ctx.editor.events)
                .map_err(CommandError::BufferReadError)?;

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .str("buffer reopened");
            Ok(EditorControlFlow::Continue)
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
                    .read_from_file(&mut ctx.editor.word_database, &mut ctx.editor.events)
                    .map_err(CommandError::BufferReadError)?;
                count += 1;
            }

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("{} buffers reopened", count));
            Ok(EditorControlFlow::Continue)
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

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .str("buffer closed");

            Ok(EditorControlFlow::Continue)
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
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "status",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            // TODO status command
            let client_handle = ctx.client_handle()?;
            let client = ctx.clients.get_mut(client_handle);
            client.set_buffer_view_handle(None, &ctx.editor.buffer_views, &mut ctx.editor.events);

            Ok(EditorControlFlow::Continue)
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
                    Ok(()) => Ok(EditorControlFlow::Continue),
                    Err(error) => Err(CommandError::ConfigError(error)),
                },
                None => match ctx.editor.config.display_config(key) {
                    Some(display) => {
                        ctx.editor
                            .status_bar
                            .write(MessageKind::Info)
                            .fmt(format_args!("{}", display));
                        Ok(EditorControlFlow::Continue)
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

            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "map-normal",
        completions: &[],
        func: |ctx| map(ctx, ModeKind::Normal),
    },
    BuiltinCommand {
        name: "map-insert",
        completions: &[],
        func: |ctx| map(ctx, ModeKind::Insert),
    },
    BuiltinCommand {
        name: "map-command",
        completions: &[],
        func: |ctx| map(ctx, ModeKind::Command),
    },
    BuiltinCommand {
        name: "map-readline",
        completions: &[],
        func: |ctx| map(ctx, ModeKind::Command),
    },
    BuiltinCommand {
        name: "map-picker",
        completions: &[],
        func: |ctx| map(ctx, ModeKind::Picker),
    },
    BuiltinCommand {
        name: "alias",
        completions: &[CompletionSource::Custom(&[]), CompletionSource::Commands],
        func: |ctx| {
            let from = ctx.args.next()?;
            let to = ctx.args.next()?;
            ctx.args.assert_empty()?;
            ctx.editor.commands.aliases.add(from, to);
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "syntax",
        completions: &[],
        func: |ctx| {
            let glob = ctx.args.next()?;
            ctx.args.assert_empty()?;
            match ctx.editor.syntaxes.set_current_from_glob(glob) {
                Ok(()) => Ok(EditorControlFlow::Continue),
                Err(error) => Err(CommandError::InvalidGlob(error)),
            }
        },
    },
    BuiltinCommand {
        name: "syntax-keywords",
        completions: &[],
        func: |ctx| syntax_pattern(ctx, TokenKind::Keyword),
    },
    BuiltinCommand {
        name: "syntax-types",
        completions: &[],
        func: |ctx| syntax_pattern(ctx, TokenKind::Type),
    },
    BuiltinCommand {
        name: "syntax-symbols",
        completions: &[],
        func: |ctx| syntax_pattern(ctx, TokenKind::Symbol),
    },
    BuiltinCommand {
        name: "syntax-literals",
        completions: &[],
        func: |ctx| syntax_pattern(ctx, TokenKind::Literal),
    },
    BuiltinCommand {
        name: "syntax-strings",
        completions: &[],
        func: |ctx| syntax_pattern(ctx, TokenKind::String),
    },
    BuiltinCommand {
        name: "syntax-comments",
        completions: &[],
        func: |ctx| syntax_pattern(ctx, TokenKind::Comment),
    },
    BuiltinCommand {
        name: "syntax-texts",
        completions: &[],
        func: |ctx| syntax_pattern(ctx, TokenKind::Text),
    },
    BuiltinCommand {
        name: "find-file",
        completions: &[],
        func: |ctx| {
            let command = ctx.args.next()?;
            ctx.args.assert_empty()?;
            if let Some(client_handle) = ctx.client_handle {
                let mut ctx = ModeContext {
                    editor: ctx.editor,
                    platform: ctx.platform,
                    clients: ctx.clients,
                    client_handle,
                };
                picker::find_file::enter_mode(&mut ctx, command);
            }
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "pid",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("{}", std::process::id()));
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp",
        completions: &[],
        func: |ctx| {
            let command = ctx.args.next()?;
            let glob = ctx.args.next()?;
            let log_path = ctx.args.try_next();
            ctx.args.assert_empty()?;

            match ctx.editor.lsp.add_recipe(glob, command, None, log_path) {
                Ok(()) => Ok(EditorControlFlow::Continue),
                Err(error) => Err(CommandError::InvalidGlob(error)),
            }
        },
    },
    BuiltinCommand {
        name: "lsp-open-log",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            let client_handle = ctx.client_handle()?;
            let buffer_handle = ctx.current_buffer_handle()?;
            access_lsp(
                ctx,
                buffer_handle,
                |editor, _, clients, client| match client.log_file_path() {
                    Some(path) => {
                        match editor.buffer_view_handle_from_path(
                            client_handle,
                            Path::new(path),
                            BufferCapabilities::log(),
                        ) {
                            Ok(buffer_view_handle) => {
                                let client = clients.get_mut(client_handle);
                                client.set_buffer_view_handle(
                                    Some(buffer_view_handle),
                                    &editor.buffer_views,
                                    &mut editor.events,
                                );
                            }
                            Err(error) => editor
                                .status_bar
                                .write(MessageKind::Error)
                                .fmt(format_args!("{}", error)),
                        }
                        Ok(())
                    }
                    None => Err(CommandError::LspServerNotLogging),
                },
            )??;
            Ok(EditorControlFlow::Continue)
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
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-stop-all",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            ctx.editor.lsp.stop_all(ctx.platform);
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-hover",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
            access_lsp(ctx, buffer_handle, |editor, platform, _, client| {
                client.hover(editor, platform, buffer_handle, cursor.position)
            })?;
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-definition",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            let client_handle = ctx.client_handle()?;
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
            access_lsp(ctx, buffer_handle, |editor, platform, _, client| {
                client.definition(
                    editor,
                    platform,
                    buffer_handle,
                    cursor.position,
                    client_handle,
                )
            })?;
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-declaration",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            let client_handle = ctx.client_handle()?;
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
            access_lsp(ctx, buffer_handle, |editor, platform, _, client| {
                client.declaration(
                    editor,
                    platform,
                    buffer_handle,
                    cursor.position,
                    client_handle,
                )
            })?;
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-implementation",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            let client_handle = ctx.client_handle()?;
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
            access_lsp(ctx, buffer_handle, |editor, platform, _, client| {
                client.implementation(
                    editor,
                    platform,
                    buffer_handle,
                    cursor.position,
                    client_handle,
                )
            })?;
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-references",
        completions: &[],
        func: |ctx| {
            let context_len = 2;
            ctx.args.assert_empty()?;

            let client_handle = ctx.client_handle()?;
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;

            access_lsp(ctx, buffer_handle, |editor, platform, _, client| {
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
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-rename",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            let client_handle = ctx.client_handle()?;
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;

            access_lsp(ctx, buffer_handle, |editor, platform, clients, client| {
                client.rename(
                    editor,
                    platform,
                    clients,
                    client_handle,
                    buffer_handle,
                    cursor.position,
                )
            })?;
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-code-action",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            let client_handle = ctx.client_handle()?;
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;

            access_lsp(ctx, buffer_handle, |editor, platform, _, client| {
                client.code_action(
                    editor,
                    platform,
                    client_handle,
                    buffer_handle,
                    cursor.to_range(),
                )
            })?;
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-document-symbols",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;

            let client_handle = ctx.client_handle()?;
            let view_handle = ctx.current_buffer_view_handle()?;
            let buffer_handle = ctx.editor.buffer_views.get(view_handle).buffer_handle;

            access_lsp(ctx, buffer_handle, |editor, platform, _, client| {
                client.document_symbols(editor, platform, client_handle, view_handle)
            })?;
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-workspace-symbols",
        completions: &[],
        func: |ctx| {
            let query = ctx.args.try_next().unwrap_or("");
            ctx.args.assert_empty()?;

            let client_handle = ctx.client_handle()?;
            let buffer_handle = ctx.current_buffer_handle()?;

            access_lsp(ctx, buffer_handle, |editor, platform, _, client| {
                client.workspace_symbols(editor, platform, client_handle, query)
            })?;
            Ok(EditorControlFlow::Continue)
        },
    },
    BuiltinCommand {
        name: "lsp-format",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_empty()?;
            let buffer_handle = ctx.current_buffer_handle()?;
            access_lsp(ctx, buffer_handle, |editor, platform, _, client| {
                client.formatting(editor, platform, buffer_handle)
            })?;
            Ok(EditorControlFlow::Continue)
        },
    },
];

fn map(ctx: &mut CommandContext, mode: ModeKind) -> Result<EditorControlFlow, CommandError> {
    let from = ctx.args.next()?;
    let to = ctx.args.next()?;
    ctx.args.assert_empty()?;

    match ctx.editor.keymaps.parse_and_map(mode, from, to) {
        Ok(()) => Ok(EditorControlFlow::Continue),
        Err(error) => Err(CommandError::KeyMapError(error)),
    }
}

fn syntax_pattern(
    ctx: &mut CommandContext,
    token_kind: TokenKind,
) -> Result<EditorControlFlow, CommandError> {
    let pattern = ctx.args.next()?;
    ctx.args.assert_empty()?;
    match ctx
        .editor
        .syntaxes
        .get_current()
        .set_rule(token_kind, pattern)
    {
        Ok(()) => Ok(EditorControlFlow::Continue),
        Err(error) => Err(CommandError::PatternError(error)),
    }
}

fn current_buffer_and_main_cursor(
    ctx: &CommandContext,
) -> Result<(BufferHandle, Cursor), CommandError> {
    let view_handle = ctx.current_buffer_view_handle()?;
    let buffer_view = ctx.editor.buffer_views.get(view_handle);

    let buffer_handle = buffer_view.buffer_handle;
    let cursor = *buffer_view.cursors.main_cursor();
    Ok((buffer_handle, cursor))
}

fn find_lsp_client_for_buffer(
    editor: &Editor,
    buffer_handle: BufferHandle,
) -> Option<lsp::ClientHandle> {
    let buffer_path = editor.buffers.get(buffer_handle).path.to_str()?;
    let client = editor.lsp.clients().find(|c| c.handles_path(buffer_path))?;
    Some(client.handle())
}

fn access_lsp<A, R>(
    ctx: &mut CommandContext,
    buffer_handle: BufferHandle,
    accessor: A,
) -> Result<R, CommandError>
where
    A: FnOnce(&mut Editor, &mut Platform, &mut ClientManager, &mut lsp::Client) -> R,
{
    let editor = &mut *ctx.editor;
    let platform = &mut *ctx.platform;
    let clients = &mut *ctx.clients;
    match find_lsp_client_for_buffer(editor, buffer_handle).and_then(|h| {
        lsp::ClientManager::access(editor, h, |e, c| accessor(e, platform, clients, c))
    }) {
        Some(result) => Ok(result),
        None => Err(CommandError::LspServerNotRunning),
    }
}
