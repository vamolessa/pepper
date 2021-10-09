use std::path::Path;

use pepper::{
    buffer::{BufferHandle, BufferProperties},
    command::{CommandContext, CommandError, CommandManager},
    cursor::Cursor,
    editor::Editor,
    editor_utils::parse_process_command,
    plugin::{PluginGuard, PluginHandle},
};

use crate::{
    client::{Client, ClientHandle, ClientOperation},
    LspPlugin,
};

pub fn register_commands(commands: &mut CommandManager, plugin_handle: PluginHandle) {
    let mut r = |name, completions, command_fn| {
        commands.register_command(Some(plugin_handle), name, completions, command_fn);
    };

    r("lsp", &[], |ctx| {
        let command = ctx.args.next()?;
        let glob = ctx.args.next()?;
        let log_path = ctx.args.try_next();
        ctx.args.assert_empty()?;

        let mut lsp = acquire(ctx);
        let result = match lsp.add_recipe(glob, command, None, log_path) {
            Ok(()) => Ok(()),
            Err(error) => Err(CommandError::InvalidGlob(error)),
        };
        release(ctx, lsp);
        result
    });

    r("lsp-open-log", &[], |ctx| {
        ctx.args.assert_empty()?;
        let client_handle = ctx.client_handle()?;
        let buffer_handle = ctx.current_buffer_handle()?;
        access(ctx, buffer_handle, |ctx, client| {
            let path = client
                .log_file_path()
                .ok_or(CommandError::OtherStatic("lsp server is not logging"))?;

            let buffer_view_handle = ctx
                .editor
                .buffer_view_handle_from_path(
                    client_handle,
                    Path::new(path),
                    BufferProperties::log(),
                    true,
                )
                .map_err(CommandError::BufferReadError)?;

            let client = ctx.clients.get_mut(client_handle);
            client.set_buffer_view_handle(
                Some(buffer_view_handle),
                &ctx.editor.buffer_views,
                &mut ctx.editor.events,
            );

            Ok(ClientOperation::None)
        })
    });

    r("lsp-start", &[], |ctx| {
        let command = ctx.args.next()?;
        let log_path = ctx.args.try_next();
        ctx.args.assert_empty()?;

        let command = parse_process_command(command).ok_or(CommandError::OtherOwned(format!(
            "invalid lsp command '{}'",
            command
        )))?;
        let root = ctx.editor.current_directory.clone();

        let mut lsp = acquire(ctx);
        lsp.start(
            ctx.editor,
            ctx.platform,
            ctx.plugin_handle(),
            command,
            root,
            log_path.map(String::from),
        );
        release(ctx, lsp);
        Ok(())
    });

    r("lsp-stop", &[], |ctx| {
        ctx.args.assert_empty()?;
        let buffer_handle = ctx.current_buffer_handle()?;
        let mut lsp = acquire(ctx);
        match find_lsp_client_for_buffer(&lsp, ctx.editor, buffer_handle) {
            Some(client) => lsp.stop(ctx.platform, client),
            None => lsp.stop_all(ctx.platform),
        }
        release(ctx, lsp);
        Ok(())
    });

    r("lsp-stop-all", &[], |ctx| {
        ctx.args.assert_empty()?;
        let mut lsp = acquire(ctx);
        lsp.stop_all(ctx.platform);
        release(ctx, lsp);
        Ok(())
    });

    r("lsp-hover", &[], |ctx| {
        ctx.args.assert_empty()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
        access(ctx, buffer_handle, |ctx, client| {
            let op = client.hover(ctx.editor, ctx.platform, buffer_handle, cursor.position);
            Ok(op)
        })
    });

    r("lsp-definition", &[], |ctx| {
        ctx.args.assert_empty()?;
        let client_handle = ctx.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
        access(ctx, buffer_handle, |ctx, client| {
            let op = client.definition(
                ctx.editor,
                ctx.platform,
                buffer_handle,
                cursor.position,
                client_handle,
            );
            Ok(op)
        })
    });

    r("lsp-declaration", &[], |ctx| {
        ctx.args.assert_empty()?;
        let client_handle = ctx.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
        access(ctx, buffer_handle, |ctx, client| {
            let op = client.declaration(
                ctx.editor,
                ctx.platform,
                buffer_handle,
                cursor.position,
                client_handle,
            );
            Ok(op)
        })
    });

    r("lsp-implementation", &[], |ctx| {
        ctx.args.assert_empty()?;
        let client_handle = ctx.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;
        access(ctx, buffer_handle, |ctx, client| {
            let op = client.implementation(
                ctx.editor,
                ctx.platform,
                buffer_handle,
                cursor.position,
                client_handle,
            );
            Ok(op)
        })
    });

    r("lsp-references", &[], |ctx| {
        let context_len = 2;
        ctx.args.assert_empty()?;

        let client_handle = ctx.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;

        access(ctx, buffer_handle, |ctx, client| {
            let op = client.references(
                ctx.editor,
                ctx.platform,
                buffer_handle,
                cursor.position,
                context_len,
                false,
                client_handle,
            );
            Ok(op)
        })
    });

    r("lsp-rename", &[], |ctx| {
        ctx.args.assert_empty()?;

        let client_handle = ctx.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;

        access(ctx, buffer_handle, |ctx, client| {
            let op = client.rename(
                ctx.editor,
                ctx.platform,
                ctx.clients,
                ctx.plugin_handle(),
                client_handle,
                buffer_handle,
                cursor.position,
            );
            Ok(op)
        })
    });

    r("lsp-code-action", &[], |ctx| {
        ctx.args.assert_empty()?;

        let client_handle = ctx.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(&ctx)?;

        access(ctx, buffer_handle, |ctx, client| {
            let op = client.code_action(
                ctx.editor,
                ctx.platform,
                ctx.plugin_handle(),
                client_handle,
                buffer_handle,
                cursor.to_range(),
            );
            Ok(op)
        })
    });

    r("lsp-document-symbol", &[], |ctx| {
        ctx.args.assert_empty()?;

        let client_handle = ctx.client_handle()?;
        let view_handle = ctx.current_buffer_view_handle()?;
        let buffer_handle = ctx.editor.buffer_views.get(view_handle).buffer_handle;

        access(ctx, buffer_handle, |ctx, client| {
            let op = client.document_symbols(ctx.editor, ctx.platform, client_handle, view_handle);
            Ok(op)
        })
    });

    r("lsp-workspace-symbols", &[], |ctx| {
        let query = ctx.args.try_next().unwrap_or("");
        ctx.args.assert_empty()?;

        let client_handle = ctx.client_handle()?;
        let buffer_handle = ctx.current_buffer_handle()?;

        access(ctx, buffer_handle, |ctx, client| {
            let op = client.workspace_symbols(ctx.editor, ctx.platform, client_handle, query);
            Ok(op)
        })
    });

    r("lsp-format", &[], |ctx| {
        ctx.args.assert_empty()?;
        let buffer_handle = ctx.current_buffer_handle()?;
        access(ctx, buffer_handle, |ctx, client| {
            let op = client.formatting(ctx.editor, ctx.platform, buffer_handle);
            Ok(op)
        })
    });
}

fn acquire(ctx: &mut CommandContext) -> PluginGuard<LspPlugin> {
    ctx.editor.plugins.acquire(ctx.plugin_handle())
}

fn release(ctx: &mut CommandContext, plugin: PluginGuard<LspPlugin>) {
    ctx.editor.plugins.release(plugin);
}

fn current_buffer_and_main_cursor(
    ctx: &CommandContext,
) -> Result<(BufferHandle, Cursor), CommandError> {
    let view_handle = ctx.current_buffer_view_handle()?;
    let buffer_view = ctx.editor.buffer_views.get(view_handle);

    let buffer_handle = buffer_view.buffer_handle;
    let cursor = *buffer_view.cursors.main_cursor();
    Ok((buffer_handle, cursor))
}

fn find_lsp_client_for_buffer(
    lsp: &LspPlugin,
    editor: &Editor,
    buffer_handle: BufferHandle,
) -> Option<ClientHandle> {
    let buffer_path = editor.buffers.get(buffer_handle).path.to_str()?;
    let client = lsp.clients().find(|c| c.handles_path(buffer_path))?;
    Some(client.handle())
}

fn access<A>(
    ctx: &mut CommandContext,
    buffer_handle: BufferHandle,
    accessor: A,
) -> Result<(), CommandError>
where
    A: FnOnce(&mut CommandContext, &mut Client) -> Result<ClientOperation, CommandError>,
{
    let mut lsp = acquire(ctx);

    let access_with_lsp = || {
        let client_handle = find_lsp_client_for_buffer(&lsp, ctx.editor, buffer_handle);
        let client = client_handle
            .and_then(|h| lsp.get_mut(h))
            .ok_or(CommandError::OtherStatic("lsp server not running"))?;
        let op = accessor(ctx, client)?;
        let client_handle = client.handle();
        lsp.on_client_operation(client_handle, op);
        Ok(())
    };
    let result = access_with_lsp();

    release(ctx, lsp);
    result
}
