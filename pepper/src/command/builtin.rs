use std::path::Path;

use crate::{
    buffer::{parse_path_and_position, BufferProperties},
    buffer_position::BufferPosition,
    client::ViewAnchor,
    command::{CommandError, CommandIO, CommandManager, CompletionSource},
    config::{ParseConfigError, CONFIG_NAMES},
    cursor::Cursor,
    editor::{EditorContext, EditorFlow},
    editor_utils::MessageKind,
    help,
    mode::{picker, read_line, ModeKind},
    syntax::TokenKind,
    theme::{Color, THEME_COLOR_NAMES},
};

pub fn register_commands(commands: &mut CommandManager) {
    let mut r = |name, completions, command_fn| {
        commands.register_command(None, name, completions, command_fn);
    };

    static HELP_COMPLETIONS: &[CompletionSource] = &[CompletionSource::Commands];
    r("help", HELP_COMPLETIONS, |ctx, io| {
        let keyword = io.args.try_next();
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;
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
            BufferProperties::scratch(),
            true,
        ) {
            Ok(handle) => {
                let client = ctx.clients.get_mut(client_handle);
                client.set_buffer_view_handle(Some(handle), &ctx.editor.buffer_views);
                client.set_view_anchor(&ctx.editor, ViewAnchor::Center);

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

    r("print", &[], |ctx, io| {
        let mut write = ctx.editor.status_bar.write(MessageKind::Info);
        if let Some(arg) = io.args.try_next() {
            write.str(arg);
        }
        while let Some(arg) = io.args.try_next() {
            write.str("\n");
            write.str(arg);
        }
        Ok(())
    });

    r("quit", &[], |ctx, io| {
        io.args.assert_empty()?;
        if ctx.clients.iter().count() == 1 {
            io.assert_can_discard_all_buffers(ctx)?;
        }
        io.flow = EditorFlow::Quit;
        Ok(())
    });

    r("quit-all", &[], |ctx, io| {
        io.args.assert_empty()?;
        io.assert_can_discard_all_buffers(ctx)?;
        io.flow = EditorFlow::QuitAll;
        Ok(())
    });

    r("open", &[CompletionSource::Files], |ctx, io| {
        let path = io.args.next()?;

        let mut properties = BufferProperties::text();
        while let Some(property) = io.args.try_next() {
            match property {
                "text" => properties = BufferProperties::text(),
                "scratch" => properties = BufferProperties::scratch(),
                "history-enabled" => properties.history_enabled = true,
                "history-disabled" => properties.history_enabled = false,
                "saving-enabled" => properties.saving_enabled = true,
                "saving-disabled" => properties.saving_enabled = false,
                "word-database-enabled" => properties.word_database_enabled = true,
                "word-database-disabled" => properties.word_database_enabled = false,
                _ => return Err(CommandError::NoSuchBufferProperty),
            }
        }

        let client_handle = io.client_handle()?;
        let (path, position) = parse_path_and_position(path);

        let path = Path::new(&path);
        match ctx.editor.buffer_view_handle_from_path(
            client_handle,
            Path::new(path),
            properties,
            true,
        ) {
            Ok(handle) => {
                let client = ctx.clients.get_mut(client_handle);
                client.set_buffer_view_handle(Some(handle), &ctx.editor.buffer_views);

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

    r("save", &[CompletionSource::Files], |ctx, io| {
        let path = io.args.try_next().map(|p| Path::new(p));
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
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

    r("save-all", &[], |ctx, io| {
        io.args.assert_empty()?;

        let mut count = 0;
        for buffer in ctx.editor.buffers.iter_mut() {
            if buffer.properties.saving_enabled {
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

    r("reopen", &[], |ctx, io| {
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
        io.assert_can_discard_buffer(ctx, buffer_handle)?;
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

    r("reopen-all", &[], |ctx, io| {
        io.args.assert_empty()?;

        io.assert_can_discard_all_buffers(ctx)?;
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

    r("close", &[], |ctx, io| {
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
        io.assert_can_discard_buffer(ctx, buffer_handle)?;
        ctx.editor
            .buffers
            .defer_remove(buffer_handle, &mut ctx.editor.events);

        ctx.editor
            .status_bar
            .write(MessageKind::Info)
            .str("buffer closed");

        Ok(())
    });

    r("close-all", &[], |ctx, io| {
        io.args.assert_empty()?;

        io.assert_can_discard_all_buffers(ctx)?;
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
    r("config", CONFIG_COMPLETIONS, |ctx, io| {
        let key = io.args.next()?;
        let value = io.args.try_next();
        io.args.assert_empty()?;

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
    r("color", COLOR_COMPLETIONS, |ctx, io| {
        let key = io.args.next()?;
        let value = io.args.try_next();
        io.args.assert_empty()?;

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

    r("map-normal", &[], |ctx, io| map(ctx, io, ModeKind::Normal));
    r("map-insert", &[], |ctx, io| map(ctx, io, ModeKind::Insert));
    r("map-command", &[], |ctx, io| {
        map(ctx, io, ModeKind::Command)
    });
    r("map-readline", &[], |ctx, io| {
        map(ctx, io, ModeKind::ReadLine)
    });
    r("map-picker", &[], |ctx, io| map(ctx, io, ModeKind::Picker));

    static ALIAS_COMPLETIONS: &[CompletionSource] =
        &[CompletionSource::Custom(&[]), CompletionSource::Commands];
    r("alias", ALIAS_COMPLETIONS, |ctx, io| {
        let from = io.args.next()?;
        let to = io.args.next()?;
        io.args.assert_empty()?;
        ctx.editor.commands.aliases.add(from, to);
        Ok(())
    });

    r("syntax", &[], |ctx, io| {
        let glob = io.args.next()?;
        io.args.assert_empty()?;
        match ctx.editor.syntaxes.set_current_from_glob(glob) {
            Ok(()) => Ok(()),
            Err(error) => Err(CommandError::InvalidGlob(error)),
        }
    });

    r("syntax-keywords", &[], |ctx, io| {
        syntax_pattern(ctx, io, TokenKind::Keyword)
    });
    r("syntax-types", &[], |ctx, io| {
        syntax_pattern(ctx, io, TokenKind::Type)
    });
    r("syntax-symbols", &[], |ctx, io| {
        syntax_pattern(ctx, io, TokenKind::Symbol)
    });
    r("syntax-literals", &[], |ctx, io| {
        syntax_pattern(ctx, io, TokenKind::Literal)
    });
    r("syntax-strings", &[], |ctx, io| {
        syntax_pattern(ctx, io, TokenKind::String)
    });
    r("syntax-comments", &[], |ctx, io| {
        syntax_pattern(ctx, io, TokenKind::Comment)
    });
    r("syntax-texts", &[], |ctx, io| {
        syntax_pattern(ctx, io, TokenKind::Text)
    });

    r("copy-command", &[], |ctx, io| {
        let command = io.args.next()?;
        io.args.assert_empty()?;
        ctx.platform.copy_command.clear();
        ctx.platform.copy_command.push_str(command);
        Ok(())
    });

    r("paste-command", &[], |ctx, io| {
        let command = io.args.next()?;
        io.args.assert_empty()?;
        ctx.platform.paste_command.clear();
        ctx.platform.paste_command.push_str(command);
        Ok(())
    });

    r("enqueue-keys", &[], |ctx, io| {
        let keys = io.args.next()?;
        io.args.assert_empty()?;

        ctx.editor
            .buffered_keys
            .parse(keys)
            .map_err(|e| CommandError::KeyParseError(e.error))?;
        Ok(())
    });

    r(
        "on-platforms",
        &[CompletionSource::Custom(&[
            "windows", "linux", "bsd", "macos",
        ])],
        |ctx, io| {
            let current_platform = if cfg!(target_os = "windows") {
                "windows"
            } else if cfg!(target_os = "linux") {
                "linux"
            } else if cfg!(any(
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly",
            )) {
                "bsd"
            } else if cfg!(target_os = "macos") {
                "macos"
            } else {
                ""
            };

            let mut block = "";
            let mut should_execute = false;
            while let Some(arg) = io.args.try_next() {
                if should_execute {
                    block = arg;
                }

                if arg == current_platform {
                    should_execute = true;
                }
            }

            if should_execute {
                let mut command = ctx.editor.string_pool.acquire_with(block);
                CommandManager::try_eval(ctx, io.client_handle, &mut command)?;
                ctx.editor.string_pool.release(command);
            }

            Ok(())
        },
    );

    r("find-file", &[], |ctx, io| {
        let command = io.args.next()?;
        let prompt = io.args.try_next().unwrap_or("open:");
        io.args.assert_empty()?;
        picker::find_file::enter_mode(ctx, command, prompt);
        Ok(())
    });

    r("find-pattern", &[], |ctx, io| {
        let command = io.args.next()?;
        let prompt = io.args.try_next().unwrap_or("find:");
        io.args.assert_empty()?;
        read_line::find_pattern::enter_mode(ctx, command, prompt);
        Ok(())
    });

    r("pid", &[], |ctx, io| {
        io.args.assert_empty()?;
        ctx.editor
            .status_bar
            .write(MessageKind::Info)
            .fmt(format_args!("{}", std::process::id()));
        Ok(())
    });
}

fn map(ctx: &mut EditorContext, io: &mut CommandIO, mode: ModeKind) -> Result<(), CommandError> {
    let from = io.args.next()?;
    let to = io.args.next()?;
    io.args.assert_empty()?;

    match ctx.editor.keymaps.parse_and_map(mode, from, to) {
        Ok(()) => Ok(()),
        Err(error) => Err(CommandError::KeyMapError(error)),
    }
}

fn syntax_pattern(
    ctx: &mut EditorContext,
    io: &mut CommandIO,
    token_kind: TokenKind,
) -> Result<(), CommandError> {
    let pattern = io.args.next()?;
    io.args.assert_empty()?;
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

