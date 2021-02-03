use std::{borrow::Cow, path::Path};

use crate::{
    buffer::Buffer,
    client::TargetClient,
    command::{BuiltinCommand, CommandContext, CommandManager, CommandOperation, CompletionSource},
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

    macro_rules! expect_empty_args {
        ($ctx:expr) => {
            if $ctx.args.next().is_some() {
                return Err(Cow::Borrowed("too many arguments were passed to command"));
            }
        };
    }

    macro_rules! expect_args {
        ($ctx:expr, $($arg:ident),*) => {
            $(let $arg = match $ctx.args.next() {
                Some(arg) => arg,
                None => return Err(Cow::Borrowed("too few arguments were passed to command")),
            };)*
        }
    }

    commands.register_builtin(BuiltinCommand {
        name: "quit",
        alias: Some("q"),
        help: "quits this client. append a '!' to force quit",
        completion_sources: CompletionSource::None as _,
        func: |mut ctx| {
            expect_empty_args!(ctx);
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
        func: |mut ctx| {
            expect_empty_args!(ctx);
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
        func: |ctx| {
            let mut w = ctx.editor.status_bar.write(StatusMessageKind::Info);
            for arg in ctx.args {
                w.str(arg);
            }
            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "eprint",
        alias: None,
        help: "prints a message to the server's stderr",
        completion_sources: CompletionSource::None as _,
        func: |ctx| {
            for arg in ctx.args {
                eprint!("{}", arg);
            }
            eprintln!();
            Ok(None)
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "source",
        alias: None,
        help: "load a source file and execute its commands",
        completion_sources: CompletionSource::None as _,
        func: |mut ctx| {
            expect_args!(ctx, source);
            todo!("source {}", source);
        },
    });

    commands.register_builtin(BuiltinCommand {
        name: "open",
        alias: None,
        help: "open a buffer for editting",
        completion_sources: CompletionSource::None as _,
        func: |mut ctx| {
            expect_args!(ctx, path);

            let target_client = TargetClient(ctx.client_index);
            NavigationHistory::save_client_snapshot(
                ctx.clients,
                target_client,
                &ctx.editor.buffer_views,
            );

            let path = Path::new(path);
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
                &mut ctx.editor,
                target_client,
                Some(buffer_view_handle),
            );
            Ok(None)
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
