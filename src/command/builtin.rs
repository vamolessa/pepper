use crate::{
    buffer::Buffer,
    command::{BuiltinCommand, CommandContext, CommandManager, CommandOperation, CompletionSource},
    editor::Editor,
    editor::StatusMessageKind,
};

pub fn register_all(commands: &mut CommandManager) {
    fn error(editor: &mut Editor, message: &str) {
        editor
            .status_bar
            .write(StatusMessageKind::Error)
            .str(message);
    }

    fn any_buffer_needs_save(editor: &Editor) -> bool {
        editor.buffers.iter().any(|b| b.needs_save())
    }

    macro_rules! expect_empty_args {
        ($ctx:expr) => {
            if $ctx.args.next().is_some() {
                error($ctx.editor, "too many arguments were passed to command");
                return None;
            }
        };
    }

    macro_rules! expect_args {
        ($ctx:expr, $($arg:ident),*) => {
            $(let $arg = match $ctx.args.next() {
                Some(arg) => arg,
                None => {
                    error($ctx.editor, "too few arguments were passed to command");
                    return None;
                }
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
                Some(CommandOperation::Quit)
            } else {
                error(ctx.editor, "there are unsaved changes in buffers. try appending a '!' to command to force quit");
                None
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
                Some(CommandOperation::QuitAll)
            } else {
                error(ctx.editor, "there are unsaved changes in buffers. try appending a '!' to command to force quit");
                None
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
            None
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
            None
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
            todo!("open {}", path);
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
