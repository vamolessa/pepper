use std::{borrow::Cow, path::Path};

use crate::{
    buffer::Buffer,
    client::TargetClient,
    command::{
        BuiltinCommand, CommandArgs, CommandContext, CommandManager, CommandOperation,
        CompletionSource,
    },
    editor::Editor,
    editor::StatusMessageKind,
    navigation_history::NavigationHistory,
};

pub fn register_all(commands: &mut CommandManager) {
    const UNSAVED_CHANGES_ERROR: &str =
        "there are unsaved changes in buffers. try appending a '!' to command to force quit";

    fn any_buffer_needs_save(editor: &Editor) -> bool {
        editor.buffers.iter().any(|b| b.needs_save())
    }

    macro_rules! parse_switches {
        ($args:expr, $($name:ident,)*) => {
            $(let mut $name = false;)*
            for switch in $args.switches() {
                let switch = switch.as_str(&$args);
                match switch {
                    $(stringify!($name) => $name = true,)*
                    _ => return Err(format!("invalid switch '{}'", switch).into())
                }
            }
        }
    }

    macro_rules! parse_options {
        ($args:expr, $($name:ident,)*) => {
            $(let mut $name = None;)*
            for (key, value) in $args.options() {
                let key = key.as_str(&$args);
                let value = value.as_str(&$args);
                match key {
                    $(stringify!($name) => $name = Some(value),)*
                    _ => {
                        drop(value);
                        return Err(format!("invalid option '{}'", key).into());
                    }
                }
            }
        }
    }

    commands.register_builtin(BuiltinCommand {
        name: "quit",
        alias: Some("q"),
        help: "quits this client. append a '!' to force quit",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |ctx| {
            parse_switches!(ctx.args,);
            parse_options!(ctx.args,);

            if ctx.bang || !any_buffer_needs_save(ctx.editor) {
                Ok(Some(CommandOperation::Quit))
            } else {
                Err(Cow::Borrowed(UNSAVED_CHANGES_ERROR))
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "quit-all",
        alias: Some("qa"),
        help: "quits all clients. append a '!' to force quit all",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |ctx| {
            parse_switches!(ctx.args,);
            parse_options!(ctx.args,);

            if ctx.bang || !any_buffer_needs_save(ctx.editor) {
                Ok(Some(CommandOperation::QuitAll))
            } else {
                Err(Cow::Borrowed(UNSAVED_CHANGES_ERROR))
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "print",
        alias: None,
        help: "prints a message to the status bar",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |ctx| {
            parse_switches!(ctx.args, stderr,);
            parse_options!(ctx.args,);

            if stderr {
                for arg in ctx.args.values() {
                    eprint!("{}", arg.as_str(ctx.args));
                }
                eprintln!();
            } else {
                let mut w = ctx.editor.status_bar.write(StatusMessageKind::Info);
                for arg in ctx.args.values() {
                    w.str(arg.as_str(ctx.args));
                    w.str(" ");
                }
            }

            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "source",
        alias: None,
        help: "load a source file and execute its commands",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |ctx| {
            parse_switches!(ctx.args,);
            parse_options!(ctx.args,);

            //expect_args!(ctx.args, source);
            todo!();
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "open",
        alias: Some("o"),
        help: "open a buffer for editting",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |ctx| {
            parse_switches!(ctx.args,);
            parse_options!(ctx.args,);

            let target_client = TargetClient(ctx.client_index);
            NavigationHistory::save_client_snapshot(
                ctx.clients,
                target_client,
                &ctx.editor.buffer_views,
            );

            for path in ctx.args.values().iter() {
                let path = Path::new(path.as_str(ctx.args));
                let buffer_view_handle = ctx
                    .editor
                    .buffer_views
                    .buffer_view_handle_from_path(
                        target_client,
                        &mut ctx.editor.buffers,
                        &mut ctx.editor.word_database,
                        &ctx.editor.current_directory,
                        path,
                        None, // TODO: implement line_index
                        &mut ctx.editor.events,
                    )
                    .unwrap();
                ctx.clients.set_buffer_view_handle(
                    ctx.editor,
                    target_client,
                    Some(buffer_view_handle),
                );
            }
            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "close",
        alias: None,
        help: "open a buffer for editting",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |_| {
            todo!();
        },
    });
}

// buffer:
// - open
// - save
// - close[!]
// - close-all[!]
// - reload[!]
// - reload-all[!]
//
// process:
// - ???
//
// keymap:
// - map-normap
// - map-insert
// - map-read-line
// - map-picker
//
// others:
// - syntax-rules (???)
// - config
// - theme
// - register
//
// lsp:
// - lsp-start
// - lsp-stop
// - lsp-hover
// - lsp-signature-help
// - lsp-open-log
