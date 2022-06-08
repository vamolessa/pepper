use pepper::{
    buffer::BufferHandle,
    command::{CommandError, CommandIO, CommandManager},
    cursor::Cursor,
    editor::{Editor, EditorContext},
    editor_utils::parse_process_command,
    plugin::PluginHandle,
};

use crate::{
    client::Client,
    {ClientGuard, LspPlugin},
};

pub fn register_commands(commands: &mut CommandManager, plugin_handle: PluginHandle) {
    let mut r = |name, completions, command_fn| {
        commands.register_command(Some(plugin_handle), name, completions, command_fn);
    };

    r("lsp", &[], |ctx, io| {
        let command = io.args.next()?;
        let glob = io.args.next()?;
        io.args.assert_empty()?;

        let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
        let result = match lsp.add_recipe(glob, command, None) {
            Ok(()) => Ok(()),
            Err(error) => Err(CommandError::InvalidGlob(error)),
        };
        result
    });

    r("lsp-start", &[], |ctx, io| {
        let command = io.args.next()?;
        io.args.assert_empty()?;

        let command = parse_process_command(command).ok_or(CommandError::OtherOwned(format!(
            "invalid lsp command '{}'",
            command
        )))?;
        let root = ctx.editor.current_directory.clone();

        let plugin_handle = io.plugin_handle();
        let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
        lsp.start(&mut ctx.platform, plugin_handle, command, root);
        Ok(())
    });

    r("lsp-stop", &[], |ctx, io| {
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx).ok();
        let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
        let any_stopped = match find_lsp_client_for_buffer(lsp, &ctx.editor, buffer_handle) {
            Some(client) => {
                let handle = client.handle();
                lsp.release(client);
                lsp.stop(&mut ctx.platform, handle)
            }
            None => lsp.stop_all(&mut ctx.platform),
        };
        if any_stopped {
            Ok(())
        } else {
            Err(CommandError::OtherStatic("no lsp server running"))
        }
    });

    r("lsp-stop-all", &[], |ctx, io| {
        io.args.assert_empty()?;

        let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
        if lsp.stop_all(&mut ctx.platform) {
            Ok(())
        } else {
            Err(CommandError::OtherStatic("no lsp server running"))
        }
    });

    r("lsp-hover", &[], |ctx, io| {
        io.args.assert_empty()?;

        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;
        access(ctx, io, Some(buffer_handle), |ctx, client| {
            let op = client.hover(
                &mut ctx.editor,
                &mut ctx.platform,
                buffer_handle,
                cursor.position,
            );
            Ok(op)
        })
    });

    r("lsp-definition", &[], |ctx, io| {
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;

        access(ctx, io, Some(buffer_handle), |ctx, client| {
            let op = client.definition(
                &mut ctx.editor,
                &mut ctx.platform,
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

        access(ctx, io, Some(buffer_handle), |ctx, client| {
            let op = client.declaration(
                &mut ctx.editor,
                &mut ctx.platform,
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
        access(ctx, io, Some(buffer_handle), |ctx, client| {
            let op = client.implementation(
                &mut ctx.editor,
                &mut ctx.platform,
                buffer_handle,
                cursor.position,
                client_handle,
            );
            Ok(op)
        })
    });

    r("lsp-references", &[], |ctx, io| {
        let context_len = match io.args.try_next() {
            Some(len) => match len.parse() {
                Ok(len) => len,
                Err(_) => {
                    return Err(CommandError::OtherOwned(format!(
                        "could not parse context-len from '{}'",
                        len
                    )))
                }
            },
            None => 2,
        };
        io.args.assert_empty()?;

        let client_handle = io.client_handle()?;
        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;

        access(ctx, io, Some(buffer_handle), |ctx, client| {
            let op = client.references(
                &mut ctx.editor,
                &mut ctx.platform,
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

        access(ctx, io, Some(buffer_handle), |ctx, client| {
            let op = client.rename(ctx, plugin_handle, buffer_handle, cursor.position);
            Ok(op)
        })
    });

    r("lsp-code-action", &[], |ctx, io| {
        io.args.assert_empty()?;

        let (buffer_handle, cursor) = current_buffer_and_main_cursor(ctx, io)?;
        let plugin_handle = io.plugin_handle();

        access(ctx, io, Some(buffer_handle), |ctx, client| {
            let op = client.code_action(
                &mut ctx.editor,
                &mut ctx.platform,
                plugin_handle,
                buffer_handle,
                cursor.to_range(),
            );
            Ok(op)
        })
    });

    r("lsp-document-symbols", &[], |ctx, io| {
        io.args.assert_empty()?;

        let view_handle = io.current_buffer_view_handle(ctx)?;
        let buffer_handle = ctx.editor.buffer_views.get(view_handle).buffer_handle;

        access(ctx, io, Some(buffer_handle), |ctx, client| {
            let op = client.document_symbols(&mut ctx.editor, &mut ctx.platform, view_handle);
            Ok(op)
        })
    });

    r("lsp-workspace-symbols", &[], |ctx, io| {
        let query = io.args.try_next().unwrap_or("");
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx).ok();
        access(ctx, io, buffer_handle, |ctx, client| {
            let op = client.workspace_symbols(&mut ctx.editor, &mut ctx.platform, query);
            Ok(op)
        })
    });

    r("lsp-format", &[], |ctx, io| {
        io.args.assert_empty()?;

        let buffer_handle = io.current_buffer_handle(ctx)?;
        access(ctx, io, Some(buffer_handle), |ctx, client| {
            let op = client.formatting(&mut ctx.editor, &mut ctx.platform, buffer_handle);
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
    buffer_handle: Option<BufferHandle>,
) -> Option<ClientGuard> {
    match buffer_handle {
        Some(buffer_handle) => {
            let buffer_path = editor.buffers.get(buffer_handle).path.to_str()?;
            lsp.find_client(|c| c.handles_path(buffer_path))
        }
        None => lsp.find_client(|_| true),
    }
}

fn access<A>(
    ctx: &mut EditorContext,
    io: &mut CommandIO,
    buffer_handle: Option<BufferHandle>,
    accessor: A,
) -> Result<(), CommandError>
where
    A: FnOnce(&mut EditorContext, &mut Client) -> Result<(), CommandError>,
{
    let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
    if let Some(mut client) = find_lsp_client_for_buffer(lsp, &ctx.editor, buffer_handle) {
        let result = accessor(ctx, &mut client);
        let lsp = ctx.plugins.get_as::<LspPlugin>(io.plugin_handle());
        lsp.release(client);
        let _ = result?;
    }

    Ok(())
}

