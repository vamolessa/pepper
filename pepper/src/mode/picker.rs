use crate::{
    buffer::BufferProperties,
    client::ClientHandle,
    command::CommandManager,
    editor::{Editor, EditorContext, EditorFlow, KeysIterator},
    editor_utils::{MessageKind, ReadLinePoll},
    mode::{ModeKind, ModeState},
    platform::{Key, KeyCode},
    word_database::WordIndicesIter,
};

pub struct State {
    pub on_client_keys: fn(
        ctx: &mut EditorContext,
        ClientHandle,
        &mut KeysIterator,
        ReadLinePoll,
    ) -> Option<EditorFlow>,
    continuation: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _, _| Some(EditorFlow::Continue),
            continuation: String::new(),
        }
    }
}

impl ModeState for State {
    fn on_enter(editor: &mut Editor) {
        editor.read_line.input_mut().clear();
    }

    fn on_exit(editor: &mut Editor) {
        editor.mode.plugin_handle = None;
        editor.read_line.input_mut().clear();
        editor.picker.clear();
    }

    fn on_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<EditorFlow> {
        let this = &mut ctx.editor.mode.picker_state;
        let poll = ctx.editor.read_line.poll(
            &mut ctx.platform,
            &mut ctx.editor.string_pool,
            &ctx.editor.buffered_keys,
            keys,
        );
        if let ReadLinePoll::Pending = poll {
            keys.index = keys.index.saturating_sub(1);
            match keys.next(&ctx.editor.buffered_keys) {
                Key {
                    code: KeyCode::Char('n'),
                    shift: false,
                    control: true,
                    alt: false,
                }
                | Key {
                    code: KeyCode::Down,
                    shift: false,
                    control: false,
                    alt: false,
                } => {
                    ctx.editor.picker.move_cursor(1);
                }
                Key {
                    code: KeyCode::Char('p'),
                    shift: false,
                    control: true,
                    alt: false,
                }
                | Key {
                    code: KeyCode::Up,
                    shift: false,
                    control: false,
                    alt: false,
                } => {
                    ctx.editor.picker.move_cursor(-1);
                }
                Key {
                    code: KeyCode::Char('j'),
                    shift: false,
                    control: true,
                    alt: false,
                }
                | Key {
                    code: KeyCode::PageDown,
                    shift: false,
                    control: false,
                    alt: false,
                } => {
                    let picker_height = ctx
                        .editor
                        .picker
                        .len()
                        .min(ctx.editor.config.picker_max_height as _)
                        as isize;
                    ctx.editor.picker.move_cursor(picker_height / 2);
                }
                Key {
                    code: KeyCode::Char('k'),
                    shift: false,
                    control: true,
                    alt: false,
                }
                | Key {
                    code: KeyCode::PageUp,
                    shift: false,
                    control: false,
                    alt: false,
                } => {
                    let picker_height = ctx
                        .editor
                        .picker
                        .len()
                        .min(ctx.editor.config.picker_max_height as _)
                        as isize;
                    ctx.editor.picker.move_cursor(-picker_height / 2);
                }
                Key {
                    code: KeyCode::Char('b'),
                    shift: false,
                    control: true,
                    alt: false,
                }
                | Key {
                    code: KeyCode::Home,
                    shift: false,
                    control: false,
                    alt: false,
                } => {
                    let cursor = ctx.editor.picker.cursor().unwrap_or(0) as isize;
                    ctx.editor.picker.move_cursor(-cursor);
                }
                Key {
                    code: KeyCode::Char('e'),
                    shift: false,
                    control: true,
                    alt: false,
                }
                | Key {
                    code: KeyCode::End,
                    shift: false,
                    control: false,
                    alt: false,
                } => {
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

        let f = this.on_client_keys;
        f(ctx, client_handle, keys, poll)
    }
}

pub mod opened_buffers {
    use super::*;

    use std::path::Path;

    pub fn enter_mode(ctx: &mut EditorContext) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorFlow> {
            match poll {
                ReadLinePoll::Pending => return Some(EditorFlow::Continue),
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => {
                    ctx.editor.enter_mode(ModeKind::default());
                    return Some(EditorFlow::Continue);
                }
            }

            let path = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
                Some((_, entry)) => entry,
                _ => {
                    ctx.editor.enter_mode(ModeKind::default());
                    return Some(EditorFlow::Continue);
                }
            };

            let path = ctx.editor.string_pool.acquire_with(path);
            if let Ok(buffer_view_handle) = ctx.editor.buffer_view_handle_from_path(
                client_handle,
                Path::new(&path),
                BufferProperties::text(),
                false,
            ) {
                let client = ctx.clients.get_mut(client_handle);
                client.set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);
            }
            ctx.editor.string_pool.release(path);

            ctx.editor.enter_mode(ModeKind::default());
            Some(EditorFlow::Continue)
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
            ctx.editor.enter_mode(ModeKind::Picker);
        } else {
            ctx.editor
                .status_bar
                .write(MessageKind::Error)
                .str("no buffer opened");
        }
    }
}

pub mod custom {
    use super::*;

    pub fn enter_mode(ctx: &mut EditorContext, continuation: &str, prompt: &str) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorFlow> {
            match poll {
                ReadLinePoll::Pending => (),
                ReadLinePoll::Submitted => {
                    if ctx.editor.picker.cursor().is_none() {
                        ctx.editor.enter_mode(ModeKind::default());
                        return Some(EditorFlow::Continue);
                    }

                    let continuation = &ctx.editor.mode.picker_state.continuation;
                    let continuation = ctx.editor.string_pool.acquire_with(continuation);
                    let result = CommandManager::eval(ctx, Some(client_handle), &continuation);
                    let flow = CommandManager::unwrap_eval_result(
                        ctx,
                        result,
                        &continuation,
                        Some("picker-continuation"),
                    );
                    ctx.editor.string_pool.release(continuation);
                    ctx.editor.enter_mode(ModeKind::default());
                    return Some(flow);
                }
                ReadLinePoll::Canceled => ctx.editor.enter_mode(ModeKind::default()),
            }
            Some(EditorFlow::Continue)
        }

        ctx.editor.read_line.set_prompt(prompt);
        let state = &mut ctx.editor.mode.picker_state;
        state.on_client_keys = on_client_keys;
        state.continuation.clear();
        state.continuation.push_str(continuation);
        ctx.editor.enter_mode(ModeKind::Picker);
    }
}

