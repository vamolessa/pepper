use std::path::Path;

use pepper::{
    buffer::BufferProperties,
    client::ClientHandle,
    cursor::Cursor,
    editor::{EditorContext, EditorFlow, KeysIterator},
    editor_utils::{parse_path_and_ranges, LogKind, ReadLinePoll, REGISTER_READLINE_PROMPT},
    mode::ModeKind,
    picker::EntrySource,
    plugin::PluginHandle,
    word_database::WordIndicesIter,
};

use crate::{client::Client, LspPlugin};

pub fn enter_definition_mode(
    ctx: &mut EditorContext,
    plugin_handle: PluginHandle,
    client: &Client,
) {
    fn on_client_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        _: &mut KeysIterator,
        poll: ReadLinePoll,
    ) -> Option<EditorFlow> {
        match poll {
            ReadLinePoll::Pending => Some(EditorFlow::Continue),
            ReadLinePoll::Submitted => {
                if let Some((_, entry)) = ctx.editor.picker.current_entry(&ctx.editor.word_database)
                {
                    let entry = ctx.editor.string_pool.acquire_with(entry);
                    let (path, ranges) = parse_path_and_ranges(&entry);
                    match ctx.editor.buffer_view_handle_from_path(
                        client_handle,
                        Path::new(path),
                        BufferProperties::text(),
                        false,
                    ) {
                        Ok(buffer_view_handle) => {
                            let client = ctx.clients.get_mut(client_handle);
                            client.set_buffer_view_handle(
                                Some(buffer_view_handle),
                                &ctx.editor.buffer_views,
                            );

                            let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
                            let buffer_content =
                                ctx.editor.buffers.get(buffer_view.buffer_handle).content();
                            let mut cursors = buffer_view.cursors.mut_guard();
                            cursors.clear();
                            for range in ranges {
                                cursors.add(Cursor {
                                    anchor: buffer_content.saturate_position(range.0),
                                    position: buffer_content.saturate_position(range.1),
                                });
                            }
                        }
                        Err(error) => ctx
                            .editor
                            .logger
                            .write(LogKind::Error)
                            .fmt(format_args!("{}", error)),
                    }
                    ctx.editor.string_pool.release(entry);
                }
                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
            ReadLinePoll::Canceled => {
                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
        }
    }

    ctx.editor
        .registers
        .set(REGISTER_READLINE_PROMPT, "definition:");
    ctx.editor.picker.filter(WordIndicesIter::empty(), "");
    ctx.editor.picker.move_cursor(0);

    if ctx.editor.picker.len() > 0 {
        ctx.editor.mode.plugin_handle = Some(plugin_handle);
        ctx.editor.mode.picker_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::Picker);

        let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
        lsp.current_client_handle = Some(client.handle());
    }
}

pub fn enter_code_action_mode(
    ctx: &mut EditorContext,
    plugin_handle: PluginHandle,
    client: &mut Client,
) {
    fn on_client_keys(
        ctx: &mut EditorContext,
        _: ClientHandle,
        _: &mut KeysIterator,
        poll: ReadLinePoll,
    ) -> Option<EditorFlow> {
        match poll {
            ReadLinePoll::Pending => Some(EditorFlow::Continue),
            ReadLinePoll::Submitted => {
                if let Some(handle) = ctx.editor.mode.plugin_handle {
                    let lsp = ctx.plugins.get_as::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .current_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        let index = match ctx.editor.picker.current_entry(&ctx.editor.word_database)
                        {
                            Some((EntrySource::Custom(i), _)) => i,
                            _ => 0,
                        };
                        client.finish_code_action(&mut ctx.editor, index);
                    }
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
            ReadLinePoll::Canceled => {
                if let Some(handle) = ctx.editor.mode.plugin_handle {
                    let lsp = ctx.plugins.get_as::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .current_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        client.cancel_current_request();
                    }
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
        }
    }

    ctx.editor
        .registers
        .set(REGISTER_READLINE_PROMPT, "code action:");
    ctx.editor.picker.filter(WordIndicesIter::empty(), "");
    ctx.editor.picker.move_cursor(0);

    if ctx.editor.picker.len() > 0 {
        ctx.editor.mode.plugin_handle = Some(plugin_handle);
        ctx.editor.mode.picker_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::Picker);

        let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
        lsp.current_client_handle = Some(client.handle());
    } else {
        client.cancel_current_request();
    }
}

pub fn enter_document_symbol_mode(
    ctx: &mut EditorContext,
    plugin_handle: PluginHandle,
    client: &mut Client,
) {
    fn on_client_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        _: &mut KeysIterator,
        poll: ReadLinePoll,
    ) -> Option<EditorFlow> {
        match poll {
            ReadLinePoll::Pending => Some(EditorFlow::Continue),
            ReadLinePoll::Submitted => {
                if let Some(handle) = ctx.editor.mode.plugin_handle {
                    let lsp = ctx.plugins.get_as::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .current_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        let index = match ctx.editor.picker.current_entry(&ctx.editor.word_database)
                        {
                            Some((EntrySource::Custom(i), _)) => i,
                            _ => 0,
                        };
                        client.finish_document_symbols(
                            &mut ctx.editor,
                            &mut ctx.clients,
                            client_handle,
                            index,
                        );
                    }
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
            ReadLinePoll::Canceled => {
                if let Some(handle) = ctx.editor.mode.plugin_handle {
                    let lsp = ctx.plugins.get_as::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .current_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        client.cancel_current_request();
                    }
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
        }
    }

    ctx.editor
        .registers
        .set(REGISTER_READLINE_PROMPT, "document symbol:");
    ctx.editor.picker.filter(WordIndicesIter::empty(), "");
    ctx.editor.picker.move_cursor(0);

    if ctx.editor.picker.len() > 0 {
        ctx.editor.mode.plugin_handle = Some(plugin_handle);
        ctx.editor.mode.picker_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::Picker);

        let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
        lsp.current_client_handle = Some(client.handle());
    } else {
        client.cancel_current_request();
    }
}

pub fn enter_workspace_symbol_mode(
    ctx: &mut EditorContext,
    plugin_handle: PluginHandle,
    client: &mut Client,
) {
    fn on_client_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        _: &mut KeysIterator,
        poll: ReadLinePoll,
    ) -> Option<EditorFlow> {
        match poll {
            ReadLinePoll::Pending => Some(EditorFlow::Continue),
            ReadLinePoll::Submitted => {
                if let Some(handle) = ctx.editor.mode.plugin_handle {
                    let lsp = ctx.plugins.get_as::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .current_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        let index = match ctx.editor.picker.current_entry(&ctx.editor.word_database)
                        {
                            Some((EntrySource::Custom(i), _)) => i,
                            _ => 0,
                        };
                        client.finish_workspace_symbols(
                            &mut ctx.editor,
                            &mut ctx.clients,
                            client_handle,
                            index,
                        );
                    }
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
            ReadLinePoll::Canceled => {
                if let Some(handle) = ctx.editor.mode.plugin_handle {
                    let lsp = ctx.plugins.get_as::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .current_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        client.cancel_current_request();
                    }
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
        }
    }

    ctx.editor
        .registers
        .set(REGISTER_READLINE_PROMPT, "workspace symbol:");
    ctx.editor.picker.filter(WordIndicesIter::empty(), "");
    ctx.editor.picker.move_cursor(0);

    if ctx.editor.picker.len() > 0 {
        ctx.editor.mode.plugin_handle = Some(plugin_handle);
        ctx.editor.mode.picker_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::Picker);

        let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
        lsp.current_client_handle = Some(client.handle());
    } else {
        client.cancel_current_request();
    }
}
