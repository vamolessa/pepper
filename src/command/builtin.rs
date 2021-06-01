use std::{
    any, fmt,
    fs::File,
    io,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    buffer::BufferHandle,
    buffer_position::{BufferPosition, BufferRange},
    client::{Client, ClientManager},
    command::{
        parse_process_command, BuiltinCommand, CommandContext, CommandError, CommandManager,
        CommandOperation, CommandToken, CommandTokenIter, CommandTokenKind, CommandValue,
        CompletionSource, MacroCommand, RequestCommand,
    },
    config::{ParseConfigError, CONFIG_NAMES},
    cursor::{Cursor, CursorCollection},
    editor::{Editor, EditorControlFlow},
    editor_utils::MessageKind,
    help,
    keymap::ParseKeyMapError,
    lsp,
    mode::{picker, read_line, Mode, ModeContext, ModeKind},
    navigation_history::NavigationHistory,
    platform::{Platform, SharedBuf},
    register::RegisterKey,
    syntax::{Syntax, TokenKind},
    theme::{Color, THEME_COLOR_NAMES},
};

fn parse_command_value<T>(value: &CommandValue) -> Result<T, CommandError>
where
    T: 'static + FromStr,
{
    match value.text.parse() {
        Ok(arg) => Ok(arg),
        Err(_) => Err(CommandError::ParseCommandValueError {
            value: value.token,
            type_name: any::type_name::<T>(),
        }),
    }
}

fn parse_register_key(value: &CommandValue) -> Result<RegisterKey, CommandError> {
    match RegisterKey::from_str(value.text) {
        Some(register) => Ok(register),
        None => Err(CommandError::InvalidRegisterKey(value.token)),
    }
}

fn run_commands(
    ctx: &mut CommandContext,
    commands: &str,
) -> Result<Option<CommandOperation>, CommandError> {
    match CommandManager::eval(
        ctx.editor,
        ctx.platform,
        ctx.clients,
        ctx.client_handle,
        commands,
        ctx.source_path,
        ctx.output,
    ) {
        Ok(op) => Ok(op),
        Err((command, error)) => Err(CommandError::EvalCommandError {
            command: command.into(),
            error: Box::new(error),
        }),
    }
}

pub static COMMANDS: &[BuiltinCommand] = &[
    BuiltinCommand {
        name: "help",
        alias: "h",
        hidden: false,
        completions: &[CompletionSource::Commands],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            let keyword = args.try_next()?;
            args.assert_empty()?;

            let (path, position) = match keyword.and_then(|k| help::search(k.text)) {
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
        name: "try",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;

            let try_commands = args.next()?.text;
            let catch_keyword = args.try_next()?;
            let catch_commands = if let Some(catch_keyword) = catch_keyword {
                if catch_keyword.text != "catch" {
                    return Err(CommandError::InvalidToken(catch_keyword.token));
                }
                Some(args.next()?.text)
            } else {
                None
            };

            let try_commands = ctx.editor.string_pool.acquire_with(try_commands);
            let string_pool = &mut ctx.editor.string_pool;
            let catch_commands = catch_commands.map(|c| string_pool.acquire_with(c));

            let op = match run_commands(ctx, &try_commands) {
                Ok(op) => Ok(op),
                Err(_) => match catch_commands {
                    Some(ref commands) => run_commands(ctx, commands),
                    None => Ok(None),
                },
            };

            ctx.editor.string_pool.release(try_commands);
            catch_commands.map(|c| ctx.editor.string_pool.release(c));
            op
        },
    },
    BuiltinCommand {
        name: "macro",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("hidden", None)];
            args.get_flags(&mut flags)?;
            let hidden = flags[0].1.is_some();

            let name = args.next()?;

            let mut params = Vec::new();
            let mut last_arg = args.next()?;
            while let Some(arg) = args.try_next()? {
                params.push(parse_register_key(&last_arg)?);
                last_arg = arg;
            }
            args.assert_empty()?;

            let body = last_arg.text.into();

            if name.text.is_empty() {
                return Err(CommandError::InvalidCommandName(name.token));
            }
            if !name
                .text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                return Err(CommandError::InvalidCommandName(name.token));
            }

            let command = MacroCommand {
                name: name.text.into(),
                hidden,
                params,
                body,
                source_path: ctx.source_path.map(Into::into),
            };
            ctx.editor.commands.register_macro(command);

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "request",
        alias: "",
        hidden: true,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("hidden", None)];
            args.get_flags(&mut flags)?;
            let hidden = flags[0].1.is_some();

            let name = args.next()?;
            args.assert_empty()?;

            if name.text.is_empty() {
                return Err(CommandError::InvalidCommandName(name.token));
            }
            if !name
                .text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                return Err(CommandError::InvalidCommandName(name.token));
            }

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };

            let command = RequestCommand {
                name: name.text.into(),
                hidden,
                client_handle,
            };
            ctx.editor.commands.register_request(command);

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "copy-command",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            let command = args.next()?.text;
            ctx.platform.copy_command.clear();
            ctx.platform.copy_command.push_str(command);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "paste-command",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            let command = args.next()?.text;
            ctx.platform.paste_command.clear();
            ctx.platform.paste_command.push_str(command);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "spawn",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("input", None), ("env", None), ("split-on-byte", None)];
            args.get_flags(&mut flags)?;
            let input = flags[0].1.as_ref().map(|f| f.text);
            let env = flags[1].1.as_ref().map(|f| f.text).unwrap_or("");
            let split_on_byte = match flags[2].1 {
                Some(ref flag) => match flag.text.parse() {
                    Ok(b) => Some(b),
                    Err(_) => return Err(CommandError::InvalidToken(flag.token)),
                },
                None => None,
            };

            let command = args.next()?.text;
            let on_output = args.try_next()?.as_ref().map(|a| a.text);
            args.assert_empty()?;

            let command = parse_process_command(&ctx.editor.registers, command, env)?;
            ctx.editor.commands.spawn_process(
                ctx.platform,
                ctx.client_handle,
                command,
                input,
                on_output,
                split_on_byte,
            );

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "replace-with",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("buffer", None), ("from", None), ("to", None)];
            args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.as_ref().map(parse_command_value).transpose()?;
            let from = flags[1].1.as_ref().map(parse_command_value).transpose()?;
            let to = flags[2].1.as_ref().map(parse_command_value).transpose()?;

            let text = args.next()?.text;
            args.assert_empty()?;

            let buffer_handle = match buffer_handle {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };
            let buffer_end_position = match ctx.editor.buffers.get_mut(buffer_handle) {
                Some(buffer) => buffer.content().end(),
                None => return Err(CommandError::InvalidBufferHandle(buffer_handle)),
            };
            let range = match from {
                Some(from) => match to {
                    Some(to) => Some(BufferRange::between(from, to)),
                    None => Some(BufferRange::between(from, buffer_end_position)),
                },
                None => match to {
                    Some(to) => Some(BufferRange::between(BufferPosition::zero(), to)),
                    None => match ctx
                        .client_handle
                        .and_then(|h| ctx.clients.get(h))
                        .and_then(Client::buffer_view_handle)
                        .and_then(|h| ctx.editor.buffer_views.get(h))
                    {
                        Some(buffer_view) if buffer_view.buffer_handle == buffer_handle => None,
                        _ => Some(BufferRange::between(
                            BufferPosition::zero(),
                            buffer_end_position,
                        )),
                    },
                },
            };

            match range {
                Some(range) => {
                    let buffer = match ctx.editor.buffers.get_mut(buffer_handle) {
                        Some(buffer) => buffer,
                        None => return Err(CommandError::NoBufferOpened),
                    };
                    buffer.delete_range(
                        &mut ctx.editor.word_database,
                        range,
                        &mut ctx.editor.events,
                    );
                    let text = ctx.editor.string_pool.acquire_with(text);
                    buffer.insert_text(
                        &mut ctx.editor.word_database,
                        range.from,
                        &text,
                        &mut ctx.editor.events,
                    );
                    ctx.editor.string_pool.release(text);
                }
                None => {
                    let buffer_view_handle = ctx.current_buffer_view_handle()?;
                    let buffer_view = match ctx.editor.buffer_views.get_mut(buffer_view_handle) {
                        Some(buffer_view) => buffer_view,
                        None => return Err(CommandError::NoBufferOpened),
                    };
                    buffer_view.delete_text_in_cursor_ranges(
                        &mut ctx.editor.buffers,
                        &mut ctx.editor.word_database,
                        &mut ctx.editor.events,
                    );

                    let text = ctx.editor.string_pool.acquire_with(text);
                    ctx.editor.trigger_event_handlers(ctx.platform, ctx.clients);
                    if let Some(buffer_view) = ctx.editor.buffer_views.get_mut(buffer_view_handle) {
                        buffer_view.insert_text_at_cursor_positions(
                            &mut ctx.editor.buffers,
                            &mut ctx.editor.word_database,
                            &text,
                            &mut ctx.editor.events,
                        );
                    }
                    ctx.editor.string_pool.release(text);
                }
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "replace-with-output",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("pipe", None), ("env", None), ("split-on-byte", None)];
            args.get_flags(&mut flags)?;
            let pipe = flags[0].1.is_some();
            let env = flags[1].1.as_ref().map(|f| f.text).unwrap_or("");
            let split_on_byte = match flags[2].1 {
                Some(ref flag) => match flag.text.parse() {
                    Ok(b) => Some(b),
                    Err(_) => return Err(CommandError::InvalidToken(flag.token)),
                },
                None => None,
            };

            let command = args.next()?.text;
            args.assert_empty()?;

            let buffer_view_handle = ctx.current_buffer_view_handle()?;
            let buffer_view = match ctx.editor.buffer_views.get_mut(buffer_view_handle) {
                Some(buffer_view) => buffer_view,
                None => return Err(CommandError::NoBufferOpened),
            };

            const DEFAULT_SHARED_BUF: Option<SharedBuf> = None;
            let mut stdins = [DEFAULT_SHARED_BUF; CursorCollection::capacity()];

            if pipe {
                let mut text = ctx.editor.string_pool.acquire();
                for (i, cursor) in buffer_view.cursors[..].iter().enumerate() {
                    let content = match ctx.editor.buffers.get(buffer_view.buffer_handle) {
                        Some(buffer) => buffer.content(),
                        None => return Err(CommandError::NoBufferOpened),
                    };

                    let range = cursor.to_range();
                    text.clear();
                    content.append_range_text_to_string(range, &mut text);

                    let mut buf = ctx.platform.buf_pool.acquire();
                    let writer = buf.write();
                    writer.extend_from_slice(text.as_bytes());
                    let buf = buf.share();
                    ctx.platform.buf_pool.release(buf.clone());

                    stdins[i] = Some(buf);
                }
                ctx.editor.string_pool.release(text);
            }

            buffer_view.delete_text_in_cursor_ranges(
                &mut ctx.editor.buffers,
                &mut ctx.editor.word_database,
                &mut ctx.editor.events,
            );

            let command = ctx.editor.string_pool.acquire_with(command);
            let env = ctx.editor.string_pool.acquire_with(env);
            ctx.editor.trigger_event_handlers(ctx.platform, ctx.clients);

            if let Some(buffer_view) = ctx.editor.buffer_views.get_mut(buffer_view_handle) {
                for (i, cursor) in buffer_view.cursors[..].iter().enumerate() {
                    let range = cursor.to_range();
                    let command = parse_process_command(&ctx.editor.registers, &command, &env)?;

                    ctx.editor.buffers.spawn_insert_process(
                        ctx.platform,
                        command,
                        buffer_view.buffer_handle,
                        range.from,
                        stdins[i].take(),
                        split_on_byte,
                    );
                }
            }

            ctx.editor.string_pool.release(command);
            ctx.editor.string_pool.release(env);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "execute-keys",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("client", None)];
            args.get_flags(&mut flags)?;
            let client = match flags[0].1 {
                Some(ref flag) => match flag.text.parse() {
                    Ok(handle) => Some(handle),
                    Err(_) => return Err(CommandError::InvalidToken(flag.token)),
                },
                None => ctx.client_handle,
            };

            let keys = args.next()?;
            args.assert_empty()?;

            match client {
                Some(client_handle) => match ctx.editor.buffered_keys.parse(keys.text) {
                    Ok(keys) => {
                        let mut ctx = ModeContext {
                            editor: ctx.editor,
                            platform: ctx.platform,
                            clients: ctx.clients,
                            client_handle,
                        };
                        let mode = ctx.editor.mode.kind();
                        Mode::change_to(&mut ctx, ModeKind::default());
                        let op = match ctx.editor.execute_keys(
                            ctx.platform,
                            ctx.clients,
                            client_handle,
                            keys,
                        ) {
                            EditorControlFlow::Continue => Ok(None),
                            EditorControlFlow::Suspend => Ok(Some(CommandOperation::Suspend)),
                            EditorControlFlow::Quit => Ok(Some(CommandOperation::Quit)),
                            EditorControlFlow::QuitAll => Ok(Some(CommandOperation::QuitAll)),
                        };
                        Mode::change_to(&mut ctx, mode);
                        op
                    }
                    Err(_) => Err(CommandError::BufferedKeysParseError(keys.token)),
                },
                None => Ok(None),
            }
        },
    },
    BuiltinCommand {
        name: "read-line",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("prompt", None)];
            args.get_flags(&mut flags)?;
            let prompt = flags[0].1.as_ref().map(|f| f.text).unwrap_or("read-line:");

            let commands = args.next()?.text;
            args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };

            ctx.editor.read_line.set_prompt(prompt);

            let commands = ctx.editor.string_pool.acquire_with(commands);
            let mut mode_ctx = ModeContext {
                editor: ctx.editor,
                platform: ctx.platform,
                clients: ctx.clients,
                client_handle,
            };
            read_line::custom::enter_mode(&mut mode_ctx, commands);

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "pick",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("prompt", None)];
            args.get_flags(&mut flags)?;
            let prompt = flags[0].1.as_ref().map(|f| f.text).unwrap_or("pick:");

            let commands = args.next()?.text;
            args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };

            ctx.editor.read_line.set_prompt(prompt);

            let commands = ctx.editor.string_pool.acquire_with(commands);
            let mut mode_ctx = ModeContext {
                editor: ctx.editor,
                platform: ctx.platform,
                clients: ctx.clients,
                client_handle,
            };
            picker::custom::enter_mode(&mut mode_ctx, commands);

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "add-picker-option",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            let name = args.next()?.text;
            args.assert_empty()?;

            if ModeKind::Picker == ctx.editor.mode.kind() {
                ctx.editor
                    .picker
                    .add_custom_entry_filtered(name, ctx.editor.read_line.input());
                ctx.editor.picker.move_cursor(0);
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "quit",
        alias: "q",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.get_flags(&mut [])?;
            args.assert_empty()?;

            if ctx.clients.iter().count() == 1 {
                ctx.assert_can_discard_all_buffers()?;
            }
            Ok(Some(CommandOperation::Quit))
        },
    },
    BuiltinCommand {
        name: "quit-all",
        alias: "qa",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.get_flags(&mut [])?;
            args.assert_empty()?;

            ctx.assert_can_discard_all_buffers()?;
            Ok(Some(CommandOperation::QuitAll))
        },
    },
    BuiltinCommand {
        name: "print",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("error", None), ("dbg", None)];
            args.get_flags(&mut flags)?;
            let error = flags[0].1.is_some();
            let dbg = flags[1].1.is_some();

            let message_kind = if error {
                MessageKind::Error
            } else {
                MessageKind::Info
            };

            let mut write = ctx.editor.status_bar.write(message_kind);
            while let Some(arg) = args.try_next()? {
                write.str(arg.text);
                if dbg {
                    eprint!("{}", arg.text);
                }
            }
            if dbg {
                eprintln!();
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "source",
        alias: "",
        hidden: false,
        completions: &[CompletionSource::Files],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;

            let path = args.next()?;
            let path_token = path.token;
            let path = Path::new(path.text);
            args.assert_empty()?;

            let mut path_buf = PathBuf::new();
            if path.is_relative() {
                if let Some(parent) = ctx.source_path.and_then(Path::parent) {
                    path_buf.push(parent);
                }
            }
            path_buf.push(path);
            let path = path_buf.as_path();

            use io::Read;
            let mut file = File::open(path).map_err(|e| CommandError::OpenFileError {
                path: path_token,
                error: e,
            })?;
            let mut source = ctx.editor.string_pool.acquire();
            file.read_to_string(&mut source)
                .map_err(|e| CommandError::OpenFileError {
                    path: path_token,
                    error: e,
                })?;

            let op = CommandManager::eval_and_then_output(
                ctx.editor,
                ctx.platform,
                ctx.clients,
                None,
                &source,
                Some(path),
            );
            ctx.editor.string_pool.release(source);

            Ok(op)
        },
    },
    BuiltinCommand {
        name: "open",
        alias: "o",
        hidden: false,
        completions: &[CompletionSource::Files],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [
                ("line", None),
                ("column", None),
                ("no-history", None),
                ("no-save", None),
                ("no-word-database", None),
                ("auto-close", None),
            ];
            args.get_flags(&mut flags)?;
            let line = flags[0]
                .1
                .as_ref()
                .map(parse_command_value::<u32>)
                .transpose()?;
            let column = flags[1]
                .1
                .as_ref()
                .map(parse_command_value::<u32>)
                .transpose()?;
            let no_history = flags[2].1.is_some();
            let no_save = flags[3].1.is_some();
            let no_word_database = flags[4].1.is_some();
            let auto_close = flags[5].1.is_some();

            let path = args.next()?.text;
            args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };

            let mut has_position = false;
            let mut position = BufferPosition::zero();
            if let Some(line) = line {
                has_position = true;
                position.line_index = line.saturating_sub(1);
            }
            if let Some(column) = column {
                has_position = true;
                position.column_byte_index = column.saturating_sub(1);
            }

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
                if has_position {
                    let mut cursors = buffer_view.cursors.mut_guard();
                    cursors.clear();
                    cursors.add(Cursor {
                        anchor: position,
                        position,
                    });
                }

                if let Some(buffer) = ctx.editor.buffers.get_mut(buffer_view.buffer_handle) {
                    buffer.capabilities.has_history = !no_history;
                    buffer.capabilities.can_save = !no_save;
                    buffer.capabilities.uses_word_database = !no_word_database;
                    buffer.capabilities.auto_close = auto_close;
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
        alias: "s",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("buffer", None)];
            args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.as_ref().map(parse_command_value).transpose()?;

            let path = args.try_next()?.map(|a| Path::new(a.text));
            args.assert_empty()?;

            let buffer_handle = match buffer_handle {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };

            let buffer = ctx
                .editor
                .buffers
                .get_mut(buffer_handle)
                .ok_or(CommandError::InvalidBufferHandle(buffer_handle))?;

            buffer
                .save_to_file(path, &mut ctx.editor.events)
                .map_err(|e| CommandError::BufferError(buffer_handle, e))?;

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("buffer saved to {:?}", &buffer.path));
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "save-all",
        alias: "sa",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

            let mut count = 0;
            for buffer in ctx.editor.buffers.iter_mut() {
                if buffer.capabilities.can_save {
                    buffer
                        .save_to_file(None, &mut ctx.editor.events)
                        .map_err(|e| CommandError::BufferError(buffer.handle(), e))?;
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
        name: "reload",
        alias: "r",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            let mut flags = [("buffer", None)];
            args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.as_ref().map(parse_command_value).transpose()?;

            args.assert_empty()?;

            let buffer_handle = match buffer_handle {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };

            ctx.assert_can_discard_buffer(buffer_handle)?;
            let buffer = ctx
                .editor
                .buffers
                .get_mut(buffer_handle)
                .ok_or(CommandError::InvalidBufferHandle(buffer_handle))?;

            buffer
                .discard_and_reload_from_file(&mut ctx.editor.word_database, &mut ctx.editor.events)
                .map_err(|e| CommandError::BufferError(buffer_handle, e))?;

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .str("buffer reloaded");
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "reload-all",
        alias: "ra",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.get_flags(&mut [])?;
            args.assert_empty()?;

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
        name: "close",
        alias: "c",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            let mut flags = [("buffer", None), ("no-previous-buffer", None)];
            args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.as_ref().map(parse_command_value).transpose()?;
            let no_previous_buffer = flags[1].1.is_some();

            args.assert_empty()?;

            let buffer_handle = match buffer_handle {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };

            ctx.assert_can_discard_buffer(buffer_handle)?;
            ctx.editor
                .buffers
                .defer_remove(buffer_handle, &mut ctx.editor.events);

            if !no_previous_buffer {
                let clients = &mut *ctx.clients;
                if let Some(client) = ctx.client_handle.and_then(|h| clients.get_mut(h)) {
                    client.set_buffer_view_handle(
                        client.previous_buffer_view_handle(),
                        &mut ctx.editor.events,
                    );
                }
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
        alias: "ca",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.get_flags(&mut [])?;
            args.assert_empty()?;

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
        alias: "",
        hidden: false,
        completions: &[(CompletionSource::Custom(CONFIG_NAMES))],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;

            let key = args.next()?;
            let value = args.try_next()?;
            args.assert_empty()?;

            match value {
                Some(value) => match ctx.editor.config.parse_config(key.text, value.text) {
                    Ok(()) => Ok(None),
                    Err(ParseConfigError::NotFound) => Err(CommandError::ConfigNotFound(key.token)),
                    Err(ParseConfigError::InvalidValue) => Err(CommandError::InvalidConfigValue {
                        key: key.token,
                        value: value.token,
                    }),
                },
                None => match ctx.editor.config.display_config(key.text) {
                    Some(display) => {
                        use fmt::Write;
                        let _ = write!(ctx.output, "{}", display);
                        Ok(None)
                    }
                    None => Err(CommandError::ConfigNotFound(key.token)),
                },
            }
        },
    },
    BuiltinCommand {
        name: "color",
        alias: "",
        hidden: false,
        completions: &[CompletionSource::Custom(THEME_COLOR_NAMES)],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;

            let key = args.next()?;
            let value = args.try_next()?;
            args.assert_empty()?;

            let color = ctx
                .editor
                .theme
                .color_from_name(key.text)
                .ok_or(CommandError::ColorNotFound(key.token))?;

            match value {
                Some(value) => {
                    let encoded = u32::from_str_radix(value.text, 16).map_err(|_| {
                        CommandError::InvalidColorValue {
                            key: key.token,
                            value: value.token,
                        }
                    })?;
                    *color = Color::from_u32(encoded);
                }
                None => {
                    use fmt::Write;
                    let _ = write!(ctx.output, "0x{:0<6x}", color.into_u32());
                }
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "syntax",
        alias: "",
        hidden: true,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;

            let glob = args.next()?;
            let definition = args.next()?.text;
            args.assert_empty()?;

            let mut syntax = Syntax::new();
            syntax
                .set_glob(glob.text)
                .map_err(|_| CommandError::InvalidGlob(glob.token))?;

            let mut definition_tokens = CommandTokenIter::new(definition);
            loop {
                let token_kind = match definition_tokens.next() {
                    Some((CommandTokenKind::Identifier, token)) => token,
                    Some((CommandTokenKind::Unterminated, token)) => {
                        return Err(CommandError::UnterminatedToken(token))
                    }
                    Some((_, token)) => return Err(CommandError::InvalidToken(token)),
                    None => break,
                };
                let token_kind = match token_kind.as_str(definition) {
                    "keywords" => TokenKind::Keyword,
                    "types" => TokenKind::Type,
                    "symbols" => TokenKind::Symbol,
                    "literals" => TokenKind::Literal,
                    "strings" => TokenKind::String,
                    "comments" => TokenKind::Comment,
                    "texts" => TokenKind::Text,
                    _ => return Err(CommandError::InvalidToken(token_kind)),
                };
                match definition_tokens.next() {
                    Some((CommandTokenKind::Equals, _)) => (),
                    Some((CommandTokenKind::Unterminated, token)) => {
                        return Err(CommandError::UnterminatedToken(token));
                    }
                    Some((_, token)) => {
                        return Err(CommandError::InvalidToken(token));
                    }
                    None => {
                        let end = definition_tokens.end_token();
                        return Err(CommandError::SyntaxExpectedEquals(end));
                    }
                }
                let pattern = match definition_tokens.next() {
                    Some((CommandTokenKind::String, token)) => token,
                    Some((CommandTokenKind::Unterminated, token)) => {
                        return Err(CommandError::UnterminatedToken(token));
                    }
                    Some((_, token)) => return Err(CommandError::InvalidToken(token)),
                    None => {
                        let end = definition_tokens.end_token();
                        return Err(CommandError::SyntaxExpectedPattern(end));
                    }
                };

                if let Err(error) = syntax.set_rule(token_kind, pattern.as_str(definition)) {
                    return Err(CommandError::PatternError(pattern, error.kind));
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
        name: "map",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [
                ("normal", None),
                ("insert", None),
                ("read-line", None),
                ("picker", None),
                ("command", None),
            ];
            args.get_flags(&mut flags)?;

            let from = args.next()?;
            let to = args.next()?;
            args.assert_empty()?;

            let modes = [
                ModeKind::Normal,
                ModeKind::Insert,
                ModeKind::ReadLine,
                ModeKind::Picker,
                ModeKind::Command,
            ];
            for ((_, flag), &mode) in flags.iter().zip(modes.iter()) {
                if !flag.is_some() {
                    continue;
                }

                match ctx.editor.keymaps.parse_and_map(mode, from.text, to.text) {
                    Ok(()) => (),
                    Err(ParseKeyMapError::From(e)) => {
                        let token = &from.text[e.index..];
                        let end = token.chars().next().map(char::len_utf8).unwrap_or(0);
                        let from = to.token.from + e.index;
                        let token = CommandToken {
                            from,
                            to: from + end,
                        };
                        return Err(CommandError::KeyParseError(token, e.error));
                    }
                    Err(ParseKeyMapError::To(e)) => {
                        let token = &to.text[e.index..];
                        let end = token.chars().next().map(char::len_utf8).unwrap_or(0);
                        let from = to.token.from + e.index;
                        let token = CommandToken {
                            from,
                            to: from + end,
                        };
                        return Err(CommandError::KeyParseError(token, e.error));
                    }
                }
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "text-len",
        alias: "",
        hidden: true,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            let text = args.next()?.text;
            args.assert_empty()?;

            use fmt::Write;
            let _ = write!(ctx.output, "{}", text.len());
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "text-join",
        alias: "",
        hidden: true,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;

            while let Some(arg) = args.try_next()? {
                ctx.output.push_str(arg.text);
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "new-line",
        alias: "",
        hidden: true,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

            ctx.output.push('\n');
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "client-id",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;
            if let Some(handle) = ctx.client_handle {
                use fmt::Write;
                let _ = write!(ctx.output, "{}", handle.into_index());
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "buffer-id",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;
            let buffer_handle = ctx.current_buffer_handle()?;
            use fmt::Write;
            let _ = write!(ctx.output, "{}", buffer_handle);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "buffer-path",
        alias: "",
        hidden: true,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("buffer", None)];
            args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.as_ref().map(parse_command_value).transpose()?;

            args.assert_empty()?;

            let buffer_handle = match buffer_handle {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };

            if let Some(path) = ctx
                .editor
                .buffers
                .get(buffer_handle)
                .and_then(|b| b.path.to_str())
            {
                use fmt::Write;
                let _ = write!(ctx.output, "{}", path);
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "buffer-line-count",
        alias: "",
        hidden: true,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("buffer", None)];
            args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.as_ref().map(parse_command_value).transpose()?;

            args.assert_empty()?;

            let buffer_handle = match buffer_handle {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };
            let buffer = match ctx.editor.buffers.get(buffer_handle) {
                Some(buffer) => buffer.content(),
                None => return Err(CommandError::InvalidBufferHandle(buffer_handle)),
            };

            use fmt::Write;
            let _ = write!(ctx.output, "{}", buffer.line_count());
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "buffer-text",
        alias: "",
        hidden: true,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("buffer", None), ("from", None), ("to", None)];
            args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.as_ref().map(parse_command_value).transpose()?;
            let from = flags[1].1.as_ref().map(parse_command_value).transpose()?;
            let to = flags[1].1.as_ref().map(parse_command_value).transpose()?;

            args.assert_empty()?;

            let buffer_handle = match buffer_handle {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };
            let buffer = match ctx.editor.buffers.get(buffer_handle) {
                Some(buffer) => buffer.content(),
                None => return Err(CommandError::InvalidBufferHandle(buffer_handle)),
            };
            let from = match from {
                Some(from) => from,
                None => BufferPosition::zero(),
            };
            let to = match to {
                Some(to) => to,
                None => buffer.end(),
            };
            let range = BufferRange::between(from, to);

            buffer.append_range_text_to_string(range, ctx.output);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp",
        alias: "",
        hidden: true,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("root", None), ("log", None), ("env", None)];
            args.get_flags(&mut flags)?;
            let root = flags[0].1.as_ref().map(|f| Path::new(f.text));
            let log_buffer = flags[1].1.as_ref().map(|f| f.text);
            let env = flags[2].1.as_ref().map(|f| f.text).unwrap_or("");

            let glob = args.next()?;
            let command = args.next()?.text;

            ctx.editor
                .lsp
                .add_recipe(glob.text, command, env, root, log_buffer)
                .map_err(|_| CommandError::InvalidGlob(glob.token))?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-start",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("root", None), ("log", None), ("env", None)];
            args.get_flags(&mut flags)?;
            let root = flags[0].1.as_ref();
            let log_buffer = flags[1].1.as_ref().map(|f| f.text);
            let env = flags[2].1.as_ref().map(|f| f.text).unwrap_or("");

            let command = args.next()?.text;
            let command = parse_process_command(&ctx.editor.registers, command, env)?;

            let root = match root {
                Some(root) => PathBuf::from(root.text),
                None => ctx.editor.current_directory.clone(),
            };

            ctx.editor.lsp.start(
                ctx.platform,
                &mut ctx.editor.buffers,
                command,
                root,
                log_buffer,
            );
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-stop",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

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
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

            ctx.editor.lsp.stop_all(ctx.platform);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-hover",
        alias: "",
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.hover(editor, platform, buffer_handle, cursor.position)
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-definition",
        alias: "",
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

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
        alias: "",
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;

            let mut flags = [("context", None), ("auto-close", None)];
            args.get_flags(&mut flags)?;
            let context_len = flags[0]
                .1
                .as_ref()
                .map(parse_command_value)
                .transpose()?
                .unwrap_or(0);
            let auto_close_buffer = flags[1].1.is_some();

            args.assert_empty()?;

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
                    auto_close_buffer,
                    client_handle,
                )
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-rename",
        alias: "",
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

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
        alias: "",
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

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
        alias: "",
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

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
        alias: "",
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            let query = args.try_next()?.map(|a| a.text).unwrap_or("");
            args.assert_empty()?;

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
        alias: "",
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

            let buffer_handle = ctx.current_buffer_handle()?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.formatting(editor, platform, buffer_handle)
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-debug",
        alias: "",
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut args = ctx.args.with(&ctx.editor.registers);
            args.assert_no_bang()?;
            args.get_flags(&mut [])?;
            args.assert_empty()?;

            use fmt::Write;
            let mut message = ctx.editor.string_pool.acquire();
            for client in ctx.editor.lsp.clients() {
                let _ = writeln!(
                    message,
                    "handle [{}] log buffer handle: {:?}",
                    client.handle(),
                    client.log_buffer_handle,
                );
            }
            let _ = writeln!(
                message,
                "\nbuffer count: {}",
                ctx.editor.buffers.iter().count()
            );
            ctx.editor.status_bar.write(MessageKind::Info).str(&message);
            ctx.editor.string_pool.release(message);
            Ok(None)
        },
    },
];

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
