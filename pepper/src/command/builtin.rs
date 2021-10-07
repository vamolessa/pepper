use std::path::Path;

use crate::{
    buffer::{parse_path_and_position, BufferProperties},
    buffer_position::BufferPosition,
    command::{CommandContext, CommandError, CommandManager, CompletionSource},
    config::{ParseConfigError, CONFIG_NAMES},
    cursor::Cursor,
    editor::EditorControlFlow,
    editor_utils::MessageKind,
    help,
    mode::{picker, ModeContext, ModeKind},
    syntax::TokenKind,
    theme::{Color, THEME_COLOR_NAMES},
};

pub fn register_commands(commands: &mut CommandManager) {
    let mut r = |name, completions, command_fn| {
        commands.register_command(None, name, completions, command_fn);
    };

    static HELP_COMPLETIONS: &[CompletionSource] = &[CompletionSource::Commands];
    r("help", HELP_COMPLETIONS, |ctx| {
        let keyword = ctx.args.try_next();
        ctx.args.assert_empty()?;

        let client_handle = ctx.client_handle()?;
        let (path, position) = match keyword.and_then(help::search) {
            Some((path, position)) => (path, position),
            None => (help::main_help_name(), BufferPosition::zero()),
        };

        let mut buffer_path = ctx.editor.string_pool.acquire();
        buffer_path.push_str(help::HELP_PREFIX);
        buffer_path.push_str(path);
        match ctx.editor.buffer_view_handle_from_path(
            client_handle,
            Path::new(&buffer_path),
            BufferProperties::log(),
            true,
        ) {
            Ok(handle) => {
                let client = ctx.clients.get_mut(client_handle);
                client.set_buffer_view_handle(
                    Some(handle),
                    &ctx.editor.buffer_views,
                    &mut ctx.editor.events,
                );
                client.scroll.0 = 0;
                client.scroll.1 = position.line_index.saturating_sub((client.height / 2) as _);

                let mut cursors = ctx.editor.buffer_views.get_mut(handle).cursors.mut_guard();
                cursors.clear();
                cursors.add(Cursor {
                    anchor: position,
                    position,
                });
            }
            Err(error) => ctx
                .editor
                .status_bar
                .write(MessageKind::Error)
                .fmt(format_args!("{}", error)),
        }
        ctx.editor.string_pool.release(buffer_path);

        Ok(())
    });

    r("quit", &[], |ctx| {
        ctx.args.assert_empty()?;
        if ctx.clients.iter().count() == 1 {
            ctx.assert_can_discard_all_buffers()?;
        }
        ctx.flow = EditorControlFlow::Quit;
        Ok(())
    });

    r("quit-all", &[], |ctx| {
        ctx.args.assert_empty()?;
        ctx.assert_can_discard_all_buffers()?;
        ctx.flow = EditorControlFlow::QuitAll;
        Ok(())
    });

    r("open", &[CompletionSource::Files], |ctx| {
        let path = ctx.args.next()?;
        ctx.args.assert_empty()?;

        let client_handle = ctx.client_handle()?;
        let (path, position) = parse_path_and_position(path);

        let path = Path::new(&path);
        match ctx.editor.buffer_view_handle_from_path(
            client_handle,
            Path::new(path),
            BufferProperties::text(),
            true,
        ) {
            Ok(handle) => {
                let client = ctx.clients.get_mut(client_handle);
                client.set_buffer_view_handle(
                    Some(handle),
                    &ctx.editor.buffer_views,
                    &mut ctx.editor.events,
                );

                if let Some(position) = position {
                    let mut cursors = ctx.editor.buffer_views.get_mut(handle).cursors.mut_guard();
                    cursors.clear();
                    cursors.add(Cursor {
                        anchor: position,
                        position,
                    });
                }
            }
            Err(error) => ctx
                .editor
                .status_bar
                .write(MessageKind::Error)
                .fmt(format_args!("{}", error)),
        }

        Ok(())
    });

    r("save", &[CompletionSource::Files], |ctx| {
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
        Ok(())
    });

    r("save-all", &[], |ctx| {
        ctx.args.assert_empty()?;

        let mut count = 0;
        for buffer in ctx.editor.buffers.iter_mut() {
            if buffer.properties.can_save {
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
        Ok(())
    });

    r("reopen", &[], |ctx| {
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
        Ok(())
    });

    r("reopen-all", &[], |ctx| {
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
        Ok(())
    });

    r("close", &[], |ctx| {
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

        Ok(())
    });

    r("close-all", &[], |ctx| {
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
        Ok(())
    });

    static CONFIG_COMPLETIONS: &[CompletionSource] = &[CompletionSource::Custom(CONFIG_NAMES)];
    r("config", CONFIG_COMPLETIONS, |ctx| {
        let key = ctx.args.next()?;
        let value = ctx.args.try_next();
        ctx.args.assert_empty()?;

        match value {
            Some(value) => match ctx.editor.config.parse_config(key, value) {
                Ok(()) => Ok(()),
                Err(error) => Err(CommandError::ConfigError(error)),
            },
            None => match ctx.editor.config.display_config(key) {
                Some(display) => {
                    ctx.editor
                        .status_bar
                        .write(MessageKind::Info)
                        .fmt(format_args!("{}", display));
                    Ok(())
                }
                None => Err(CommandError::ConfigError(ParseConfigError::NoSuchConfig)),
            },
        }
    });

    static COLOR_COMPLETIONS: &[CompletionSource] = &[CompletionSource::Custom(THEME_COLOR_NAMES)];
    r("color", COLOR_COMPLETIONS, |ctx| {
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

        Ok(())
    });

    r("map-normal", &[], |ctx| map(ctx, ModeKind::Normal));
    r("map-insert", &[], |ctx| map(ctx, ModeKind::Normal));
    r("map-command", &[], |ctx| map(ctx, ModeKind::Normal));
    r("map-readline", &[], |ctx| map(ctx, ModeKind::Normal));
    r("map-picker", &[], |ctx| map(ctx, ModeKind::Normal));

    static ALIAS_COMPLETIONS: &[CompletionSource] =
        &[CompletionSource::Custom(&[]), CompletionSource::Commands];
    r("alias", ALIAS_COMPLETIONS, |ctx| {
        let from = ctx.args.next()?;
        let to = ctx.args.next()?;
        ctx.args.assert_empty()?;
        ctx.editor.commands.aliases.add(from, to);
        Ok(())
    });

    r("syntax", &[], |ctx| {
        let glob = ctx.args.next()?;
        ctx.args.assert_empty()?;
        match ctx.editor.syntaxes.set_current_from_glob(glob) {
            Ok(()) => Ok(()),
            Err(error) => Err(CommandError::InvalidGlob(error)),
        }
    });

    r("syntax-keywords", &[], |ctx| {
        syntax_pattern(ctx, TokenKind::Keyword)
    });
    r("syntax-types", &[], |ctx| {
        syntax_pattern(ctx, TokenKind::Type)
    });
    r("syntax-symbols", &[], |ctx| {
        syntax_pattern(ctx, TokenKind::Symbol)
    });
    r("syntax-literals", &[], |ctx| {
        syntax_pattern(ctx, TokenKind::Literal)
    });
    r("syntax-strings", &[], |ctx| {
        syntax_pattern(ctx, TokenKind::String)
    });
    r("syntax-comments", &[], |ctx| {
        syntax_pattern(ctx, TokenKind::Comment)
    });
    r("syntax-texts", &[], |ctx| {
        syntax_pattern(ctx, TokenKind::Text)
    });

    r("find-file", &[], |ctx| {
        let command = ctx.args.next()?;
        ctx.args.assert_empty()?;
        let client_handle = ctx.client_handle()?;
        let mut ctx = ModeContext {
            editor: ctx.editor,
            platform: ctx.platform,
            clients: ctx.clients,
            client_handle,
        };
        picker::find_file::enter_mode(&mut ctx, command);
        Ok(())
    });

    r("pid", &[], |ctx| {
        ctx.args.assert_empty()?;
        ctx.editor
            .status_bar
            .write(MessageKind::Info)
            .fmt(format_args!("{}", std::process::id()));
        Ok(())
    });
}

fn map(ctx: &mut CommandContext, mode: ModeKind) -> Result<(), CommandError> {
    let from = ctx.args.next()?;
    let to = ctx.args.next()?;
    ctx.args.assert_empty()?;

    match ctx.editor.keymaps.parse_and_map(mode, from, to) {
        Ok(()) => Ok(()),
        Err(error) => Err(CommandError::KeyMapError(error)),
    }
}

fn syntax_pattern(ctx: &mut CommandContext, token_kind: TokenKind) -> Result<(), CommandError> {
    let pattern = ctx.args.next()?;
    ctx.args.assert_empty()?;
    match ctx
        .editor
        .syntaxes
        .get_current()
        .set_rule(token_kind, pattern)
    {
        Ok(()) => Ok(()),
        Err(error) => Err(CommandError::PatternError(error)),
    }
}

