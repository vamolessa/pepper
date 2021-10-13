use std::path::Path;

use pepper::{
    buffer::{BufferHandle, BufferProperties},
    command::{CommandError, CommandIO, CommandManager},
    cursor::Cursor,
    editor::{Editor, EditorContext},
    editor_utils::parse_process_command,
    platform::Platform,
    plugin::PluginHandle,
};

use crate::{
    client::{Client, ClientOperation},
    {ClientGuard, LspPlugin},
};

pub fn register_commands(commands: &mut CommandManager, plugin_handle: PluginHandle) {
    let mut r = |name, completions, command_fn| {
        commands.register(Some(plugin_handle), name, completions, command_fn);
    };

    r("lsp-debug", &[], |ctx, io| {
        io.args.assert_empty()?;

        let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
        let mut write = ctx
            .editor
            .status_bar
            .write(pepper::editor_utils::MessageKind::Info);
        write.fmt(format_args!("client count: {}\n", lsp.clients().count()));
        for (i, client) in lsp.clients().enumerate() {
            let d = std::mem::discriminant(&client.request_state);
            write.fmt(format_args!("client {}: request_state: {:?}", i, d));
        }

        Ok(())
    });

    r("lsp", &[], |ctx, io| {
        let command = io.args.next()?;
        let glob = io.args.next()?;
        let log_path = io.args.try_next();
        io.args.assert_empty()?;

        let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
        let result = match lsp.add_recipe(glob, command, None, log_path) {
            Ok(()) => Ok(()),
            Err(error) => Err(CommandError::InvalidGlob(error)),
        };
        result
    });

    r("lsp-open-log", &[], |ctx, io| {
        io.args.assert_empty()?;
        let client_handle = io.client_handle()?;
        let buffer_handle = io.current_buffer_handle(ctx)?;
        access(ctx, io, buffer_handle, |editor, _, clients, client| {
            let path = client
                .log_file_path()
                .ok_or(CommandError::OtherStatic("lsp server is not logging"))?;

            let buffer_view_handle = editor
                .buffer_view_handle_from_path(
                    client_handle,
                    Path::new(path),
                    BufferProperties::scratch(),
                    true,
                )
                .map_err(CommandError::BufferReadError)?;

            let client = clients.get_mut(client_handle);
            client.set_buffer_view_handle(Some(buffer_view_handle), &editor.buffer_views);

            Ok(ClientOperation::None)
        })
    });

    r("lsp-start", &[], |ctx, io| {
        let command = io.args.next()?;
        let log_path = io.args.try_next();
        io.args.assert_empty()?;

        let command = parse_process_command(command).ok_or(CommandError::OtherOwned(format!(
            "invalid lsp command '{}'",
            command
        )))?;
        let root = ctx.editor.current_directory.clone();

        let plugin_handle = io.plugin_handle();
        let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
        lsp.start(
            &mut ctx.platform,
            plugin_handle,
            command,
            root,
            log_path.map(String::from),
        );
        Ok(())
    });

    r("lsp-stop", &[], |ctx, io| {
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
        let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
        match find_lsp_client_for_buffer(lsp, &ctx.editor, buffer_handle) {
            Some(client) => {
                let handle = client.handle();
                lsp.release(client);
                lsp.stop(&mut ctx.platform, handle);
            }
            None => lsp.stop_all(&mut ctx.platform),
        }
        Ok(())
    });

    r("lsp-stop-all", &[], |ctx, io| {
        io.args.assert_empty()?;

        let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
        lsp.stop_all(&mut ctx.platform);
        Ok(())
    });

    r("lsp-hover", &[], |ctx, io| {
        io.args.assert_empty()?;

        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;
        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.hover(editor, platform, buffer_handle, cursor.position);
            Ok(op)
        })
    });

    r("lsp-definition", &[], |ctx, io| {
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;

        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.definition(
                editor,
                platform,
                buffer_handle,
                cursor.position,
                client_handle,
            );
            Ok(op)
        })
    });

    r("lsp-declaration", &[], |ctx, io| {
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;

        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.declaration(
                editor,
                platform,
                buffer_handle,
                cursor.position,
                client_handle,
            );
            Ok(op)
        })
    });

    r("lsp-implementation", &[], |ctx, io| {
        io.args.assert_empty()?;
        let client_handle = io.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;
        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.implementation(
                editor,
                platform,
                buffer_handle,
                cursor.position,
                client_handle,
            );
            Ok(op)
        })
    });

    r("lsp-references", &[], |ctx, io| {
        let context_len = 2;
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;

        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.references(
                editor,
                platform,
                buffer_handle,
                cursor.position,
                context_len,
                client_handle,
            );
            Ok(op)
        })
    });

    r("lsp-rename", &[], |ctx, io| {
        io.args.assert_empty()?;

        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;
        let plugin_handle = io.plugin_handle();

        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.rename(
                editor,
                platform,
                plugin_handle,
                buffer_handle,
                cursor.position,
            );
            Ok(op)
        })
    });

    r("lsp-code-action", &[], |ctx, io| {
        io.args.assert_empty()?;

        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;
        let plugin_handle = io.plugin_handle();

        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.code_action(
                editor,
                platform,
                plugin_handle,
                buffer_handle,
                cursor.to_range(),
            );
            Ok(op)
        })
    });

    r("lsp-document-symbol", &[], |ctx, io| {
        io.args.assert_empty()?;

        let view_handle = io.current_buffer_view_handle(ctx)?;
        let buffer_handle = ctx.editor.buffer_views.get(view_handle).buffer_handle;

        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.document_symbols(editor, platform, view_handle);
            Ok(op)
        })
    });

    r("lsp-workspace-symbols", &[], |ctx, io| {
        let query = io.args.try_next().unwrap_or("");
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.workspace_symbols(editor, platform, query);
            Ok(op)
        })
    });

    r("lsp-format", &[], |ctx, io| {
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
        access(ctx, io, buffer_handle, |editor, platform, _, client| {
            let op = client.formatting(editor, platform, buffer_handle);
            Ok(op)
        })
    });
}

fn current_buffer_and_main_cursor(
    ctx: &EditorContext,
    io: &CommandIO,
) -> Result<(BufferHandle, Cursor), CommandError> {
    let view_handle = io.current_buffer_view_handle(ctx)?;
    let buffer_view = ctx.editor.buffer_views.get(view_handle);

    let buffer_handle = buffer_view.buffer_handle;
    let cursor = *buffer_view.cursors.main_cursor();
    Ok((buffer_handle, cursor))
}

fn find_lsp_client_for_buffer(
    lsp: &mut LspPlugin,
    editor: &Editor,
    buffer_handle: BufferHandle,
) -> Option<ClientGuard> {
    let buffer_path = editor.buffers.get(buffer_handle).path.to_str()?;
    lsp.find_client(|c| c.handles_path(buffer_path))
}

fn access<A>(
    ctx: &mut EditorContext,
    io: &mut CommandIO,
    buffer_handle: BufferHandle,
    accessor: A,
) -> Result<(), CommandError>
where
    A: FnOnce(
        &mut Editor,
        &mut Platform,
        &mut pepper::client::ClientManager,
        &mut Client,
    ) -> Result<ClientOperation, CommandError>,
{
    let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
    if let Some(mut client) = find_lsp_client_for_buffer(lsp, &ctx.editor, buffer_handle) {
        let op = accessor(
            &mut ctx.editor,
            &mut ctx.platform,
            &mut ctx.clients,
            &mut client,
        );
        let client_handle = client.handle();
        lsp.release(client);
        lsp.on_client_operation(client_handle, op?);
    }

    Ok(())
}
