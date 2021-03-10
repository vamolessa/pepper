use crate::{
    buffer_view::BufferViewError,
    command::CommandManager,
    editor::KeysIterator,
    editor_utils::{MessageKind, ReadLinePoll},
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    platform::Key,
    word_database::WordIndicesIter,
};

pub struct State {
    on_client_keys:
        fn(ctx: &mut ModeContext, &mut KeysIterator, ReadLinePoll) -> Option<ModeOperation>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _| None,
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
        let poll = ctx
            .editor
            .read_line
            .poll(ctx.platform, &ctx.editor.buffered_keys, keys);
        if let ReadLinePoll::Pending = poll {
            keys.put_back();
            match keys.next(&ctx.editor.buffered_keys) {
                Key::Ctrl('n') | Key::Ctrl('j') | Key::Down => ctx.editor.picker.move_cursor(1),
                Key::Ctrl('p') | Key::Ctrl('k') | Key::Up => ctx.editor.picker.move_cursor(-1),
                Key::Ctrl('d') | Key::PageDown => {
                    let picker_height = ctx
                        .editor
                        .picker
                        .height(ctx.editor.config.picker_max_height as _)
                        as isize;
                    ctx.editor.picker.move_cursor(picker_height / 2);
                }
                Key::Ctrl('u') | Key::PageUp => {
                    let picker_height = ctx
                        .editor
                        .picker
                        .height(ctx.editor.config.picker_max_height as _)
                        as isize;
                    ctx.editor.picker.move_cursor(-picker_height / 2);
                }
                Key::Ctrl('b') | Key::Home => {
                    let cursor = ctx.editor.picker.cursor() as isize;
                    ctx.editor.picker.move_cursor(-cursor);
                }
                Key::Ctrl('e') | Key::End => {
                    let cursor = ctx.editor.picker.cursor() as isize;
                    let entry_count = ctx.editor.picker.len() as isize;
                    ctx.editor.picker.move_cursor(entry_count - cursor - 1);
                }
                _ => ctx
                    .editor
                    .picker
                    .filter(WordIndicesIter::empty(), ctx.editor.read_line.input()),
            }
        }

        (this.on_client_keys)(ctx, keys, poll)
    }
}

pub mod buffer {
    use super::*;

    use std::path::Path;

    use crate::{buffer::Buffer, navigation_history::NavigationHistory, picker::Picker};

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

            let path = match ctx
                .editor
                .picker
                .current_entry(&ctx.editor.word_database, &ctx.editor.commands)
            {
                Some(entry) => entry.name,
                None => {
                    Mode::change_to(ctx, ModeKind::default());
                    return None;
                }
            };

            NavigationHistory::save_client_snapshot(
                ctx.clients,
                ctx.client_handle,
                &ctx.editor.buffer_views,
            );

            let mut buf = ctx.editor.string_pool.acquire();
            buf.push_str(path);
            let path = &buf[..];

            match ctx.editor.buffer_views.buffer_view_handle_from_path(
                ctx.client_handle,
                &mut ctx.editor.buffers,
                &mut ctx.editor.word_database,
                &ctx.editor.current_directory,
                Path::new(path),
                None,
                &mut ctx.editor.events,
            ) {
                Ok(handle) => {
                    if let Some(client) = ctx.clients.get_mut(ctx.client_handle) {
                        client.set_buffer_view_handle(Some(handle));
                    }
                }
                Err(BufferViewError::InvalidPath) => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("invalid path '{}'", path)),
            }

            ctx.editor.string_pool.release(buf);
            Mode::change_to(ctx, ModeKind::default());
            None
        }

        fn add_buffer_to_picker(picker: &mut Picker, buffer: &Buffer) {
            if let Some(path) = buffer.path().and_then(|p| p.to_str()) {
                picker.add_custom_entry(path, if buffer.needs_save() { "changed" } else { "" });
            }
        }

        ctx.editor.read_line.set_prompt("buffer:");
        ctx.editor.picker.clear();

        let buffers = &ctx.editor.buffers;
        let buffer_views = &ctx.editor.buffer_views;
        let prevous_buffer_handle = ctx
            .clients
            .get(ctx.client_handle)
            .and_then(|c| c.previous_buffer_view_handle())
            .and_then(|h| buffer_views.get(h))
            .map(|v| v.buffer_handle);

        if let Some(buffer) = prevous_buffer_handle.and_then(|h| buffers.get(h)) {
            add_buffer_to_picker(&mut ctx.editor.picker, buffer);
        }

        for buffer in ctx.editor.buffers.iter() {
            let buffer_handle = buffer.handle();
            if prevous_buffer_handle
                .map(|h| h != buffer_handle)
                .unwrap_or(true)
            {
                add_buffer_to_picker(&mut ctx.editor.picker, buffer);
            }
        }

        ctx.editor.picker.filter(WordIndicesIter::empty(), "");
        if ctx.editor.picker.len() > 0 {
            ctx.editor.mode.picker_state.on_client_keys = on_client_keys;
            Mode::change_to(ctx, ModeKind::Picker);
        }
    }
}

pub mod custom {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => None,
                ReadLinePoll::Submitted => {
                    let continuation = ctx.editor.commands.continuation.take().unwrap();
                    let operation = CommandManager::eval_body_and_print(
                        ctx.editor,
                        ctx.platform,
                        ctx.clients,
                        Some(ctx.client_handle),
                        &continuation,
                    )
                    .map(Into::into);
                    ctx.editor.string_pool.release(continuation);

                    if ctx.editor.mode.kind() == ModeKind::Picker
                        && ctx.editor.commands.continuation.is_none()
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
        if ctx.editor.picker.len() > 0 {
            ctx.editor.mode.picker_state.on_client_keys = on_client_keys;
            Mode::change_to(ctx, ModeKind::Picker);
        }
    }
}
