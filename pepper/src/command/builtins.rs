use std::{env, path::Path, process::Stdio};

use crate::{
    buffer::{BufferProperties, BufferReadError, BufferWriteError},
    buffer_position::{BufferPosition, BufferPositionIndex, BufferRange},
    command::{CommandError, CommandIO, CommandManager, CompletionSource},
    config::{ParseConfigError, CONFIG_NAMES},
    cursor::Cursor,
    editor::{EditorContext, EditorFlow},
    editor_utils::{
        parse_path_and_ranges, parse_process_command, validate_process_command, LogKind,
        RegisterKey, REGISTER_READLINE_INPUT,
    },
    events::BufferEditMutGuard,
    help,
    mode::{picker, readline, ModeKind},
    platform::{PlatformRequest, ProcessTag},
    syntax::TokenKind,
    theme::{Color, THEME_COLOR_NAMES},
    word_database::{WordIndicesIter, WordKind},
};

pub fn register_commands(commands: &mut CommandManager) {
    let mut r = |name, completions, command_fn| {
        commands.register_command(None, name, completions, command_fn);
    };

    r("help", &[CompletionSource::HelpPages], |ctx, io| {
        let help_page_name = io.args.try_next().unwrap_or("");
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;

        let mut buffer_path = ctx.editor.string_pool.acquire();
        buffer_path.push_str(help::HELP_PREFIX);
        buffer_path.push_str(help_page_name);

        let buffer_properties = BufferProperties {
            history_enabled: false,
            saving_enabled: false,
            file_backed_enabled: true,
            word_database_enabled: false,
        };

        let result = ctx.editor.buffer_view_handle_from_path(
            client_handle,
            Path::new(&buffer_path),
            buffer_properties,
            true,
        );
        ctx.editor.string_pool.release(buffer_path);
        let handle = result.map_err(CommandError::BufferReadError)?;

        let client = ctx.clients.get_mut(client_handle);
        client.set_buffer_view_handle(Some(handle), &ctx.editor.buffer_views);

        let mut cursors = ctx.editor.buffer_views.get_mut(handle).cursors.mut_guard();
        cursors.clear();

        Ok(())
    });

    static LOG_COMPLETIONS: &[CompletionSource] = &[CompletionSource::Custom(&[
        "status",
        "info",
        "diagnostic",
        "error",
    ])];
    r("log", LOG_COMPLETIONS, |ctx, io| {
        let log_kind = match io.args.next()? {
            "status" => LogKind::Status,
            "info" => LogKind::Info,
            "diagnostic" => LogKind::Diagnostic,
            "error" => LogKind::Error,
            _ => return Err(CommandError::InvalidLogKind),
        };
        let mut write = ctx.editor.logger.write(log_kind);
        if let Some(arg) = io.args.try_next() {
            write.str(arg);
        }
        while let Some(arg) = io.args.try_next() {
            write.str("\n");
            write.str(arg);
        }
        Ok(())
    });

    r("open-log", &[], |ctx, io| {
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;
        let path = ctx
            .editor
            .logger
            .log_file_path()
            .ok_or(CommandError::EditorNotLogging)?;

        let buffer_handle = ctx
            .editor
            .buffers
            .find_with_path(&ctx.editor.current_directory, Path::new(path));
        let buffer_view_handle = match buffer_handle {
            Some(buffer_handle) => {
                let buffer = ctx.editor.buffers.get_mut(buffer_handle);
                buffer
                    .read_from_file(&mut ctx.editor.word_database, ctx.editor.events.writer())
                    .map_err(CommandError::BufferReadError)?;
                ctx.editor
                    .buffer_views
                    .buffer_view_handle_from_buffer_handle(client_handle, buffer_handle)
            }
            None => {
                let path = ctx.editor.string_pool.acquire_with(path);
                let result = ctx.editor.buffer_view_handle_from_path(
                    client_handle,
                    Path::new(&path),
                    BufferProperties::log(),
                    true,
                );
                ctx.editor.string_pool.release(path);
                result.map_err(CommandError::BufferReadError)?
            }
        };

        let client = ctx.clients.get_mut(client_handle);
        client.set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);

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
                "log" => properties = BufferProperties::log(),
                "output" => properties = BufferProperties::output(),
                "history-enabled" => properties.history_enabled = true,
                "history-disabled" => properties.history_enabled = false,
                "saving-enabled" => properties.saving_enabled = true,
                "saving-disabled" => properties.saving_enabled = false,
                "file-backed-enabled" => properties.file_backed_enabled = true,
                "file-backed-disabled" => properties.file_backed_enabled = false,
                "word-database-enabled" => properties.word_database_enabled = true,
                "word-database-disabled" => properties.word_database_enabled = false,
                _ => return Err(CommandError::NoSuchBufferProperty),
            }
            path = arg;
        }

        let client_handle = io.client_handle()?;
        let (path, ranges) = parse_path_and_ranges(path);
        let path = Path::new(path);

        let handle = ctx
            .editor
            .buffer_view_handle_from_path(client_handle, path, properties, true)
            .map_err(CommandError::BufferReadError)?;
        let client = ctx.clients.get_mut(client_handle);
        client.set_buffer_view_handle(Some(handle), &ctx.editor.buffer_views);

        let buffer_view = ctx.editor.buffer_views.get_mut(handle);
        let buffer_content = ctx.editor.buffers.get(buffer_view.buffer_handle).content();

        let mut cursors = buffer_view.cursors.mut_guard();
        let mut cleared_cursors = false;
        for range in ranges {
            if !cleared_cursors {
                cursors.clear();
                cleared_cursors = true;
            }
            cursors.add(Cursor {
                anchor: buffer_content.saturate_position(range.0),
                position: buffer_content.saturate_position(range.1),
            });
        }

        Ok(())
    });

    r("save", &[CompletionSource::Files], |ctx, io| {
        let path = io.args.try_next().map(|p| Path::new(p));
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
        let buffer = ctx.editor.buffers.get_mut(buffer_handle);

        buffer
            .write_to_file(path, ctx.editor.events.writer())
            .map_err(CommandError::BufferWriteError)?;

        ctx.editor
            .logger
            .write(LogKind::Status)
            .fmt(format_args!("buffer saved to {:?}", &buffer.path));
        Ok(())
    });

    r("save-all", &[], |ctx, io| {
        io.args.assert_empty()?;

        let mut count = 0;
        let mut maybe_error = None;
        for buffer in ctx.editor.buffers.iter_mut() {
            match buffer.write_to_file(None, ctx.editor.events.writer()) {
                Ok(()) => count += 1,
                Err(BufferWriteError::SavingDisabled) => (),
                Err(error) => maybe_error = Some(CommandError::BufferWriteError(error)),
            }
        }

        if let Some(error) = maybe_error {
            return Err(error);
        }

        ctx.editor
            .logger
            .write(LogKind::Status)
            .fmt(format_args!("{} buffers saved", count));
        Ok(())
    });

    r("reopen", &[], |ctx, io| {
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
        io.assert_can_discard_buffer(ctx, buffer_handle)?;
        let buffer = ctx.editor.buffers.get_mut(buffer_handle);

        buffer
            .read_from_file(&mut ctx.editor.word_database, ctx.editor.events.writer())
            .map_err(CommandError::BufferReadError)?;

        ctx.editor
            .logger
            .write(LogKind::Status)
            .str("buffer reopened");
        Ok(())
    });

    r("reopen-all", &[], |ctx, io| {
        io.args.assert_empty()?;

        io.assert_can_discard_all_buffers(ctx)?;
        let mut count = 0;
        let mut all_files_found = true;
        let mut maybe_error = None;
        for buffer in ctx.editor.buffers.iter_mut() {
            match buffer.read_from_file(&mut ctx.editor.word_database, ctx.editor.events.writer()) {
                Ok(()) => count += 1,
                Err(BufferReadError::FileNotFound) => all_files_found = true,
                Err(error) => maybe_error = Some(CommandError::BufferReadError(error)),
            }
        }

        if let Some(error) = maybe_error {
            return Err(error);
        }
        if count == 0 && all_files_found {
            return Err(CommandError::BufferReadError(BufferReadError::FileNotFound));
        }

        ctx.editor
            .logger
            .write(LogKind::Status)
            .fmt(format_args!("{} buffers reopened", count));
        Ok(())
    });

    r("close", &[], |ctx, io| {
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
        io.assert_can_discard_buffer(ctx, buffer_handle)?;
        ctx.editor
            .buffers
            .defer_remove(buffer_handle, ctx.editor.events.writer());

        ctx.editor
            .logger
            .write(LogKind::Status)
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
                .defer_remove(buffer.handle(), ctx.editor.events.writer());
            count += 1;
        }

        ctx.editor
            .logger
            .write(LogKind::Status)
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
                        .logger
                        .write(LogKind::Status)
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
                .logger
                .write(LogKind::Status)
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

    r("list-buffer", &[], |ctx, io| {
        io.args.assert_empty()?;
        let client_handle = io.client_handle()?;

        let mut content = ctx.editor.string_pool.acquire();
        for buffer in ctx.editor.buffers.iter() {
            use std::fmt::Write;

            let buffer_path = match buffer.path.to_str() {
                Some(path) => path,
                None => continue,
            };

            content.push_str(buffer_path);
            content.push(':');

            let props = &buffer.properties;
            if !props.history_enabled
                || !props.saving_enabled
                || !props.file_backed_enabled
                || !props.word_database_enabled
            {
                content.push_str(" (");
                if !props.history_enabled {
                    content.push_str("history-disabled, ");
                }
                if !props.saving_enabled {
                    content.push_str("saving-disabled, ");
                }
                if !props.file_backed_enabled {
                    content.push_str("file-backed-disabled, ");
                }
                if !props.word_database_enabled {
                    content.push_str("word-database-disabled, ");
                }
                content.truncate(content.len() - 2);
                content.push(')');
            }
            if buffer.needs_save() {
                content.push_str(" (needs save)");
            }
            if !buffer.lints.all().is_empty() {
                let _ = write!(content, " ({} lints)", buffer.lints.all().len());
            }
            content.push('\n');
        }

        let buffer_view_handle = match ctx.editor.buffer_view_handle_from_path(
            client_handle,
            Path::new("buffers.refs"),
            BufferProperties::scratch(),
            true,
        ) {
            Ok(handle) => handle,
            Err(error) => {
                ctx.editor.string_pool.release(content);
                return Err(CommandError::BufferReadError(error));
            }
        };

        let buffer_handle = ctx
            .editor
            .buffer_views
            .get(buffer_view_handle)
            .buffer_handle;
        let buffer = ctx.editor.buffers.get_mut(buffer_handle);
        let range = BufferRange::between(BufferPosition::zero(), buffer.content().end());
        buffer.delete_range(
            &mut ctx.editor.word_database,
            range,
            &mut ctx
                .editor
                .events
                .writer()
                .buffer_range_deletes_mut_guard(buffer_handle),
        );
        buffer.insert_text(
            &mut ctx.editor.word_database,
            BufferPosition::zero(),
            &content,
            &mut ctx
                .editor
                .events
                .writer()
                .buffer_text_inserts_mut_guard(buffer_handle),
        );

        ctx.editor.string_pool.release(content);

        let client = ctx.clients.get_mut(client_handle);
        client.set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);
        Ok(())
    });

    r("list-lints", &[], |ctx, io| {
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;
        let buffer_view_handle = ctx
            .editor
            .buffer_view_handle_from_path(
                client_handle,
                Path::new("lints.refs"),
                BufferProperties::scratch(),
                true,
            )
            .map_err(CommandError::BufferReadError)?;

        let mut content = ctx.editor.string_pool.acquire();
        for buffer in ctx.editor.buffers.iter() {
            let buffer_path = match buffer.path.to_str() {
                Some(path) => path,
                None => continue,
            };

            for lint in buffer.lints.all() {
                use std::fmt::Write;

                let lint_message = lint.message(&buffer.lints);
                let _ = write!(
                    content,
                    "{}:{}:{}\n",
                    buffer_path, lint.range.from, lint_message
                );
            }
        }
        if content.ends_with('\n') {
            content.pop();
        }

        let buffer_handle = ctx
            .editor
            .buffer_views
            .get(buffer_view_handle)
            .buffer_handle;
        let buffer = ctx.editor.buffers.get_mut(buffer_handle);
        let range = BufferRange::between(BufferPosition::zero(), buffer.content().end());
        buffer.delete_range(
            &mut ctx.editor.word_database,
            range,
            &mut ctx
                .editor
                .events
                .writer()
                .buffer_range_deletes_mut_guard(buffer_handle),
        );
        buffer.insert_text(
            &mut ctx.editor.word_database,
            BufferPosition::zero(),
            &content,
            &mut ctx
                .editor
                .events
                .writer()
                .buffer_text_inserts_mut_guard(buffer_handle),
        );

        ctx.editor.string_pool.release(content);

        let client = ctx.clients.get_mut(client_handle);
        client.set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);
        Ok(())
    });

    r("list-breakpoints", &[], |ctx, io| {
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;
        let buffer_view_handle = ctx
            .editor
            .buffer_view_handle_from_path(
                client_handle,
                Path::new("breakpoints.refs"),
                BufferProperties::scratch(),
                true,
            )
            .map_err(CommandError::BufferReadError)?;

        let mut content = ctx.editor.string_pool.acquire();
        for buffer in ctx.editor.buffers.iter() {
            let buffer_path = match buffer.path.to_str() {
                Some(path) => path,
                None => continue,
            };

            for breakpoint in buffer.breakpoints() {
                use std::fmt::Write;

                let line_content =
                    buffer.content().lines()[breakpoint.line_index as usize].as_str();
                let _ = write!(
                    content,
                    "{}:{}:{}\n",
                    buffer_path,
                    breakpoint.line_index + 1,
                    line_content
                );
            }
        }
        if content.ends_with('\n') {
            content.pop();
        }

        let buffer_handle = ctx
            .editor
            .buffer_views
            .get(buffer_view_handle)
            .buffer_handle;
        let buffer = ctx.editor.buffers.get_mut(buffer_handle);
        let range = BufferRange::between(BufferPosition::zero(), buffer.content().end());
        buffer.delete_range(
            &mut ctx.editor.word_database,
            range,
            &mut ctx
                .editor
                .events
                .writer()
                .buffer_range_deletes_mut_guard(buffer_handle),
        );
        buffer.insert_text(
            &mut ctx.editor.word_database,
            BufferPosition::zero(),
            &content,
            &mut ctx
                .editor
                .events
                .writer()
                .buffer_text_inserts_mut_guard(buffer_handle),
        );

        ctx.editor.string_pool.release(content);

        let client = ctx.clients.get_mut(client_handle);
        client.set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);
        Ok(())
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

    r("insert-text", &[], |ctx, io| {
        let text = io.args.next()?;
        io.args.assert_empty()?;

        let buffer_view_handle = io.current_buffer_view_handle(ctx)?;
        let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
        buffer_view.delete_text_in_cursor_ranges(
            &mut ctx.editor.buffers,
            &mut ctx.editor.word_database,
            ctx.editor.events.writer(),
        );

        ctx.trigger_event_handlers();

        let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
        buffer_view.insert_text_at_cursor_positions(
            &mut ctx.editor.buffers,
            &mut ctx.editor.word_database,
            text,
            ctx.editor.events.writer(),
        );

        ctx.editor
            .buffers
            .get_mut(buffer_view.buffer_handle)
            .commit_edits();
        Ok(())
    });

    fn change_case(
        ctx: &mut EditorContext,
        io: &mut CommandIO,
        to_lower: bool,
    ) -> Result<(), CommandError> {
        io.args.assert_empty()?;

        let buffer_view_handle = io.current_buffer_view_handle(ctx)?;
        let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
        let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle);

        let mut cursor_texts = ctx.editor.string_pool.acquire();

        {
            let mut events = ctx
                .editor
                .events
                .writer()
                .buffer_range_deletes_mut_guard(buffer.handle());
            for cursor in buffer_view.cursors[..].iter().rev() {
                let range = cursor.to_range();
                for text in buffer.content().text_range(range) {
                    cursor_texts.push_str(text);
                }
                cursor_texts.push('\0');
                buffer.delete_range(&mut ctx.editor.word_database, range, &mut events);
            }
        }

        if to_lower {
            cursor_texts.make_ascii_lowercase();
        } else {
            cursor_texts.make_ascii_uppercase();
        }

        {
            let mut cursor_texts_splits = cursor_texts.split_terminator('\0').rev();
            let mut events = ctx
                .editor
                .events
                .writer()
                .buffer_text_inserts_mut_guard(buffer.handle());
            for cursor in buffer_view.cursors[..].iter() {
                let range = cursor.to_range();
                let cursor_text = cursor_texts_splits.next().unwrap();
                buffer.insert_text(
                    &mut ctx.editor.word_database,
                    range.from,
                    cursor_text,
                    &mut events,
                );
            }
        }

        ctx.editor.string_pool.release(cursor_texts);
        buffer.commit_edits();

        {
            let mut events = ctx
                .editor
                .events
                .writer()
                .fix_cursors_mut_guard(buffer_view_handle);
            for &cursor in buffer_view.cursors[..].iter() {
                events.add(cursor);
            }
        }

        Ok(())
    }

    r("to-lowercase", &[], |ctx, io| change_case(ctx, io, true));
    r("to-uppercase", &[], |ctx, io| change_case(ctx, io, false));

    r("toggle-comment", &[], |ctx, io| {
        let comment_prefix = io.args.next()?;
        io.args.assert_empty()?;

        let buffer_view_handle = io.current_buffer_view_handle(ctx)?;
        let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
        let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle);

        let mut events = BufferEditMutGuard::new(ctx.editor.events.writer(), buffer.handle());

        let mut all_lines_commented = true;

        let mut previous_toggle_line_index = BufferPositionIndex::MAX;
        'cursor_loop: for cursor in &buffer_view.cursors[..] {
            let range = cursor.to_range();
            let from_line_index = previous_toggle_line_index
                .wrapping_add(1)
                .max(range.from.line_index);
            let to_line_index = range.to.line_index;
            previous_toggle_line_index = to_line_index;

            for line_index in from_line_index..=to_line_index {
                let line = buffer.content().lines()[line_index as usize]
                    .as_str()
                    .trim_start();
                if !line.is_empty() && !line.starts_with(comment_prefix) {
                    all_lines_commented = false;
                    break 'cursor_loop;
                }
            }
        }

        let mut previous_toggle_line_index = BufferPositionIndex::MAX;
        for cursor in &buffer_view.cursors[..] {
            let range = cursor.to_range();
            let from_line_index = previous_toggle_line_index
                .wrapping_add(1)
                .max(range.from.line_index);
            let to_line_index = range.to.line_index;
            previous_toggle_line_index = to_line_index;

            for line_index in from_line_index..=to_line_index {
                let line = &buffer.content().lines()[line_index as usize];
                let mut position = BufferPosition::line_col(line_index, 0);
                let word = line.word_at(0);
                if word.kind == WordKind::Whitespace {
                    position.column_byte_index += word.text.len() as BufferPositionIndex;
                }

                let line = &line.as_str()[position.column_byte_index as usize..];
                if !line.starts_with(comment_prefix) {
                    if !line.is_empty() {
                        buffer.insert_text(
                            &mut ctx.editor.word_database,
                            position,
                            comment_prefix,
                            events.to_text_inserts(),
                        );
                    }
                } else if all_lines_commented {
                    let to_column_byte_index =
                        position.column_byte_index + comment_prefix.len() as BufferPositionIndex;
                    let range = BufferRange::between(
                        position,
                        BufferPosition::line_col(line_index, to_column_byte_index),
                    );
                    buffer.delete_range(
                        &mut ctx.editor.word_database,
                        range,
                        events.to_range_deletes(),
                    );
                }
            }
        }

        buffer.commit_edits();

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

    r("set-clipboard", &[], |ctx, io| {
        let text = io.args.next()?;
        io.args.assert_empty()?;

        ctx.platform.write_to_clipboard(text);
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
        let continuation = io.args.next()?;
        io.args.assert_empty()?;
        readline::custom::enter_mode(ctx, continuation);
        Ok(())
    });

    r("pick", &[], |ctx, io| {
        let continuation = io.args.next()?;
        io.args.assert_empty()?;
        picker::custom::enter_mode(ctx, continuation);
        Ok(())
    });

    r("picker-entries", &[], |ctx, io| {
        ctx.editor.picker.clear();
        while let Some(arg) = io.args.try_next() {
            ctx.editor.picker.add_custom_entry(arg);
        }
        let readline_input = ctx.editor.registers.get(REGISTER_READLINE_INPUT);
        ctx.editor
            .picker
            .filter(WordIndicesIter::empty(), readline_input);
        Ok(())
    });

    r("picker-entries-from-lines", &[], |ctx, io| {
        let command = io.args.next()?;
        io.args.assert_empty()?;

        ctx.editor.picker.clear();

        let mut command =
            parse_process_command(command).ok_or(CommandError::InvalidProcessCommand)?;

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

        Ok(())
    });

    r("spawn", &[], |ctx, io| {
        let command_text = io.args.next()?;
        io.args.assert_empty()?;

        let mut command =
            parse_process_command(command_text).ok_or(CommandError::InvalidProcessCommand)?;

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

        ctx.editor
            .logger
            .write(LogKind::Diagnostic)
            .fmt(format_args!("spawn '{}'", command_text));

        Ok(())
    });

    r("replace-with-output", &[], |ctx, io| {
        let command_text = io.args.next()?;
        io.args.assert_empty()?;

        if !validate_process_command(command_text) {
            return Err(CommandError::InvalidProcessCommand);
        }

        let buffer_view_handle = io.current_buffer_view_handle(ctx)?;
        let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);

        for cursor in buffer_view.cursors[..].iter().rev() {
            let command = match parse_process_command(command_text) {
                Some(command) => command,
                None => unreachable!(),
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

            let path = &ctx.editor.buffers.get(buffer_view.buffer_handle).path;
            ctx.editor
                .logger
                .write(LogKind::Diagnostic)
                .fmt(format_args!(
                    "replace-with-output '{}' {:?} {}:{}",
                    command_text, &path, cursor.anchor, cursor.position
                ));
        }

        buffer_view.delete_text_in_cursor_ranges(
            &mut ctx.editor.buffers,
            &mut ctx.editor.word_database,
            ctx.editor.events.writer(),
        );

        Ok(())
    });

    r("command", &[], |ctx, io| {
        let name = io.args.next()?;
        let source = io.args.next()?;
        io.args.assert_empty()?;
        ctx.editor.commands.register_macro(name, source)
    });

    r("eval", &[], |ctx, io| {
        let continuation = io.args.next()?;
        io.args.assert_empty()?;
        match CommandManager::eval(ctx, io.client_handle, "eval", continuation) {
            Ok(flow) => {
                io.flow = flow;
                Ok(())
            }
            Err(error) => Err(error),
        }
    });

    static IF_COMPLETIONS: &[CompletionSource] = &[
        CompletionSource::Custom(&[]),
        CompletionSource::Custom(&["==", "!="]),
    ];
    r("if", IF_COMPLETIONS, |ctx, io| {
        let left_expr = io.args.next()?;
        let op = io.args.next()?;
        let right_expr = io.args.next()?;
        let continuation = io.args.next()?;
        io.args.assert_empty()?;

        let should_execute = match op {
            "==" => left_expr == right_expr,
            "!=" => left_expr != right_expr,
            _ => return Err(CommandError::InvalidIfOp),
        };

        if !should_execute {
            return Ok(());
        }

        match CommandManager::eval(ctx, io.client_handle, "if", continuation) {
            Ok(flow) => {
                io.flow = flow;
                Ok(())
            }
            Err(error) => Err(error),
        }
    });
}
