use crate::{
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    client_event::Key,
    editor::KeysIterator,
    mode::{Mode, ModeContext, ModeOperation, ModeState},
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

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        let handle = match ctx.current_buffer_view_handle() {
            Some(handle) => handle,
            None => return ModeOperation::EnterMode(Mode::default()),
        };

        match keys.next() {
            Key::Esc => {
                unwrap_or_none!(ctx.buffer_views.get_mut(handle)).commit_edits(ctx.buffers);
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
            Key::Tab => ctx.buffer_views.insert_text(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
                "\t",
            ),
            Key::Enter => ctx.buffer_views.insert_line_break_with_identation(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
            ),
            Key::Char(c) => {
                let mut buf = [0; std::mem::size_of::<char>()];
                let s = c.encode_utf8(&mut buf);
                ctx.buffer_views.insert_text(
                    ctx.buffers,
                    ctx.word_database,
                    &ctx.config.syntaxes,
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
                ctx.buffer_views.delete_in_selection(
                    ctx.buffers,
                    ctx.word_database,
                    &ctx.config.syntaxes,
                    handle,
                );
            }
            Key::Delete => {
                unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                    ctx.buffers,
                    CursorMovement::ColumnsForward(1),
                    CursorMovementKind::PositionOnly,
                );
                ctx.buffer_views.delete_in_selection(
                    ctx.buffers,
                    ctx.word_database,
                    &ctx.config.syntaxes,
                    handle,
                );
            }
            Key::Ctrl('w') => {
                unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                    ctx.buffers,
                    CursorMovement::WordsBackward(1),
                    CursorMovementKind::PositionOnly,
                );
                ctx.buffer_views.delete_in_selection(
                    ctx.buffers,
                    ctx.word_database,
                    &ctx.config.syntaxes,
                    handle,
                );
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
        word_position.column_byte_index = word_position.column_byte_index.saturating_sub(1);
        let word = buffer.content.word_at(word_position);

        if matches!(word.kind, WordKind::Identifier)
            && word_position.column_byte_index
                >= word.end_position().column_byte_index.saturating_sub(1)
        {
            ctx.picker.filter(&ctx.word_database, word.text);
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
    if let Some(entry_name) = ctx.picker.current_entry_name(&ctx.word_database) {
        ctx.buffer_views.apply_completion(
            ctx.buffers,
            ctx.word_database,
            &ctx.config.syntaxes,
            handle,
            &entry_name,
        );
    }
}
