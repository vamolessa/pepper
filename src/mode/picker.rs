use crate::{
    command::CommandManager,
    editor::KeysIterator,
    editor_utils::{MessageKind, ReadLinePoll},
    lsp,
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    picker::EntrySource,
    platform::Key,
    register::RETURN_REGISTER,
    word_database::WordIndicesIter,
};

pub struct State {
    on_client_keys:
        fn(ctx: &mut ModeContext, &mut KeysIterator, ReadLinePoll) -> Option<ModeOperation>,
    continuation: Option<String>,
    lsp_client_handle: Option<lsp::ClientHandle>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _| None,
            continuation: None,
            lsp_client_handle: None,
        }
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.editor.read_line.input_mut().clear();
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.editor.read_line.input_mut().clear();
        ctx.editor.picker.clear();
    }

    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<ModeOperation> {
        let this = &mut ctx.editor.mode.picker_state;
        let poll = ctx.editor.read_line.poll(
            ctx.platform,
            &mut ctx.editor.string_pool,
            &ctx.editor.buffered_keys,
            keys,
            &ctx.editor.registers,
        );
        if let ReadLinePoll::Pending = poll {
            keys.put_back();
            match keys.next(&ctx.editor.buffered_keys) {
                Key::Ctrl('n') | Key::Ctrl('j') | Key::Down => ctx.editor.picker.move_cursor(1),
                Key::Ctrl('p') | Key::Ctrl('k') | Key::Up => ctx.editor.picker.move_cursor(-1),
                Key::Ctrl('d') | Key::PageDown => {
                    let picker_height = ctx
                        .editor
                        .picker
                        .len()
                        .min(ctx.editor.config.picker_max_height as _)
                        as isize;
                    ctx.editor.picker.move_cursor(picker_height / 2);
                }
                Key::Ctrl('u') | Key::PageUp => {
                    let picker_height = ctx
                        .editor
                        .picker
                        .len()
                        .min(ctx.editor.config.picker_max_height as _)
                        as isize;
                    ctx.editor.picker.move_cursor(-picker_height / 2);
                }
                Key::Ctrl('b') | Key::Home => {
                    let cursor = ctx.editor.picker.cursor().unwrap_or(0) as isize;
                    ctx.editor.picker.move_cursor(-cursor);
                }
                Key::Ctrl('e') | Key::End => {
                    let cursor = ctx.editor.picker.cursor().unwrap_or(0) as isize;
                    let entry_count = ctx.editor.picker.len() as isize;
                    ctx.editor.picker.move_cursor(entry_count - cursor - 1);
                }
                _ => {
                    ctx.editor
                        .picker
                        .filter(WordIndicesIter::empty(), ctx.editor.read_line.input());
                    ctx.editor.picker.move_cursor(0);
                }
            }
        }

        (this.on_client_keys)(ctx, keys, poll)
    }
}

pub mod buffer {
    use super::*;

    use std::path::Path;

    use crate::navigation_history::NavigationHistory;

    pub fn enter_mode(ctx: &mut ModeContext) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => return None,
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => {
                    Mode::change_to(ctx, ModeKind::default());
                    return None;
                }
            }

            let path = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
                Some((_, entry)) => entry,
                _ => {
                    Mode::change_to(ctx, ModeKind::default());
                    return None;
                }
            };

            NavigationHistory::save_client_snapshot(
                ctx.clients,
                ctx.client_handle,
                &ctx.editor.buffer_views,
            );

            let path = ctx.editor.string_pool.acquire_with(path);
            let buffer_view_handle = ctx
                .editor
                .buffer_view_handle_from_path(ctx.client_handle, Path::new(&path));
            ctx.editor.string_pool.release(path);

            if let Some(client) = ctx.clients.get_mut(ctx.client_handle) {
                client.set_buffer_view_handle(Some(buffer_view_handle), &mut ctx.editor.events);
            }

            Mode::change_to(ctx, ModeKind::default());
            None
        }

        ctx.editor.read_line.set_prompt("buffer:");
        ctx.editor.picker.clear();

        for path in ctx.editor.buffers.iter().filter_map(|b| b.path.to_str()) {
            ctx.editor.picker.add_custom_entry(path);
        }

        ctx.editor.picker.filter(WordIndicesIter::empty(), "");
        ctx.editor.picker.move_cursor(0);

        if ctx.editor.picker.len() > 0 {
            ctx.editor.mode.picker_state.on_client_keys = on_client_keys;
            Mode::change_to(ctx, ModeKind::Picker);
        } else {
            ctx.editor
                .status_bar
                .write(MessageKind::Error)
                .str("no buffer opened");
        }
    }
}

pub mod lsp_code_action {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext, client_handle: lsp::ClientHandle) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => None,
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
                    None
                }
                ReadLinePoll::Canceled => {
                    if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                        lsp::ClientManager::access(ctx.editor, handle, |_, c| {
                            c.cancel_current_request();
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    None
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
    }
}

pub mod lsp_document_symbol {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext, client_handle: lsp::ClientHandle) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => None,
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
                    None
                }
                ReadLinePoll::Canceled => {
                    if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                        lsp::ClientManager::access(ctx.editor, handle, |_, c| {
                            c.cancel_current_request();
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    None
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
    }
}

pub mod custom {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext, continuation: String) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => None,
                ReadLinePoll::Submitted => {
                    let continuation = ctx.editor.mode.picker_state.continuation.take().unwrap();
                    let entry = ctx.editor.picker.current_entry(&ctx.editor.word_database);

                    let mut operation = None;
                    if let Some((_, entry)) = entry {
                        ctx.editor.registers.set(RETURN_REGISTER, entry);
                        operation = CommandManager::eval_and_then_output(
                            ctx.editor,
                            ctx.platform,
                            ctx.clients,
                            Some(ctx.client_handle),
                            &continuation,
                            None,
                        )
                        .map(Into::into);
                        ctx.editor.string_pool.release(continuation);
                    }

                    if ctx.editor.mode.kind() == ModeKind::Picker
                        && ctx.editor.mode.picker_state.continuation.is_none()
                    {
                        Mode::change_to(ctx, ModeKind::default());
                    }

                    operation
                }
                ReadLinePoll::Canceled => {
                    Mode::change_to(ctx, ModeKind::default());
                    None
                }
            }
        }

        ctx.editor.picker.filter(WordIndicesIter::empty(), "");
        ctx.editor.picker.move_cursor(0);

        let state = &mut ctx.editor.mode.picker_state;
        state.on_client_keys = on_client_keys;
        state.continuation = Some(continuation);

        Mode::change_to(ctx, ModeKind::Picker);
    }
}
