use std::fmt::Write;

use crate::{
    buffer::BufferHandle,
    buffer_position::BufferPosition,
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    editor::{Editor, KeysIterator},
    lsp,
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    platform::Key,
    register::AUTO_MACRO_REGISTER,
    word_database::{WordIndicesIter, WordKind},
};

#[derive(Default)]
pub struct State {
    lsp_client_handle: Option<lsp::ClientHandle>,
    completion_positions: Vec<BufferPosition>,
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        cancel_completion(ctx.editor);
    }

    fn on_exit(ctx: &mut ModeContext) {
        cancel_completion(ctx.editor);
    }

    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<ModeOperation> {
        let handle = match ctx
            .clients
            .get(ctx.client_handle)
            .and_then(|c| c.buffer_view_handle())
        {
            Some(handle) => handle,
            None => {
                Mode::change_to(ctx, ModeKind::default());
                return None;
            }
        };

        let key = keys.next(&ctx.editor.buffered_keys);
        drop(keys);

        let register = ctx.editor.registers.get_mut(AUTO_MACRO_REGISTER);
        let _ = write!(register, "{}", key);

        match key {
            Key::Esc => {
                let buffer_view = ctx.editor.buffer_views.get(handle)?;
                ctx.editor
                    .buffers
                    .get_mut(buffer_view.buffer_handle)?
                    .commit_edits();
                Mode::change_to(ctx, ModeKind::default());
                return None;
            }
            Key::Left => {
                ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::ColumnsBackward(1),
                    CursorMovementKind::PositionAndAnchor,
                );
                cancel_completion(ctx.editor);
                return None;
            }
            Key::Down => {
                ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::LinesForward(1),
                    CursorMovementKind::PositionAndAnchor,
                );
                cancel_completion(ctx.editor);
                return None;
            }
            Key::Up => {
                ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::LinesBackward(1),
                    CursorMovementKind::PositionAndAnchor,
                );
                cancel_completion(ctx.editor);
                return None;
            }
            Key::Right => {
                ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::ColumnsForward(1),
                    CursorMovementKind::PositionAndAnchor,
                );
                cancel_completion(ctx.editor);
                return None;
            }
            Key::Tab => ctx
                .editor
                .buffer_views
                .get_mut(handle)?
                .insert_text_at_cursor_positions(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    "\t",
                    &mut ctx.editor.events,
                ),
            Key::Enter => {
                let buffer_view = ctx.editor.buffer_views.get(handle)?;
                let cursor_count = buffer_view.cursors[..].len();
                let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle)?;

                let mut buf = ctx.editor.string_pool.acquire();
                for i in (0..cursor_count).rev() {
                    let position = buffer_view.cursors[i].position;

                    buf.push('\n');
                    let indentation_word = buffer
                        .content()
                        .word_at(BufferPosition::line_col(position.line_index, 0));
                    if indentation_word.kind == WordKind::Whitespace {
                        let indentation_len =
                            position.column_byte_index.min(indentation_word.text.len());
                        buf.push_str(&indentation_word.text[..indentation_len]);
                    }

                    buffer.insert_text(
                        &mut ctx.editor.word_database,
                        position,
                        &buf,
                        &mut ctx.editor.events,
                    );
                    buf.clear();
                }
                ctx.editor.string_pool.release(buf);
            }
            Key::Char(c) => {
                let mut buf = [0; std::mem::size_of::<char>()];
                let s = c.encode_utf8(&mut buf);
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                buffer_view.insert_text_at_cursor_positions(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    s,
                    &mut ctx.editor.events,
                );
            }
            Key::Backspace => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                buffer_view.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::ColumnsBackward(1),
                    CursorMovementKind::PositionOnly,
                );
                buffer_view.delete_text_in_cursor_ranges(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );
            }
            Key::Delete => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                buffer_view.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::ColumnsForward(1),
                    CursorMovementKind::PositionOnly,
                );
                buffer_view.delete_text_in_cursor_ranges(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );
            }
            Key::Ctrl('w') => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                buffer_view.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::WordsBackward(1),
                    CursorMovementKind::PositionOnly,
                );
                buffer_view.delete_text_in_cursor_ranges(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );
            }
            Key::Ctrl('n') => {
                apply_completion(ctx, handle, 1);
                return None;
            }
            Key::Ctrl('p') => {
                apply_completion(ctx, handle, -1);
                return None;
            }
            _ => return None,
        };

        ctx.editor.trigger_event_handlers(ctx.platform, ctx.clients);
        update_completions(ctx.editor, handle);
        None
    }
}

fn cancel_completion(editor: &mut Editor) {
    editor.picker.clear();
    editor.mode.insert_state.completion_positions.clear();
}

fn find_lsp_client(editor: &mut Editor, buffer_handle: BufferHandle) -> Option<lsp::ClientHandle> {
    if let Some(handle) = editor.mode.insert_state.lsp_client_handle {
        return Some(handle);
    }

    let buffer_path = editor
        .buffers
        .get(buffer_handle)?
        .path()
        .to_str()?
        .as_bytes();

    let client_handle = editor
        .lsp
        .clients()
        .find(|c| c.handles_path(buffer_path))?
        .handle();

    editor.mode.insert_state.lsp_client_handle = Some(client_handle);
    Some(client_handle)
}

pub fn update_completions(editor: &mut Editor, buffer_view_handle: BufferViewHandle) {
    let state = &mut editor.mode.insert_state;
    let buffer_view = match editor.buffer_views.get(buffer_view_handle) {
        Some(buffer_view) => buffer_view,
        None => return cancel_completion(editor),
    };
    let buffer = match editor.buffers.get(buffer_view.buffer_handle) {
        Some(buffer) => buffer,
        None => return cancel_completion(editor),
    };
    let content = buffer.content();

    let main_cursor_position = buffer_view.cursors.main_cursor().position;
    let word = content.word_at(content.position_before(main_cursor_position));

    let main_cursor_index = buffer_view.cursors.main_cursor_index();
    let main_completion_position = match state.completion_positions.get(main_cursor_index) {
        Some(&position) => {
            if main_cursor_position < position {
                return cancel_completion(editor);
            }

            position
        }
        None => {
            // TODO: create a config for the min word len that triggers auto complete
            if word.kind != WordKind::Identifier || word.text.len() < 3 {
                return cancel_completion(editor);
            }

            state.completion_positions.clear();
            for cursor in &buffer_view.cursors[..] {
                let word = content.word_at(content.position_before(cursor.position));
                let position = match word.kind {
                    WordKind::Identifier => word.position,
                    _ => cursor.position,
                };
                state.completion_positions.push(position);
            }

            // TODO: if there's lsp server, send it a 'completion' request

            state.completion_positions[main_cursor_index]
        }
    };

    if word.position > main_completion_position {
        return cancel_completion(editor);
    }

    match editor.mode.insert_state.lsp_client_handle {
        Some(_) => editor.picker.filter(WordIndicesIter::empty(), word.text),
        None => {
            editor
                .picker
                .filter(editor.word_database.word_indices(), word.text);
            if editor.picker.cursor().is_none() {
                editor.picker.move_cursor(0);
            }
            if editor.picker.len() == 1 {
                editor.picker.clear();
            }
        }
    }
}

fn apply_completion(
    ctx: &mut ModeContext,
    buffer_view_handle: BufferViewHandle,
    cursor_movement: isize,
) {
    let buffer_view = match ctx.editor.buffer_views.get_mut(buffer_view_handle) {
        Some(view) => view,
        None => return,
    };

    ctx.editor.picker.move_cursor(cursor_movement);
    let entry = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
        Some((_, entry)) => entry,
        None => return,
    };

    let completion = ctx.editor.string_pool.acquire_with(entry);
    buffer_view.apply_completion(
        &mut ctx.editor.buffers,
        &mut ctx.editor.word_database,
        &completion,
        &ctx.editor.mode.insert_state.completion_positions,
        &mut ctx.editor.events,
    );
    ctx.editor.string_pool.release(completion);
}
