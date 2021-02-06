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
        ($ctx:expr, $($name:ident,)*) => {
            $(let mut $name = false;)*
            for switch in $ctx.args.switches() {
                let switch = switch.as_str($ctx.args);
                match switch {
                    $(stringify!($name) => $name = true,)*
                    _ => return $ctx.error(format_args!("invalid switch '{}'", switch)),
                }
            }
        }
    }

    macro_rules! parse_options {
        ($ctx:expr, $($name:ident,)*) => {
            $(let mut $name = None;)*
            for (key, value) in $ctx.args.options() {
                let key = key.as_str($ctx.args);
                let value = value.as_str($ctx.args);
                match key {
                    $(stringify!($name) => $name = Some(value),)*
                    _ => {
                        drop(value);
                        return $ctx.error(format_args!("invalid option '{}'", key));
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
        func: |mut ctx| {
            parse_switches!(ctx,);
            parse_options!(ctx,);

            if ctx.bang || !any_buffer_needs_save(ctx.editor) {
                Some(CommandOperation::Quit)
            } else {
                ctx.error(format_args!("{}", UNSAVED_CHANGES_ERROR))
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "quit-all",
        alias: Some("qa"),
        help: "quits all clients. append a '!' to force quit all",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |mut ctx| {
            parse_switches!(ctx,);
            parse_options!(ctx,);

            if ctx.bang || !any_buffer_needs_save(ctx.editor) {
                Some(CommandOperation::QuitAll)
            } else {
                ctx.error(format_args!("{}", UNSAVED_CHANGES_ERROR))
            }
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "print",
        alias: None,
        help: "prints a message to the status bar",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |mut ctx| {
            parse_switches!(ctx, stderr,);
            parse_options!(ctx,);

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

            None
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "source",
        alias: None,
        help: "load a source file and execute its commands",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |mut ctx| {
            parse_switches!(ctx,);
            parse_options!(ctx,);

            for path in ctx.args.values() {
                let path = path.as_str(ctx.args);
                if !ctx.editor.load_config(ctx.clients, path) {
                    return Some(CommandOperation::Error);
                }
            }

            None
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "open",
        alias: Some("o"),
        help: "open a buffer for editting",
        completion_sources: CompletionSource::None as _,
        params: &[],
        func: |mut ctx| {
            parse_switches!(ctx,);
            parse_options!(ctx,);

            let target_client = TargetClient(ctx.client_index?);
            NavigationHistory::save_client_snapshot(
                ctx.clients,
                target_client,
                &ctx.editor.buffer_views,
            );

            for path in ctx.args.values() {
                let mut path = path.as_str(ctx.args);

                let mut line_index = None;
                if let Some(separator_index) = path.rfind(':') {
                    if let Ok(n) = path[(separator_index + 1)..].parse() {
                        let n: usize = n;
                        line_index = Some(n.saturating_sub(1));
                        path = &path[..separator_index];
                    }
                }

                let buffer_view_handle = ctx
                    .editor
                    .buffer_views
                    .buffer_view_handle_from_path(
                        target_client,
                        &mut ctx.editor.buffers,
                        &mut ctx.editor.word_database,
                        &ctx.editor.current_directory,
                        Path::new(path),
                        line_index,
                        &mut ctx.editor.events,
                    )
                    .unwrap();
                ctx.clients.set_buffer_view_handle(
                    ctx.editor,
                    target_client,
                    Some(buffer_view_handle),
                );
            }

            None
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
