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
    word_database::WordKind,
};

#[derive(Default)]
pub struct State {
    lsp_client_handle: Option<lsp::ClientHandle>,
    //lsp_completion_position: Option<BufferPosition>,
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.editor.picker.clear();
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.editor.picker.clear();
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
            Key::Left => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::ColumnsBackward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Down => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::LinesForward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Up => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::LinesBackward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Right => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::ColumnsForward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
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

                let buffer_handle = buffer_view.buffer_handle;
                let lsp_client = find_lsp_client(ctx.editor, buffer_handle)?;
                if lsp_client.completion_triggers().contains(c) {
                    // TODO
                }

                if lsp_client.signature_help_triggers().contains(c) {
                    // TODO
                }
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
                apply_completion(ctx.editor, handle, 1);
                return None;
            }
            Key::Ctrl('p') => {
                apply_completion(ctx.editor, handle, -1);
                return None;
            }
            _ => (),
        }

        ctx.editor.trigger_event_handlers(ctx.platform, ctx.clients);

        let buffer_view = ctx.editor.buffer_views.get(handle)?;
        let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?;

        let mut word_position = buffer_view.cursors.main_cursor().position;
        word_position.column_byte_index =
            buffer.content().line_at(word_position.line_index).as_str()
                [..word_position.column_byte_index]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        let word = buffer.content().word_at(word_position);

        if matches!(word.kind, WordKind::Identifier)
            && word_position.column_byte_index
                >= word.end_position().column_byte_index.saturating_sub(1)
        {
            ctx.editor
                .picker
                .filter(ctx.editor.word_database.word_indices(), word.text);
            if ctx.editor.picker.len() == 1 {
                ctx.editor.picker.clear_filtered();
            } else if ctx.editor.picker.cursor().is_none() {
                ctx.editor.picker.move_cursor(0);
            }
        } else {
            ctx.editor.picker.clear_filtered();
        }

        None
    }
}

fn find_lsp_client(editor: &Editor, buffer_handle: BufferHandle) -> Option<&lsp::Client> {
    let buffer_path = editor
        .buffers
        .get(buffer_handle)?
        .path()
        .to_str()?
        .as_bytes();

    let lsp_client_handle = editor.mode.insert_state.lsp_client_handle;
    lsp_client_handle
        .and_then(|h| editor.lsp.get(h))
        .or_else(|| editor.lsp.clients().find(|c| c.handles_path(buffer_path)))
}

fn apply_completion(editor: &mut Editor, handle: BufferViewHandle, cursor_movement: isize) {
    editor.picker.move_cursor(cursor_movement);
    let entry = match editor.picker.current_entry(&editor.word_database) {
        Some((_, entry)) => entry,
        None => return,
    };
    let buffer_view = match editor.buffer_views.get_mut(handle) {
        Some(view) => view,
        None => return,
    };
    let buf = editor.string_pool.acquire_with(entry);
    buffer_view.apply_completion(
        &mut editor.buffers,
        &mut editor.word_database,
        &buf,
        &mut editor.events,
    );
    editor.string_pool.release(buf);
}
