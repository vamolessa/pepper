use std::{
    any, fmt,
    fs::File,
    io,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    buffer::BufferHandle,
    buffer_position::BufferPosition,
    client::ClientManager,
    command::{
        parse_process_command, BuiltinCommand, CommandContext, CommandError, CommandIter,
        CommandManager, CommandOperation, CommandSource, CommandTokenIter, CommandTokenKind,
        CompletionSource, MacroCommand, RequestCommand,
    },
    config::{ParseConfigError, CONFIG_NAMES},
    cursor::{Cursor, CursorCollection},
    editor::{Editor, EditorControlFlow},
    editor_utils::MessageKind,
    keymap::ParseKeyMapError,
    lsp,
    mode::{picker, read_line, Mode, ModeContext, ModeKind},
    navigation_history::NavigationHistory,
    platform::{Platform, SharedBuf},
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
        help: concat!(
            "Prints help about <command-name>.\n",
            "If <command-name> is not present, prints the name of all commands available.\n",
            "\n",
            "help [<command-name>]",
        ),
        hidden: false,
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
                        CommandSource::Macro(i) => {
                            let command = &commands.macro_commands()[i];
                            ("", &command.help[..])
                        }
                        CommandSource::Request(i) => {
                            let command = &commands.request_commands()[i];
                            ("", &command.help[..])
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
            "Try executing commands without propagating errors.\n",
            "Then optionally executes commands if there was an error.\n",
            "\n",
            "try { <commands...> } [catch { <commands...> }]",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            fn run_commands(
                ctx: &mut CommandContext,
                commands: &str
            ) -> Result<Option<CommandOperation>, CommandError> {
                for command in CommandIter(commands) {
                    match CommandManager::eval(
                        ctx.editor,
                        ctx.platform,
                        ctx.clients,
                        ctx.client_handle,
                        command,
                        ctx.source_path,
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

            let try_commands = ctx.args.next()?;
            let catch_keyword = ctx.args.try_next()?;
            let catch_commands = if let Some(catch_keyword) = catch_keyword {
                if catch_keyword != "catch" {
                    return Err(CommandError::InvalidToken(catch_keyword.into()));
                }

                Some(ctx.args.next()?)
            } else {
                None
            };

            match run_commands(ctx, try_commands) {
                Ok(op) => Ok(op),
                Err(_) => match catch_commands {
                    Some(commands) => run_commands(ctx, commands),
                    None => Ok(None),
                }
            }
        },
    },
    BuiltinCommand {
        name: "macro",
        alias: "",
        help: concat!(
            "Defines a new macro command.\n",
            "\n",
            "macro [<flags>] <name> <param-names...> <commands>\n",
            " -help=<help-text> : the help text that shows when using `help` with this command\n",
            " -hidden : whether this command is shown in completions or not",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("help", None), ("hidden", None)];
            ctx.args.get_flags(&mut flags)?;
            let help = flags[0].1.unwrap_or("");
            let hidden = flags[1].1.is_some();

            let name = ctx.args.next()?;

            let mut params = Vec::new();
            params.push(ctx.args.next()?.into());
            while let Some(param) = ctx.args.try_next()? {
                params.push(param.into());
            }
            ctx.args.assert_empty()?;

            let commands = params.pop().unwrap();

            if name.is_empty() {
                return Err(CommandError::InvalidCommandName(name.into()));
            }
            if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
                return Err(CommandError::InvalidCommandName(name.into()));
            }

            let command = MacroCommand {
                name: name.into(),
                help: help.into(),
                hidden,
                params,
                commands,
                source_path: ctx.source_path.map(Into::into),
            };
            ctx.editor.commands.register_macro(command);

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "request",
        alias: "",
        help: concat!(
            "Register a request command for this client.\n",
            "The client needs to implement the editor protocol.\n",
            "Because of that, it only makes sense to use this if it's called from a custom client.\n",
            "\n",
            "request [<flags>] <name>\n",
            " -help=<help-text> : the help text that shows when using `help` with this command\n",
            " -hidden : whether this command is shown in completions or not",
        ),
        hidden: true,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("help", None), ("hidden", None)];
            ctx.args.get_flags(&mut flags)?;
            let help = flags[0].1.unwrap_or("");
            let hidden = flags[1].1.is_some();

            let name = ctx.args.next()?;
            ctx.args.assert_empty()?;

            if name.is_empty() {
                return Err(CommandError::InvalidCommandName(name.into()));
            }
            if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
                return Err(CommandError::InvalidCommandName(name.into()));
            }

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };

            let command = RequestCommand {
                name: name.into(),
                help: help.into(),
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
        help: concat!(
            "Sets the command to be used when copying text to clipboard.\n",
            "The copied text is written to stdin utf8 encoded.\n",
            "This is most useful on platforms that do not have an unique way to interact with the clipboard.\n",
            "If <command> is empty, no command is used.\n",
            "\n",
            "copy-command <command>",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            let command = ctx.args.next()?;
            ctx.platform.copy_command.clear();
            ctx.platform.copy_command.push_str(command);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "paste-command",
        alias: "",
        help: concat!(
            "Sets the command to be used when pasting text from clipboard.\n",
            "The pasted text is read from stdout and needs to be utf8 encoded.\n",
            "This is most useful on platforms that do not have an unique way to interact with the clipboard.\n",
            "If <command> is empty, no command is used.\n",
            "\n",
            "paste-command <command>",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            let command = ctx.args.next()?;
            ctx.platform.paste_command.clear();
            ctx.platform.paste_command.push_str(command);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "spawn",
        alias: "",
        help: concat!(
            "Spawns a new process and then optionally executes commands on its output.\n",
            "Those commands will be executed on every splitted output if `-split-on-byte` is set\n",
            "or on its etirety when the process exits otherwise.\n",
            "`<output-var-name>` will be text replaced in `<commands-on-output>` with the process' output.\n",
            "\n",
            "spawn [<flags>] <spawn-command> [<output-var-name> <commands-on-output>]\n",
            " -input=<text> : sends <text> to the stdin\n",
            " -env=<vars> : sets environment variables in the form VAR=<value> VAR=<value>...\n",
            " -split-on-byte=<number> : splits process output at every <number> byte",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("input", None), ("env", None), ("split-on-byte", None)];
            ctx.args.get_flags(&mut flags)?;
            let input = flags[0].1;
            let env = flags[1].1.unwrap_or("");
            let split_on_byte = match flags[2].1 {
                Some(token) => match token.parse() {
                    Ok(b) => Some(b),
                    Err(_) => return Err(CommandError::InvalidToken(token.into())),
                }
                None => None,
            };

            let command = ctx.args.next()?;
            let output_name = ctx.args.try_next()?;
            let on_output = match output_name {
                Some(_) => Some(ctx.args.next()?),
                None => None,
            };
            ctx.args.assert_empty()?;

            let command = parse_process_command(command, env)?;
            ctx.editor.commands.spawn_process(
                ctx.platform,
                ctx.client_handle,
                command,
                input,
                output_name,
                on_output,
                split_on_byte
            );

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "replace-with-text",
        alias: "",
        help: concat!(
            "Replace each cursor selection with text.\n",
            "\n",
            "replace-with-text <text>",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            let text = ctx.args.next()?;
            ctx.args.assert_empty()?;

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
            ctx.editor.trigger_event_handlers(ctx.platform, ctx.clients);

            let buffer_view = match ctx.editor.buffer_views.get_mut(buffer_view_handle) {
                Some(buffer_view) => buffer_view,
                None => return Ok(None),
            };
            buffer_view.insert_text_at_cursor_positions(
                &mut ctx.editor.buffers,
                &mut ctx.editor.word_database,
                text,
                &mut ctx.editor.events,
            );

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "replace-with-output",
        alias: "",
        help: concat!(
            "Replace each cursor selection with command output.\n",
            "\n",
            "replace-with-output [<flags>] <command>\n",
            " -pipe : also pipes selected text to command's input\n",
            " -env=<vars> : sets environment variables in the form VAR=<value> VAR=<value>...\n",
            " -split-on-byte=<number> : splits output at every <number> byte",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("pipe", None), ("env", None), ("split-on-byte", None)];
            ctx.args.get_flags(&mut flags)?;
            let pipe = flags[0].1.is_some();
            let env = flags[1].1.unwrap_or("");
            let split_on_byte = match flags[2].1 {
                Some(token) => match token.parse() {
                    Ok(b) => Some(b),
                    Err(_) => return Err(CommandError::InvalidToken(token.into())),
                }
                None => None,
            };

            let text_or_command = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let buffer_view_handle = ctx.current_buffer_view_handle()?;
            let buffer_view = match ctx.editor.buffer_views.get_mut(buffer_view_handle) {
                Some(buffer_view) => buffer_view,
                None => return Err(CommandError::NoBufferOpened),
            };

            const NONE_BUF : Option<SharedBuf> = None;
            let mut stdins = [NONE_BUF; CursorCollection::capacity()];

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
            ctx.editor.trigger_event_handlers(ctx.platform, ctx.clients);

            let buffer_view = match ctx.editor.buffer_views.get_mut(buffer_view_handle) {
                Some(buffer_view) => buffer_view,
                None => return Ok(None),
            };

            for (i, cursor) in buffer_view.cursors[..].iter().enumerate() {
                let range = cursor.to_range();
                let command = parse_process_command(text_or_command, env)?;

                let buffer = match ctx.editor.buffers.get(buffer_view.buffer_handle) {
                    Some(buffer) => buffer,
                    None => return Ok(None),
                };

                let buffer_handle = buffer.handle();
                ctx.editor.buffers.spawn_insert_process(
                    ctx.platform,
                    command,
                    buffer_handle,
                    range.from,
                    stdins[i].take(),
                    split_on_byte,
                );
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "execute-keys",
        alias: "",
        help: concat!(
            "Executes keys as if they were inputted manually.\n",
            "\n",
            "execute-keys [<flags>] <keys>\n",
            " -client=<client-id> : send keys on behalf of client <client-id>",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("client", None)];
            ctx.args.get_flags(&mut flags)?;
            let client = match flags[0].1 {
                Some(token) => match token.parse() {
                    Ok(handle) => Some(handle),
                    Err(_) => return Err(CommandError::InvalidToken(token.into())),
                }
                None => ctx.client_handle,
            };

            let keys = ctx.args.next()?;
            ctx.args.assert_empty()?;

            match client {
                Some(client_handle) => match ctx.editor.buffered_keys.parse(keys) {
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
                            keys
                        ) {
                            EditorControlFlow::Continue => Ok(None),
                            EditorControlFlow::Quit => Ok(Some(CommandOperation::Quit)),
                            EditorControlFlow::QuitAll => Ok(Some(CommandOperation::QuitAll)),
                        };
                        Mode::change_to(&mut ctx, mode);
                        op
                    }
                    Err(_) => Err(CommandError::BufferedKeysParseError(keys.into())),
                }
                None => Ok(None)
            }
        }
    },
    BuiltinCommand {
        name: "read-line",
        alias: "",
        help: concat!(
            "Prompts for a line read and then executes commands.\n",
            "<line-var-name> will be text replaced in `<commands>` with the line read value.\n",
            "\n",
            "read-line [<flags>] <line-var-name> <commands>\n",
            " -prompt=<prompt-text> : the prompt text that shows just before user input (default: `read-line:`)",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("prompt", None)];
            ctx.args.get_flags(&mut flags)?;
            let prompt = flags[0].1.unwrap_or("read-line:");

            let line_var_name = ctx.args.next()?;
            let commands = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle{
                Some(handle) => handle,
                None => return Ok(None),
            };

            ctx.editor.read_line.set_prompt(prompt);

            let mut mode_ctx = ModeContext {
                editor: ctx.editor,
                platform: ctx.platform,
                clients: ctx.clients,
                client_handle,
            };
            read_line::custom::enter_mode(&mut mode_ctx, commands, line_var_name);

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "pick",
        alias: "",
        help: concat!(
            "Opens up a menu from where an option can be picked and then executes commands.\n",
            "Options can be added with the `add-picker-entry` command.\n",
            "`<option-var-name>` will be text replaced in `<commands>` with the picked option value.\n",
            "\n",
            "pick [<flags>] <option-var-name> <commands>\n",
            " -prompt=<prompt-text> : the prompt text that shows just before user input (default: `pick:`)",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("prompt", None)];
            let prompt = flags[0].1.unwrap_or("pick:");
            ctx.args.get_flags(&mut flags)?;

            let option_var_name = ctx.args.next()?;
            let commands = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle{
                Some(handle) => handle,
                None => return Ok(None),
            };

            ctx.editor.read_line.set_prompt(prompt);

            let mut mode_ctx = ModeContext {
                editor: ctx.editor,
                platform: ctx.platform,
                clients: ctx.clients,
                client_handle,
            };
            picker::custom::enter_mode(&mut mode_ctx, commands, option_var_name);

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "add-picker-option",
        alias: "",
        help: concat!(
            "Adds a new picker option that will then be shown in the next call to the `pick` command.\n",
            "\n",
            "add-picker-option <name>",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            let name = ctx.args.next()?;
            ctx.args.assert_empty()?;

            if ModeKind::Picker == ctx.editor.mode.kind() {
                ctx.editor.picker.add_custom_entry_filtered(
                    name,
                    ctx.editor.read_line.input()
                );
                ctx.editor.picker.move_cursor(0);
            }

            Ok(None)
        },
    },
    BuiltinCommand {
        name: "quit",
        alias: "q",
        help: concat!(
            "Quits this client.\n",
            "With '!' will discard any unsaved changes.\n",
            "\n",
            "quit[!]",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            if ctx.clients.iter().count() == 1 {
                ctx.assert_can_discard_all_buffers()?;
            }
            Ok(Some(CommandOperation::Quit))
        },
    },
    BuiltinCommand {
        name: "quit-all",
        alias: "qa",
        help: concat!(
            "Quits all clients.\n",
            "With '!' will discard any unsaved changes.\n",
            "\n",
            "quit-all[!]",
        ),
        hidden: false,
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
            "Prints arguments to the status bar\n",
            "\n",
            "print [<flags>] <values...>\n",
            " -error : will print as an error",
            " -dbg : will also print to the stderr",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("error", None), ("dbg", None)];
            ctx.args.get_flags(&mut flags)?;
            let error = flags[0].1.is_some();
            let dbg = flags[1].1.is_some();

            let message_kind = if error {
                MessageKind::Error
            } else {
                MessageKind::Info
            };

            let mut write = ctx.editor.status_bar.write(message_kind);
            while let Some(arg) = ctx.args.try_next()? {
                write.str(arg);

                if dbg {
                    eprint!("{}", arg);
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
        help: concat!(
            "Loads a source file and execute its commands.\n",
            "\n",
            "source <path>",
        ),
        hidden: false,
        completions: &[CompletionSource::Files],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;

            let path = ctx.args.next()?;
            let path_token = path.into();
            let path = Path::new(path);
            ctx.args.assert_empty()?;

            let mut path_buf = PathBuf::new();
            let path = if path.is_relative() {
                if let Some(parent) = ctx.source_path.and_then(Path::parent) {
                    path_buf.push(parent);
                }
                path_buf.push(path);
                path_buf.as_path()
            } else {
                path
            };

            use io::Read;
            let mut file = File::open(path)
                .map_err(|e| CommandError::OpenFileError { path: path_token, error: e })?;
            let mut source = ctx.editor.string_pool.acquire();
            file.read_to_string(&mut source)
                .map_err(|e| CommandError::OpenFileError { path: path_token, error: e })?;

            let op = CommandManager::eval_commands_then_output(
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
        help: concat!(
            "Opens a buffer up for editting.\n",
            "\n",
            "open [<flags>] <path>\n",
            " -line=<number> : set cursor at line\n",
            " -column=<number> : set cursor at column\n",
            " -no-history : disables undo/redo\n",
            " -no-save : disables saving\n",
            " -no-word-database : words in this buffer will not contribute to the word database\n",
            " -auto-close : automatically closes buffer when no other client has it in focus",
        ),
        hidden: false,
        completions: &[CompletionSource::Files],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [
                ("line", None),
                ("column", None),
                ("no-history", None),
                ("no-save", None),
                ("no-word-database", None),
                ("auto-close", None)
            ];
            ctx.args.get_flags(&mut flags)?;
            let line = flags[0]
                .1
                .map(parse_arg::<u32>)
                .transpose()?;
            let column = flags[1]
                .1
                .map(parse_arg::<u32>)
                .transpose()?;
            let no_history = flags[2].1.is_some();
            let no_save = flags[3].1.is_some();
            let no_word_database = flags[4].1.is_some();
            let auto_close = flags[5].1.is_some();

            let path = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };

            let position = match (line, column) {
                (Some(line), Some(column)) => {
                    let line = line.saturating_sub(1) as _;
                    let column = column.saturating_sub(1) as _;
                    Some(BufferPosition::line_col(line, column))
                }
                (Some(line), None) => {
                    let line = line.saturating_sub(1) as _;
                    Some(BufferPosition::line_col(line, 0))
                }
                (None, Some(column)) => {
                    let column = column.saturating_sub(1) as _;
                    Some(BufferPosition::line_col(0, column))
                }
                (None, None) => None,
            };

            NavigationHistory::save_client_snapshot(
                ctx.clients,
                client_handle,
                &ctx.editor.buffer_views,
            );

            let handle = ctx.editor.buffer_view_handle_from_path(
                client_handle,
                Path::new(path),
            );

            if let Some(buffer_view) = ctx.editor.buffer_views.get_mut(handle) {
                if let Some(position) = position {
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
        help: concat!(
            "Save buffer to file.\n",
            "\n",
            "save [<flags>] [<path>]\n",
            " -buffer=<buffer-id> : if not specified, the current buffer is used",
        ),
        hidden: false,
        completions: &[],
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

            ctx.editor
                .status_bar
                .write(MessageKind::Info)
                .fmt(format_args!("buffer saved to {:?}", buffer.path()));
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "save-all",
        alias: "sa",
        help: concat!(
            "Save all buffers to file.\n",
            "\n",
            "save-all",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

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
        help: concat!(
            "Reload buffer from file.\n",
            "With '!' will discard any unsaved changes.\n",
            "\n",
            "reload[!] [<flags>]\n",
            " -buffer=<buffer-id> : if not specified, the current buffer is used",
        ),
        hidden: false,
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
        help: concat!(
            "Reload all buffers from file.\n",
            "With '!' will discard any unsaved changes\n",
            "\n",
            "reload-all[!]",
        ),
        hidden: false,
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
            "Close buffer and opens previous buffer in view if any.\n",
            "With '!' will discard any unsaved changes\n",
            "\n",
            "close[!] [<flags>]\n",
            " -buffer=<buffer-id> : if not specified, the current buffer is used\n",
            " -no-previous-buffer : does not try to open previous buffer",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            let mut flags = [("buffer", None), ("no-previous-buffer", None)];
            ctx.args.get_flags(&mut flags)?;
            let buffer_handle = flags[0].1.map(parse_arg).transpose()?;
            let no_previous_buffer = flags[1].1.is_some();

            ctx.args.assert_empty()?;

            let buffer_handle = match buffer_handle {
                Some(handle) => handle,
                None => ctx.current_buffer_handle()?,
            };

            ctx.assert_can_discard_buffer(buffer_handle)?;
            ctx.editor.buffers.defer_remove(buffer_handle, &mut ctx.editor.events);

            if !no_previous_buffer {
                let clients = &mut *ctx.clients;
                if let Some(client) = ctx.client_handle.and_then(|h| clients.get_mut(h)) {
                    client.set_buffer_view_handle(client.previous_buffer_view_handle(), &mut ctx.editor.events);
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
        help: concat!(
            "Close all buffers.\n",
            "With '!' will discard any unsaved changes.\n",
            "\n",
            "close-all[!]\n",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            ctx.assert_can_discard_all_buffers()?;
            let mut count = 0;
            for buffer in ctx.editor.buffers.iter() {
                ctx.editor.buffers.defer_remove(buffer.handle(), &mut ctx.editor.events);
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
        help: concat!(
            "Accesses an editor config.\n",
            "\n",
            "config <key> [<value>]",
        ),
        hidden: false,
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
        help: concat!(
            "Accesses an editor theme color.\n",
            "\n",
            "color <key> [<value>]",
        ),
        hidden: false,
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
            "Creates a syntax definition from patterns for files that match a glob.\n",
            "Every line in <definition> should be of the form:\n",
            "<token-kind> = <pattern>\n",
            "Where <token-kind> is one of:\n",
            " keywords\n",
            " types\n",
            " symbols\n",
            " literals\n",
            " strings\n",
            " comments\n",
            " texts\n",
            "And <pattern> is the pattern that matches that kind of token.\n",
            "\n",
            "syntax <glob> { <definition> }",
        ),
        hidden: true,
        completions: &[],
        func: |ctx| {
            fn slice_from_last_char(s: &str) -> &str {
                let end = s.char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
                &s[end..]
            }

            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;

            let glob = ctx.args.next()?;
            let definition = ctx.args.next()?;
            ctx.args.assert_empty()?;

            let mut syntax = Syntax::new();
            syntax
                .set_glob(glob.as_bytes())
                .map_err(|_| CommandError::InvalidGlob(glob.into()))?;

            let mut definition_tokens = CommandTokenIter(definition);
            loop {
                let token_kind = match definition_tokens.next() {
                    Some((CommandTokenKind::Text, token)) => token,
                    Some((CommandTokenKind::Unterminated, token)) => {
                        return Err(CommandError::UnterminatedToken(token.into()))
                    }
                    Some((_, token)) => return Err(CommandError::InvalidToken(token.into())),
                    None => break,
                };
                let token_kind = match token_kind {
                    "keywords" => TokenKind::Keyword,
                    "types" => TokenKind::Type,
                    "symbols" => TokenKind::Symbol,
                    "literals" => TokenKind::Literal,
                    "strings" => TokenKind::String,
                    "comments" => TokenKind::Comment,
                    "texts" => TokenKind::Text,
                    _ => return Err(CommandError::InvalidToken(token_kind.into())),
                };
                match definition_tokens.next() {
                    Some((CommandTokenKind::Equals, _)) => (),
                    Some((CommandTokenKind::Unterminated, token)) => {
                        return Err(CommandError::UnterminatedToken(token.into()));
                    }
                    Some((_, token)) => {
                        return Err(CommandError::InvalidToken(token.into()));
                    }
                    None => {
                        let end = slice_from_last_char(definition);
                        return Err(CommandError::SyntaxExpectedEquals(end.into()));
                    }
                }
                let pattern = match definition_tokens.next() {
                    Some((CommandTokenKind::Text, token)) => token,
                    Some((CommandTokenKind::Unterminated, token)) => {
                        return Err(CommandError::UnterminatedToken(token.into()));
                    }
                    Some((_, token)) => return Err(CommandError::InvalidToken(token.into())),
                    None => {
                        let end = slice_from_last_char(definition);
                        return Err(CommandError::SyntaxExpectedPattern(end.into()));
                    }
                };

                if let Err(error) = syntax.set_rule(token_kind, pattern) {
                    return Err(CommandError::PatternError(pattern.into(), error));
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
            "Creates a keyboard mapping for an editor mode.\n",
            "\n",
            "map [<flags>] <from> <to>\n",
            " -normal : set mapping for normal mode\n",
            " -insert : set mapping for insert mode\n",
            " -read-line : set mapping for read-line mode\n",
            " -picker : set mapping for picker mode\n",
            " -command : set mapping for command mode",
        ),
        hidden: false,
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
        help: concat!(
            "Accesses an editor register.\n",
            "\n",
            "register <key> [<value>]",
        ),
        hidden: false,
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
    BuiltinCommand {
        name: "lsp",
        alias: "",
        help: concat!(
            "Automatically starts a lsp server when a buffer matching a glob is opened.\n",
            "The lsp command only runs if the server is not already running.\n",
            "\n",
            "lsp [<flags>] <glob> <lsp-command>\n",
            " -log=<buffer-name> : redirects the lsp server output to this buffer\n",
            " -env=<vars> : sets environment variables in the form VAR=<value> VAR=<value>...",
        ),
        hidden: true,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("root", None), ("log", None), ("env", None)];
            ctx.args.get_flags(&mut flags)?;
            let root = flags[0].1;
            let log_buffer = flags[1].1;
            let env = flags[2].1.unwrap_or("");

            let glob = ctx.args.next()?;
            let command = ctx.args.next()?;

            ctx
                .editor
                .lsp
                .add_recipe(glob.as_bytes(), command, env, root.map(Path::new), log_buffer)
                .map_err(|_| CommandError::InvalidGlob(glob.into()))?;
            Ok(None)
        }
    },
    BuiltinCommand {
        name: "lsp-start",
        alias: "",
        help: concat!(
            "Manually starts a lsp server.\n",
            "\n",
            "lsp-start [<flags>] <lsp-command>\n",
            " -root=<path> : the root path from where the lsp server will execute\n",
            " -log=<buffer-name> : redirects the lsp server output to this buffer\n",
            " -env=<vars> : sets environment variables in the form VAR=<value> VAR=<value>...",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("root", None), ("log", None), ("env", None)];
            ctx.args.get_flags(&mut flags)?;
            let root = flags[0].1;
            let log_buffer = flags[1].1;
            let env = flags[2].1.unwrap_or("");

            let command = ctx.args.next()?;
            let command = parse_process_command(command, env)?;

            let root = match root {
                Some(root) => PathBuf::from(root),
                None => ctx.editor.current_directory.clone(),
            };

            ctx.editor.lsp.start(ctx.platform, &mut ctx.editor.buffers, command, root, log_buffer);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-stop",
        alias: "",
        help: concat!(
            "Stops the lsp server associated with the current buffer.\n",
            "\n",
            "lsp-stop",
        ),
        hidden: false,
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
        name: "lsp-stop-all",
        alias: "",
        help: concat!(
            "Stops all lsp servers.\n",
            "\n",
            "lsp-stop-all",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            ctx.editor.lsp.stop_all(ctx.platform);
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-hover",
        alias: "",
        help: concat!(
            "Displays lsp hover information for the current buffer's main cursor position.\n",
            "\n",
            "lsp-hover",
        ),
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
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
        alias: "",
        help: concat!(
            "Jumps to the location of the definition of the item under the main cursor found by the lsp server.\n",
            "\n",
            "lsp-definition",
        ),
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.definition(editor, platform, buffer_handle, cursor.position, client_handle)
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-references",
        alias: "",
        help: concat!(
            "Opens up a buffer with all references of the item under the main cursor found by the lsp server.\n",
            "\n",
            "lsp-references [<flags>]\n",
            " -context=<number> : how many lines of context to show. 0 means no context is shown\n",
            " -auto-close : automatically closes buffer when no other client has it in focus",
        ),
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("context", None), ("auto-close", None)];
            ctx.args.get_flags(&mut flags)?;
            let context_len = flags[0].1.map(parse_arg).transpose()?.unwrap_or(0);
            let auto_close_buffer = flags[1].1.is_some();

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
                    auto_close_buffer,
                    client_handle
                )
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-rename",
        alias: "",
        help: concat!(
            "Renames the item under the main cursor through the lsp server.\n",
            "\n",
            "lsp-rename",
        ),
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, clients, client| {
                client.rename(
                    editor,
                    platform,
                    clients,
                    client_handle,
                    buffer_handle,
                    cursor.position,
                )
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-code-action",
        alias: "",
        help: concat!(
            "Lists and then performs a code action based on the main cursor context.\n",
            "\n",
            "lsp-code-action",
        ),
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
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
        alias: "",
        help: concat!(
            "Pick and jump to a symbol in the current buffer listed by the lsp server.\n",
            "\n",
            "lsp-document-symbols",
        ),
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let view_handle = ctx.current_buffer_view_handle()?;
            let buffer_view = ctx.editor.buffer_views.get(view_handle).ok_or(CommandError::NoBufferOpened)?;
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
        help: concat!(
            "Opens up a buffer with all symbols in the workspace found by the lsp server\n",
            "optionally filtered by a query\n",
            "\n",
            "lsp-workspace-symbols [<flags>] [<query>]\n",
            " -auto-close : automatically closes buffer when no other client has it in focus",
        ),
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;

            let mut flags = [("auto-close", None)];
            ctx.args.get_flags(&mut flags)?;
            let auto_close_buffer = flags[0].1.is_some();

            let query = ctx.args.try_next()?.unwrap_or("");
            ctx.args.assert_empty()?;

            let client_handle = match ctx.client_handle {
                Some(handle) => handle,
                None => return Ok(None),
            };
            let buffer_handle = ctx.current_buffer_handle()?;
            access_lsp(&mut ctx, buffer_handle, |editor, platform, _, client| {
                client.workspace_symbols(editor, platform, client_handle, query, auto_close_buffer)
            })?;
            Ok(None)
        },
    },
    BuiltinCommand {
        name: "lsp-format",
        alias: "",
        help: concat!(
            "Format a buffer using the lsp server.\n",
            "\n",
            "lsp-format",
        ),
        hidden: false,
        completions: &[],
        func: |mut ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
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
        alias: "",
        help: concat!(
            "Prints debug information abount running lsp servers running.\n",
            "\n",
            "lsp-debug",
        ),
        hidden: false,
        completions: &[],
        func: |ctx| {
            ctx.args.assert_no_bang()?;
            ctx.args.get_flags(&mut [])?;
            ctx.args.assert_empty()?;

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
            let _ = writeln!(message, "\nbuffer count: {}", ctx.editor.buffers.iter().count());
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
    let buffer_path_bytes = editor
        .buffers
        .get(buffer_handle)?
        .path()
        .to_str()?
        .as_bytes();
    let client = editor
        .lsp
        .clients()
        .find(|c| c.handles_path(buffer_path_bytes))?;
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

