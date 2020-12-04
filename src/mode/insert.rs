use crate::{
    buffer::Text,
    buffer_position::BufferPosition,
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    client_event::Key,
    editor::KeysIterator,
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    register::AUTO_MACRO_REGISTER,
    word_database::WordKind,
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        ctx.picker.reset();
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.picker.reset();
    }

    fn on_client_keys(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        let handle = match ctx.current_buffer_view_handle() {
            Some(handle) => handle,
            None => return ModeOperation::EnterMode(Mode::default()),
        };

        let key = keys.next();
        let keys = ();
        let _ = keys;

        ctx.registers
            .append_fmt(AUTO_MACRO_REGISTER, format_args!("{}", key));

        match key {
            Key::Esc => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
                unwrap_or_none!(ctx.buffers.get_mut(buffer_view.buffer_handle)).commit_edits();
                return ModeOperation::EnterMode(Mode::default());
            }
            Key::Left => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::ColumnsBackward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Down => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::LinesForward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Up => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::LinesBackward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Right => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::ColumnsForward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Tab => ctx.buffer_views.insert_text_at_cursor_positions(
                ctx.buffers,
                ctx.word_database,
                handle,
                "\t",
            ),
            Key::Enter => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
                let cursor_count = buffer_view.cursors[..].len();
                let buffer_handle = buffer_view.buffer_handle;

                let mut text = Text::new();

                for i in 0..cursor_count {
                    let position =
                        unwrap_or_none!(ctx.buffer_views.get(handle)).cursors[i].position;
                    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_handle));

                    text.clear();
                    text.push_str("\n");
                    let indentation_word = buffer
                        .content()
                        .word_at(BufferPosition::line_col(position.line_index, 0));
                    if indentation_word.kind == WordKind::Whitespace {
                        let indentation_len =
                            position.column_byte_index.min(indentation_word.text.len());
                        text.push_str(&indentation_word.text[..indentation_len]);
                    }

                    ctx.buffer_views.insert_text_at_position(
                        ctx.buffers,
                        ctx.word_database,
                        handle,
                        position,
                        text.as_str(),
                    );
                }
            }
            Key::Char(c) => {
                let mut buf = [0; std::mem::size_of::<char>()];
                let s = c.encode_utf8(&mut buf);
                ctx.buffer_views.insert_text_at_cursor_positions(
                    ctx.buffers,
                    ctx.word_database,
                    handle,
                    s,
                );
            }
            Key::Backspace => {
                unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                    ctx.buffers,
                    CursorMovement::ColumnsBackward(1),
                    CursorMovementKind::PositionOnly,
                );
                ctx.buffer_views
                    .delete_in_cursor_ranges(ctx.buffers, ctx.word_database, handle);
            }
            Key::Delete => {
                unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                    ctx.buffers,
                    CursorMovement::ColumnsForward(1),
                    CursorMovementKind::PositionOnly,
                );
                ctx.buffer_views
                    .delete_in_cursor_ranges(ctx.buffers, ctx.word_database, handle);
            }
            Key::Ctrl('w') => {
                unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                    ctx.buffers,
                    CursorMovement::WordsBackward(1),
                    CursorMovementKind::PositionOnly,
                );
                ctx.buffer_views
                    .delete_in_cursor_ranges(ctx.buffers, ctx.word_database, handle);
            }
            Key::Ctrl('n') => {
                apply_completion(ctx, handle, 1);
                return ModeOperation::None;
            }
            Key::Ctrl('p') => {
                apply_completion(ctx, handle, -1);
                return ModeOperation::None;
            }
            _ => (),
        }

        let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
        let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));
        let mut word_position = buffer_view.cursors.main_cursor().position;
        word_position.column_byte_index =
            buffer.content().line_at(word_position.line_index).as_str()
                [..word_position.column_byte_index]
                .char_indices()
                .next_back()
                .unwrap_or((0, char::default()))
                .0;
        let word = buffer.content().word_at(word_position);

        if matches!(word.kind, WordKind::Identifier)
            && word_position.column_byte_index
                >= word.end_position().column_byte_index.saturating_sub(1)
        {
            ctx.picker.filter(ctx.word_database, word.text);
            if ctx.picker.height(usize::MAX) == 1 {
                ctx.picker.clear_filtered();
            }
        } else {
            ctx.picker.clear_filtered();
        }

        ModeOperation::None
    }
}

fn apply_completion(ctx: &mut ModeContext, handle: BufferViewHandle, cursor_movement: isize) {
    ctx.picker.move_cursor(cursor_movement);
    if let Some(entry) = ctx.picker.current_entry(ctx.word_database) {
        ctx.buffer_views
            .apply_completion(ctx.buffers, ctx.word_database, handle, entry.name);
    }
}
