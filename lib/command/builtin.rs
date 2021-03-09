use std::{
    any, fmt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
};

use crate::{
    application::ProcessTag,
    buffer::{BufferCapabilities, BufferHandle},
    buffer_position::BufferPosition,
    buffer_view::BufferViewError,
    command::{
        BuiltinCommand, CommandContext, CommandError, CommandIter, CommandManager,
        CommandOperation, CommandSource, CompletionSource, CustomCommand,
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

pub fn parse_arg<T>(arg: &str) -> Result<T, CommandError>
where
    T: 'static + FromStr,
{
    match arg.parse() {
        Ok(arg) => Ok(arg),
        Err(_) => Err(CommandError::ParseArgError {
            arg: arg.into(),
            type_name: any::type_name::<T>(),
        }),
    }
}

pub const COMMANDS: &[BuiltinCommand] = &[
    BuiltinCommand {
        name: "help",
        alias: "h",
        help: "prints help about command\nhelp [<command-name>]",
        completions: &[CompletionSource::Commands],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            let command_name = ctx.args.try_next()?;
            ctx.args.assert_empty()?;

            let commands = &ctx.editor.commands;
            match command_name {
                Some(command_name) => {
                    let source = match commands.find_command(command_name) {
                        Some(source) => source,
                        None => return Err(CommandError::CommandNotFound(command_name.into())),
                    };

                    let (alias, help) = match source {
                        CommandSource::Builtin(i) => {
                            let command = &commands.builtin_commands()[i];
                            (command.alias, command.help)
                        },
                        CommandSource::Custom(i) => {
                            let command = &commands.custom_commands()[i];
                            (&command.alias[..], &command.help[..])
                        }
                    };

                    let mut write = ctx.editor.status_bar.write(MessageKind::Info);
                    write.str(help);
                    if !alias.is_empty() {
                        write.str("\nalias: ");
                        write.str(alias);
                    }
                }
                None => {
                    if let Some(client) = ctx.client_handle.and_then(|h| ctx.clients.get(h)) {
                        let width = client.viewport_size.0 as usize;

                        let mut write = ctx.editor.status_bar.write(MessageKind::Info);
                        write.str("all commands:\n");

                        let mut x = 0;
                        for command in commands.builtin_commands() {
                            if x + command.name.len() + 1 > width {
                                x = 0;
                                write.str("\n");
                            } else if x > 0 {
                                x += 1;
                                write.str(" ");
                            }
                            write.str(command.name);
                            x += command.name.len();
                        }
                    }
                }
            }
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "try",
        alias: "",
        help: concat!(
            "try executing commands without propagating errors\n",
            "and optionally execute commands if there was an error\n",
            "try { <commands>... } [catch { <commands>... }]",
        ),
        completions: &[],
        func: |ctx| {
            fn run_body(
                ctx: &mut CommandContext,
                body: &str
            ) -> Result<Option<CommandOperation>, CommandError> {
                for command in CommandIter(body) {
                    match CommandManager::eval(
                        ctx.editor,
                        ctx.platform,
                        ctx.clients,
                        ctx.client_handle,
                        command,
                        ctx.output,
                    ) {
                        Ok(None) => (),
                        Ok(Some(op)) => return Ok(Some(op)),
                        Err(error) => return Err(error),
                    }
                }
                Ok(None)
            }

            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;

            let try_body = ctx.args.next()?;
            let catch_keyword = ctx.args.try_next()?;
            let catch_body = if let Some(catch_keyword) = catch_keyword {
                if catch_keyword != "catch" {
                    return Err(CommandError::InvalidToken(catch_keyword.into()));
                }

                Some(ctx.args.next()?)
            } else {
                None
            };

            match run_body(ctx, try_body) {
                Ok(op) => Ok(op),
                Err(_) => match catch_body {
                    Some(body) => run_body(ctx, body),
                    None => Ok(None),
                }
            }
        },
    },
    BuiltinCommand {
        name: "command",
        alias: "",
        help: concat!(
            "define a new custom command\n",
            "command [<flags>] <name> <body>\n",
            " -help=<help-text> : the help text that shows when using `help` with this command\n",
            " -alias=<name> : also add an alias to the command\n",
            " -param-count=<number> : if defined, the number of parameters this command expects, 0 otherwise",
        ),
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("help", None), ("alias", None), ("param-count", None)];
            ctx.args.get_flags(&mut flags)?;
            let help = flags[0].1.unwrap_or("");
            let alias = flags[1].1.unwrap_or("");
            let param_count = flags[2].1.map(parse_arg).transpose()?.unwrap_or(0);

            let name = ctx.args.next()?;
            let body = ctx.args.next()?;
            ctx.args.assert_empty()?;

            if name.is_empty() {
                return Err(CommandError::InvalidCommandName(name.into()));
            }
            if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
                return Err(CommandError::InvalidCommandName(name.into()));
            }

            let command = CustomCommand {
                name: name.into(),
                alias: alias.into(),
                help: help.into(),
                param_count,
                body: body.into(),
            };
            ctx.editor.commands.register_custom_command(command);

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "quit",
        alias: "q",
        help: "quits this client\nquit[!]\nwith '!' will discard any unsaved changes",
        completions: &[],
        func: |ctx| {
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            if ctx.clients.iter_mut().count() == 1 {
                ctx.assert_can_discard_all_buffers()?;
            }
            Ok(Some(CommandOperation::Quit))
        },
    },
    BuiltinCommand {
        name: "quit-all",
        alias: "qa",
        help: "quits all clients\nquit-all[!]\nwith '!' will discard any unsaved changes",
        completions: &[],
        func: |ctx| {
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            ctx.assert_can_discard_all_buffers()?;
            Ok(Some(CommandOperation::QuitAll))
        },
    },
    BuiltinCommand {
        name: "print",
        alias: "",
        help: concat!(
            "prints arguments to the status bar\nprint <value>...\n",
            " -error : will print the message as an error",
        ),
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("error", None)];
            ctx.args.get_flags(&mut flags)?;
            let error = flags[0].1.is_some();

            let message_kind = if error {
                MessageKind::Error
            } else {
                MessageKind::Info
            };

            let mut write = ctx.editor.status_bar.write(message_kind);
            while let Some(arg) = ctx.args.try_next()? {
                write.str(arg);
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "source",
        alias: "",
        help: "loads a source file and execute its commands\nsource <path>",
        completions: &[CompletionSource::Files],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            let path = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let op = ctx.editor.load_config(ctx.platform, ctx.clients, path);
            Ok(op)
        },
    },
    BuiltinCommand {
        name: "open",
        alias: "o",
        help: concat!(
            "opens a buffer for editting\nopen [<flags>] <path>\n",
            " -line=<number> : set cursor at line",
        ),
        completions: &[CompletionSource::Files],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("line", None)];
            ctx.args.get_flags(&mut flags)?;
            let line = flags[0]
                .1
                .map(parse_arg::<usize>)
                .transpose()?
                .map(|l| l.saturating_sub(1));

            let path = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };

            NavigationHistory::save_client_snapshot(
                ctx.clients,
                client_handle,
                &ctx.editor.buffer_views,
            );

            match ctx.editor.buffer_views.buffer_view_handle_from_path(
                client_handle,
                &mut ctx.editor.buffers,
                &mut ctx.editor.word_database,
                &ctx.editor.current_directory,
                Path::new(path),
                line,
                &mut ctx.editor.events,
            ) {
                Ok(handle) => {
                    if let Some(client) = ctx.clients.get_mut(client_handle) {
                        client.set_buffer_view_handle(Some(handle));
                    }

                    use fmt::Write;
                    ctx.output.clear();
                    let _ = write!(ctx.output, "{}", handle);
                    Ok(None)
                }
                Err(BufferViewError::InvalidPath) => Err(CommandError::InvalidPath(path.into())),
            }
        },
    },
    BuiltinCommand {
        name: "save",
        alias: "s",
        help: concat!(
            "save buffer\nsave [<flags>] [<path>]\n",
            " -buffer=<buffer-id> : if not specified, the current buffer is used",
        ),
        completions: &[CompletionSource::Files],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("buffer", None)];
            ctx.args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.map(parse_arg).transpose()?;

            let path = ctx.args.try_next()?.map(Path::new);
            ctx.args.assert_empty()?;

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

            let path = buffer.path().unwrap_or(Path::new(""));
            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("buffer saved to '{:?}'", path));
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "save-all",
        alias: "sa",
        help: "save all buffers\nsave-all",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

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
        name: "reload",
        alias: "r",
        help: concat!(
            "reload buffer from file\n",
            "reload[!] [<flags>]\n",
            "with '!' will discard any unsaved changes",
            " -buffer=<buffer-id> : if not specified, the current buffer is used",
        ),
        completions: &[],
        func: |ctx| {
            let mut flags = [("buffer", None)];
            ctx.args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.map(parse_arg).transpose()?;

            ctx.args.assert_empty()?;

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
        help:
            "reload all buffers from file\nreload-all[!]\nwith '!' will discard any unsaved changes",
        completions: &[],
        func: |ctx| {
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

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
        help: concat!(
            "close buffer\n",
            "close[!] [<flags>]\n",
            "with '!' will discard any unsaved changes",
            " -buffer=<buffer-id> : if not specified, the current buffer is used"
        ),
        completions: &[],
        func: |ctx| {
            let mut flags = [("buffer", None)];
            ctx.args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.map(parse_arg).transpose()?;

            ctx.args.assert_empty()?;

            let buffer_handle = match buffer_handle {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };

            ctx.assert_can_discard_buffer(buffer_handle)?;
            ctx.editor.buffer_views.defer_remove_buffer_where(
                &mut ctx.editor.buffers,
                &mut ctx.editor.events,
                |view| view.buffer_handle == buffer_handle,
            );

            let clients = &mut *ctx.clients;
            let editor = &mut *ctx.editor;
            for client in clients.iter_mut() {
                let maybe_buffer_handle = client
                    .buffer_view_handle()
                    .and_then(|h| editor.buffer_views.get(h))
                    .map(|v| v.buffer_handle);
                if maybe_buffer_handle == Some(buffer_handle) {
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
        name: "close-all",
        alias: "ca",
        help: "close all buffers\nclose-all[!]\nwith '!' will discard any unsaved changes",
        completions: &[],
        func: |ctx| {
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

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
        name: "config",
        alias: "",
        help: "accesses an editor config\nconfig <key> [<value>]",
        completions: &[(CompletionSource::Custom(CONFIG_NAMES))],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;

            let key = ctx.args.next()?;
            let value = ctx.args.try_next()?;
            ctx.args.assert_empty()?;

            match value {
                Some(value) => match ctx.editor.config.parse_config(key, value) {
                    Ok(()) => Ok(None),
                    Err(ParseConfigError::NotFound) => Err(CommandError::ConfigNotFound(key.into())),
                    Err(ParseConfigError::InvalidValue) => {
                        Err(CommandError::InvalidConfigValue { key: key.into(), value: value.into() })
                    }
                },
                None => match ctx.editor.config.display_config(key) {
                    Some(display) => {
                        use fmt::Write;
                        ctx.output.clear();
                        let _ = write!(ctx.output, "{}", display);
                        Ok(None)
                    }
                    None => Err(CommandError::ConfigNotFound(key.into())),
                },
            }
        },
    },
    BuiltinCommand {
        name: "color",
        alias: "",
        help: "accesses an editor theme color\ncolor <key> [<value>]",
        completions: &[CompletionSource::Custom(THEME_COLOR_NAMES)],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;

            let key = ctx.args.next()?;
            let value = ctx.args.try_next()?;
            ctx.args.assert_empty()?;

            let color = ctx
                .editor
                .theme
                .color_from_name(key)
                .ok_or(CommandError::ColorNotFound(key.into()))?;

            match value {
                Some(value) => {
                    let encoded = u32::from_str_radix(value, 16)
                        .map_err(|_| CommandError::InvalidColorValue { key: key.into(), value: value.into() })?;
                    *color = Color::from_u32(encoded);
                }
                None => {
                    use fmt::Write;
                    ctx.output.clear();
                    let _ = write!(ctx.output, "0x{:0<6x}", color.into_u32());
                }
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "syntax",
        alias: "",
        help: concat!(
            "creates a syntax definition with patterns for files that match a glob\n",
            "syntax [<flags>] <glob>\n",
            " -keywords=<pattern> : pattern for keyword highlight\n",
            " -types=<pattern> : pattern for type highlight\n",
            " -symbols=<pattern> : pattern for symbol highlight\n",
            " -literals=<pattern> : pattern for literal highlight\n",
            " -strings=<pattern> : pattern for string highlight\n",
            " -comments=<pattern> : pattern for comment highlight\n",
            " -texts=<pattern> : pattern for normal text highlight",
        ),
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [
                ("keywords", None),
                ("types", None),
                ("symbols", None),
                ("literals", None),
                ("strings", None),
                ("comments", None),
                ("texts", None),
            ];
            ctx.args.get_flags(&mut flags)?;

            let glob = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let mut syntax = Syntax::new();
            syntax
                .set_glob(glob.as_bytes())
                .map_err(|_| CommandError::InvalidGlob(glob.into()))?;

            let kinds = [
                TokenKind::Keyword,
                TokenKind::Type,
                TokenKind::Symbol,
                TokenKind::Literal,
                TokenKind::String,
                TokenKind::Comment,
                TokenKind::Text,
            ];
            for ((_, flag), &kind) in flags.iter().zip(kinds.iter()) {
                if let Some(flag) = *flag {
                    syntax
                        .set_rule(kind, flag)
                        .map_err(|e| CommandError::PatternError(flag.into(), e))?;
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
        help: concat!(
            "creates a keyboard mapping for an editor mode\n",
            "map [<flags>] <from> <to>\n",
            " -normal : set mapping for normal mode\n",
            " -insert : set mapping for insert mode\n",
            " -read-line : set mapping for read-line mode\n",
            " -picker : set mapping for picker mode\n",
            " -command : set mapping for command mode",
        ),
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [
                ("normal", None),
                ("insert", None),
                ("read-line", None),
                ("picker", None),
                ("command", None),
            ];
            ctx.args.get_flags(&mut flags)?;

            let from = ctx.args.next()?;
            let to = ctx.args.next()?;
            ctx.args.assert_empty()?;

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

                match ctx.editor
                    .keymaps
                    .parse_and_map(mode, from, to)
                {
                    Ok(()) => (),
                    Err(ParseKeyMapError::From(e)) => {
                        let token = &from[e.index..];
                        let end = token.chars().next().map(char::len_utf8).unwrap_or(0);
                        let token = token[..end].into();
                        return Err(CommandError::KeyParseError(token, e.error))
                    }
                    Err(ParseKeyMapError::To(e)) => {
                        let token = &to[e.index..];
                        let end = token.chars().next().map(char::len_utf8).unwrap_or(0);
                        let token = token[..end].into();
                        return Err(CommandError::KeyParseError(token, e.error))
                    }
                }
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "register",
        alias: "",
        help: "accesses an editor register\nregister <key> [<value>]",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;

            let key = ctx.args.next()?;
            let value = ctx.args.try_next()?;
            ctx.args.assert_empty()?;

            let register = match RegisterKey::from_str(key) {
                Some(key) => ctx.editor.registers.get_mut(key),
                None => return Err(CommandError::InvalidRegisterKey(key.into())),
            };
            match value {
                Some(value) => {
                    register.clear();
                    register.push_str(value);
                }
                None => {
                    ctx.output.clear();
                    ctx.output.push_str(register);
                }
            }

            Ok(None)
        },
    },
    // TODO: remove this command
    BuiltinCommand {
        name: "run",
        alias: "",
        help: "test command",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;

            let command = ctx.args.next()?;
            eprintln!("request spawn process '{}'", command);

            let mut command = Command::new(command);
            command.stdin(Stdio::null());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::null());

            while let Some(arg) = ctx.args.try_next()? {
                command.arg(arg);
            }

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
        name: "lsp-start",
        alias: "",
        help: concat!(
            "starts a lsp server\n",
            "lsp-start [<flags>] <command> [<command-arg>...]\n",
            " -root=<path> : the root path from where the lsp server will execute",
            " -log=<buffer-name> : redirect the lsp server stdout to this buffer"
        ),
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("root", None), ("log", None)];
            ctx.args.get_flags(&mut flags)?;
            let root = flags[0].1;
            let log_buffer = flags[1].1;

            let command = ctx.args.next()?;
            let mut command = Command::new(command);
            while let Some(arg) = ctx.args.try_next()? {
                command.arg(arg);
            }

            let root = match root {
                Some(root) => PathBuf::from(root),
                None => ctx.editor.current_directory.clone(),
            };

            let handle = ctx.editor.lsp.start(ctx.platform, command, root);
            let clients = &mut *ctx.clients;
            let client_handle = ctx.client_handle;

            if let Some(log_buffer) = log_buffer {
                lsp::ClientManager::access(ctx.editor, handle, |editor, client, _| {
                    let buffer = editor.buffers.new(BufferCapabilities::log());
                    let buffer_handle = buffer.handle();
                    buffer.set_path(Some(Path::new(log_buffer)));
                    client.set_log_buffer(Some(buffer_handle));

                    if let Some(client_handle) = client_handle {
                        let buffer_view_handle = editor
                            .buffer_views
                            .buffer_view_handle_from_buffer_handle(client_handle, buffer_handle);
                        if let Some(client) = clients.get_mut(client_handle) {
                            client.set_buffer_view_handle(Some(buffer_view_handle));
                        }
                    }
                });
            }

            use fmt::Write;
            ctx.output.clear();
            let _ = write!(ctx.output, "{}", handle);

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-stop",
        alias: "",
        help: "stops the lsp server associated with the current buffer\nlsp-stop",
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
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
        name: "lsp-hover",
        alias: "",
        help: "performs a lsp hover action at the current buffer's main cursor position\nlsp-hover",
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            let (buffer_handle, position) = current_buffer_and_main_position(&ctx)?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, client, json| {
                client.hover(editor, platform, json, buffer_handle, position)
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-signature-help",
        alias: "",
        help: concat!(
            "performs a lsp signature help action at the current buffer's main cursor position\n",
            "lsp-signature_help\n",
        ),
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            let (buffer_handle, position) = current_buffer_and_main_position(&ctx)?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, client, json| {
                client.signature_help(editor, platform, json, buffer_handle, position)
            })?;
            Ok(None)
        },
    },
];

fn current_buffer_and_main_position<'state, 'command>(
    ctx: &CommandContext<'state, 'command>,
) -> Result<(BufferHandle, BufferPosition), CommandError> {
    let view_handle = ctx.current_buffer_view_handle()?;
    let buffer_view = ctx
        .editor
        .buffer_views
        .get(view_handle)
        .ok_or(CommandError::NoBufferOpened)?;

    let buffer_handle = buffer_view.buffer_handle;
    let position = buffer_view.cursors.main_cursor().position;
    Ok((buffer_handle, position))
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
    ctx: &mut CommandContext,
    buffer_handle: BufferHandle,
    accessor: A,
) -> Result<(), CommandError>
where
    A: FnOnce(&mut Editor, &mut Platform, &mut lsp::Client, &mut Json),
{
    let editor = &mut *ctx.editor;
    let platform = &mut *ctx.platform;
    match find_lsp_client_for_buffer(editor, buffer_handle)
        .and_then(|h| lsp::ClientManager::access(editor, h, |e, c, j| accessor(e, platform, c, j)))
    {
        Some(()) => Ok(()),
        None => Err(CommandError::LspServerNotRunning),
    }
}
