use std::path::Path;

use pepper::{
    buffer::{parse_path_and_position, BufferProperties},
    buffer_position::BufferPosition,
    cursor::Cursor,
    editor::{EditorControlFlow, KeysIterator},
    editor_utils::{MessageKind, ReadLinePoll},
    mode::{Mode, ModeContext, ModeKind},
    word_database::WordIndicesIter,
};

use crate::{client::ClientOperation, LspPlugin};

pub fn enter_definition_mode(ctx: &mut ModeContext) -> ClientOperation {
    fn on_client_keys(
        ctx: &mut ModeContext,
        _: &mut KeysIterator,
        poll: ReadLinePoll,
    ) -> Option<EditorControlFlow> {
        match poll {
            ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
            ReadLinePoll::Submitted => {
                if let Some((_, entry)) = ctx.editor.picker.current_entry(&ctx.editor.word_database)
                {
                    let (path, position) = parse_path_and_position(entry);
                    let position = match position {
                        Some(position) => position,
                        None => BufferPosition::zero(),
                    };

                    let path = ctx.editor.string_pool.acquire_with(path);
                    match ctx.editor.buffer_view_handle_from_path(
                        ctx.client_handle,
                        Path::new(&path),
                        BufferProperties::text(),
                        false,
                    ) {
                        Ok(buffer_view_handle) => {
                            let client = ctx.clients.get_mut(ctx.client_handle);
                            client.set_buffer_view_handle(
                                Some(buffer_view_handle),
                                &ctx.editor.buffer_views,
                                &mut ctx.editor.events,
                            );

                            let mut cursors = ctx
                                .editor
                                .buffer_views
                                .get_mut(buffer_view_handle)
                                .cursors
                                .mut_guard();
                            cursors.clear();
                            cursors.add(Cursor {
                                anchor: position,
                                position,
                            });
                        }
                        Err(error) => ctx
                            .editor
                            .status_bar
                            .write(MessageKind::Error)
                            .fmt(format_args!("{}", error)),
                    }
                    ctx.editor.string_pool.release(path);
                }
                Mode::change_to(ctx, ModeKind::default());
                Some(EditorControlFlow::Continue)
            }
            ReadLinePoll::Canceled => {
                Mode::change_to(ctx, ModeKind::default());
                Some(EditorControlFlow::Continue)
            }
        }
    }

    ctx.editor.read_line.set_prompt("definition:");
    ctx.editor.picker.filter(WordIndicesIter::empty(), "");
    ctx.editor.picker.move_cursor(0);

    if ctx.editor.picker.len() > 0 {
        let state = &mut ctx.editor.mode.picker_state;
        state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::Picker);
    }

    ClientOperation::EnteredPickerMode
}

pub fn enter_code_action_mode(ctx: &mut ModeContext) -> ClientOperation {
    /*
            fn on_client_keys(
                ctx: &mut ModeContext,
                _: &mut KeysIterator,
                poll: ReadLinePoll,
            ) -> Option<EditorControlFlow> {
                match poll {
                    ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                    ReadLinePoll::Submitted => {
                        if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                            let index = match ctx.editor.picker.current_entry(&ctx.editor.word_database)
                            {
                                Some((EntrySource::Custom(i), _)) => i,
                                _ => 0,
                            };
                            lsp::ClientManager::access(ctx.editor, handle, |e, c| {
                                c.finish_code_action(e, index);
                            });
                        }
                        Mode::change_to(ctx, ModeKind::default());
                        Some(EditorControlFlow::Continue)
                    }
                    ReadLinePoll::Canceled => {
                        if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                            lsp::ClientManager::access(ctx.editor, handle, |_, c| {
                                c.cancel_current_request();
                            });
                        }
                        Mode::change_to(ctx, ModeKind::default());
                        Some(EditorControlFlow::Continue)
                    }
                }
            }

            ctx.editor.read_line.set_prompt("code action:");
            ctx.editor.picker.filter(WordIndicesIter::empty(), "");
            ctx.editor.picker.move_cursor(0);

            if ctx.editor.picker.len() > 0 {
                let state = &mut ctx.editor.mode.picker_state;
                state.on_client_keys = on_client_keys;
                state.lsp_client_handle = Some(client_handle);
                Mode::change_to(ctx, ModeKind::Picker);
            } else {
                lsp::ClientManager::access(ctx.editor, client_handle, |_, c| {
                    c.cancel_current_request();
                });
            }
    */

    ClientOperation::EnteredPickerMode
}

pub fn enter_document_symbol_mode(ctx: &mut ModeContext) -> ClientOperation {
    /*
            fn on_client_keys(
                ctx: &mut ModeContext,
                _: &mut KeysIterator,
                poll: ReadLinePoll,
            ) -> Option<EditorControlFlow> {
                match poll {
                    ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                    ReadLinePoll::Submitted => {
                        if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                            let index = match ctx.editor.picker.current_entry(&ctx.editor.word_database)
                            {
                                Some((EntrySource::Custom(i), _)) => i,
                                _ => 0,
                            };
                            let clients = &mut *ctx.clients;
                            let client_handle = ctx.client_handle;
                            lsp::ClientManager::access(ctx.editor, handle, |e, c| {
                                c.finish_document_symbols(e, clients, client_handle, index);
                            });
                        }
                        Mode::change_to(ctx, ModeKind::default());
                        Some(EditorControlFlow::Continue)
                    }
                    ReadLinePoll::Canceled => {
                        if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                            lsp::ClientManager::access(ctx.editor, handle, |_, c| {
                                c.cancel_current_request();
                            });
                        }
                        Mode::change_to(ctx, ModeKind::default());
                        Some(EditorControlFlow::Continue)
                    }
                }
            }

            ctx.editor.read_line.set_prompt("document symbol:");
            ctx.editor.picker.filter(WordIndicesIter::empty(), "");
            ctx.editor.picker.move_cursor(0);

            if ctx.editor.picker.len() > 0 {
                let state = &mut ctx.editor.mode.picker_state;
                state.on_client_keys = on_client_keys;
                state.lsp_client_handle = Some(client_handle);
                Mode::change_to(ctx, ModeKind::Picker);
            } else {
                lsp::ClientManager::access(ctx.editor, client_handle, |_, c| {
                    c.cancel_current_request();
                });
            }
    */

    ClientOperation::EnteredPickerMode
}

pub fn enter_workspace_symbol_mode(ctx: &mut ModeContext) -> ClientOperation {
    /*
            fn on_client_keys(
                ctx: &mut ModeContext,
                _: &mut KeysIterator,
                poll: ReadLinePoll,
            ) -> Option<EditorControlFlow> {
                match poll {
                    ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                    ReadLinePoll::Submitted => {
                        if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                            let index = match ctx.editor.picker.current_entry(&ctx.editor.word_database)
                            {
                                Some((EntrySource::Custom(i), _)) => i,
                                _ => 0,
                            };
                            let clients = &mut *ctx.clients;
                            let client_handle = ctx.client_handle;
                            lsp::ClientManager::access(ctx.editor, handle, |e, c| {
                                c.finish_workspace_symbols(e, clients, client_handle, index);
                            });
                        }
                        Mode::change_to(ctx, ModeKind::default());
                        Some(EditorControlFlow::Continue)
                    }
                    ReadLinePoll::Canceled => {
                        if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                            lsp::ClientManager::access(ctx.editor, handle, |_, c| {
                                c.cancel_current_request();
                            });
                        }
                        Mode::change_to(ctx, ModeKind::default());
                        Some(EditorControlFlow::Continue)
                    }
                }
            }

            ctx.editor.read_line.set_prompt("workspace symbol:");
            ctx.editor.picker.filter(WordIndicesIter::empty(), "");
            ctx.editor.picker.move_cursor(0);

            if ctx.editor.picker.len() > 0 {
                let state = &mut ctx.editor.mode.picker_state;
                state.on_client_keys = on_client_keys;
                state.lsp_client_handle = Some(client_handle);
                Mode::change_to(ctx, ModeKind::Picker);
            } else {
                lsp::ClientManager::access(ctx.editor, client_handle, |_, c| {
                    c.cancel_current_request();
                });
            }
    */

    ClientOperation::EnteredPickerMode
}

