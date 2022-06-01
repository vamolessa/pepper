use std::{env, path::Path, process::Stdio};

use crate::{
    buffer::{parse_path_and_position, BufferProperties, BufferReadError, BufferWriteError},
    buffer_position::BufferPosition,
    client::ViewAnchor,
    command::{CommandError, CommandManager, CompletionSource},
    config::{ParseConfigError, CONFIG_NAMES},
    cursor::Cursor,
    editor::EditorFlow,
    editor_utils::{parse_process_command, MessageKind, RegisterKey},
    help,
    mode::{picker, read_line, ModeKind},
    platform::{PlatformRequest, ProcessTag},
    syntax::TokenKind,
    theme::{Color, THEME_COLOR_NAMES},
    word_database::WordIndicesIter,
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
        let mut path = io.args.next()?;

        let mut properties = BufferProperties::text();
        while let Some(arg) = io.args.try_next() {
            match path {
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
            path = arg;
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
            match buffer.write_to_file(None, &mut ctx.editor.events) {
                Ok(()) => count += 1,
                Err(BufferWriteError::SavingDisabled) => (),
                Err(error) => return Err(CommandError::BufferWriteError(error)),
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
        let mut all_files_found = true;
        for buffer in ctx.editor.buffers.iter_mut() {
            match buffer.read_from_file(&mut ctx.editor.word_database, &mut ctx.editor.events) {
                Ok(()) => count += 1,
                Err(BufferReadError::FileNotFound) => all_files_found = true,
                Err(error) => return Err(CommandError::BufferReadError(error)),
            }
        }

        if count == 0 && all_files_found {
            return Err(CommandError::BufferReadError(BufferReadError::FileNotFound));
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
                    u32::from_str_radix(value, 16).map_err(|_| CommandError::InvalidColorValue)?;
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

    static MAP_COMPLETIONS: &[CompletionSource] = &[CompletionSource::Custom(&[
        "normal", "insert", "command", "readline", "picker",
    ])];
    r("map", MAP_COMPLETIONS, |ctx, io| {
        let mode = io.args.next()?;
        let from = io.args.next()?;
        let to = io.args.next()?;
        io.args.assert_empty()?;

        let mode = match mode {
            "normal" => ModeKind::Normal,
            "insert" => ModeKind::Insert,
            "command" => ModeKind::Command,
            "readline" => ModeKind::ReadLine,
            "picker" => ModeKind::Picker,
            _ => return Err(CommandError::InvalidModeKind),
        };

        match ctx.editor.keymaps.parse_and_map(mode, from, to) {
            Ok(()) => Ok(()),
            Err(error) => Err(CommandError::KeyMapError(error)),
        }
    });

    static SYNTAX_COMPLETIONS: &[CompletionSource] = &[CompletionSource::Custom(&[
        "keywords", "types", "symbols", "literals", "strings", "comments", "texts",
    ])];
    r("syntax", SYNTAX_COMPLETIONS, |ctx, io| {
        let arg = io.args.next()?;
        let pattern = io.args.try_next();
        io.args.assert_empty()?;

        let pattern = match pattern {
            Some(pattern) => pattern,
            None => match ctx.editor.syntaxes.set_current_from_glob(arg) {
                Ok(()) => return Ok(()),
                Err(error) => return Err(CommandError::InvalidGlob(error)),
            },
        };

        let token_kind = match arg {
            "keywords" => TokenKind::Keyword,
            "types" => TokenKind::Type,
            "symbols" => TokenKind::Symbol,
            "literals" => TokenKind::Literal,
            "strings" => TokenKind::String,
            "comments" => TokenKind::Comment,
            "texts" => TokenKind::Text,
            _ => return Err(CommandError::InvalidTokenKind),
        };

        match ctx
            .editor
            .syntaxes
            .get_current()
            .set_rule(token_kind, pattern)
        {
            Ok(()) => Ok(()),
            Err(error) => Err(CommandError::PatternError(error)),
        }
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

    r("set-register", &[], |ctx, io| {
        let key = io.args.next()?;
        let value = io.args.next()?;
        io.args.assert_empty()?;

        let key = RegisterKey::from_str(key).ok_or(CommandError::InvalidRegisterKey)?;
        let register = ctx.editor.registers.get_mut(key);
        register.clear();
        register.push_str(value);
        Ok(())
    });

    r("set-env", &[], |_, io| {
        let key = io.args.next()?;
        let value = io.args.next()?;
        io.args.assert_empty()?;

        if key.is_empty() || key.contains('=') {
            return Err(CommandError::InvalidEnvironmentVariable);
        }

        env::set_var(key, value);
        Ok(())
    });

    r("readline", &[], |ctx, io| {
        let arg = io.args.next()?;
        let (prompt, continuation) = match io.args.try_next() {
            Some(continuation) => (arg, continuation),
            None => ("readline:", arg),
        };
        io.args.assert_empty()?;
        read_line::custom::enter_mode(ctx, continuation, prompt);
        Ok(())
    });

    r("pick", &[], |ctx, io| {
        let arg = io.args.next()?;
        let (prompt, continuation) = match io.args.try_next() {
            Some(continuation) => (arg, continuation),
            None => ("pick:", arg),
        };
        io.args.assert_empty()?;
        picker::custom::enter_mode(ctx, continuation, prompt);
        Ok(())
    });

    r("picker-entries", &[], |ctx, io| {
        ctx.editor.picker.clear();
        while let Some(arg) = io.args.try_next() {
            ctx.editor.picker.add_custom_entry(arg);
        }
        ctx.editor
            .picker
            .filter(WordIndicesIter::empty(), ctx.editor.read_line.input());
        Ok(())
    });

    r("picker-entries-from-lines", &[], |ctx, io| {
        let command = io.args.next()?;
        io.args.assert_empty()?;

        ctx.editor.picker.clear();

        match parse_process_command(command) {
            Some(mut command) => {
                command.stdin(Stdio::null());
                command.stdout(Stdio::piped());
                command.stderr(Stdio::null());

                ctx.platform
                    .requests
                    .enqueue(PlatformRequest::SpawnProcess {
                        tag: ProcessTag::PickerEntries,
                        command,
                        buf_len: 4 * 1024,
                    });
            }
            None => {
                ctx.editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("invalid command '{}'", command));
            }
        }

        Ok(())
    });

    r("spawn", &[], |ctx, io| {
        let command = io.args.next()?;
        io.args.assert_empty()?;

        if let Some(mut command) = parse_process_command(command) {
            command.stdin(Stdio::null());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::null());

            ctx.platform
                .requests
                .enqueue(PlatformRequest::SpawnProcess {
                    tag: ProcessTag::Ignored,
                    command,
                    buf_len: 4 * 1024,
                });
        }

        Ok(())
    });

    r("replace-with-output", &[], |ctx, io| {
        let command = io.args.next()?;
        io.args.assert_empty()?;

        let buffer_view_handle = io.current_buffer_view_handle(ctx)?;
        let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);

        for cursor in buffer_view.cursors[..].iter().rev() {
            let command = match parse_process_command(command) {
                Some(command) => command,
                None => continue,
            };

            let range = cursor.to_range();
            let stdin = if range.from == range.to {
                None
            } else {
                let mut buf = ctx.platform.buf_pool.acquire();
                let write = buf.write();

                let content = ctx.editor.buffers.get(buffer_view.buffer_handle).content();
                for text in content.text_range(range) {
                    write.extend_from_slice(text.as_bytes());
                }

                Some(buf)
            };

            ctx.editor.buffers.spawn_insert_process(
                &mut ctx.platform,
                command,
                buffer_view.buffer_handle,
                cursor.position,
                stdin,
            );
        }

        buffer_view.delete_text_in_cursor_ranges(
            &mut ctx.editor.buffers,
            &mut ctx.editor.word_database,
            &mut ctx.editor.events,
        );

        Ok(())
    });

    r("command", &[], |ctx, io| {
        let name = io.args.next()?;
        let source = io.args.next()?;
        io.args.assert_empty()?;
        ctx.editor.commands.register_macro(name, source)
    });

    static EVAL_COMPLETIONS: &[CompletionSource] = &[
        CompletionSource::Custom(&["on"]),
        CompletionSource::Custom(&["windows", "linux", "bsd", "macos"]),
    ];
    r("eval", EVAL_COMPLETIONS, |ctx, io| {
        let mut continuation = io.args.next()?;

        let mut should_execute = true;
        if continuation == "on" {
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

            should_execute = false;
            while let Some(arg) = io.args.try_next() {
                continuation = arg;
                should_execute = should_execute || arg == current_platform;
            }
        }

        io.args.assert_empty()?;

        if !should_execute {
            return Ok(());
        }

        io.args.assert_empty()?;
        match CommandManager::eval(ctx, io.client_handle, continuation) {
            Ok(flow) => {
                io.flow = flow;
                Ok(())
            }
            Err(error) => Err(error.error),
        }
    });
}

