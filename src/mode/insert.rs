use crate::{
    buffer_position::BufferPosition,
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    client::{TargetClient, ClientCollection},
    client_event::Key,
    editor::{Editor, KeysIterator},
    mode::{Mode, ModeKind, ModeOperation, ModeState},
    register::AUTO_MACRO_REGISTER,
    word_database::WordKind,
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(editor: &mut Editor, _: &mut ClientCollection, _: TargetClient) {
        editor.picker.reset();
    }

    fn on_exit(editor: &mut Editor, _: &mut ClientCollection, _: TargetClient) {
        editor.picker.reset();
    }

    fn on_client_keys(
        editor: &mut Editor,
        clients: &mut ClientCollection,
        target: TargetClient,
        keys: &mut KeysIterator,
    ) -> ModeOperation {
        let handle = match clients.get(target).and_then(|c| c.current_buffer_view_handle()) {
            Some(handle) => handle,
            None => {
                Mode::change_to(editor, clients, target, ModeKind::default());
                return ModeOperation::None;
            }
        };

        let key = keys.next(&editor.buffered_keys);
        drop(keys);

        editor
            .registers
            .append_fmt(AUTO_MACRO_REGISTER, format_args!("{}", key));

        match key {
            Key::Esc => {
                let buffer_view = unwrap_or_none!(editor.buffer_views.get(handle));
                unwrap_or_none!(editor.buffers.get_mut(buffer_view.buffer_handle)).commit_edits();
                Mode::change_to(editor, clients, target, ModeKind::default());
                return ModeOperation::None;
            }
            Key::Left => unwrap_or_none!(editor.buffer_views.get_mut(handle)).move_cursors(
                &editor.buffers,
                CursorMovement::ColumnsBackward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Down => unwrap_or_none!(editor.buffer_views.get_mut(handle)).move_cursors(
                &editor.buffers,
                CursorMovement::LinesForward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Up => unwrap_or_none!(editor.buffer_views.get_mut(handle)).move_cursors(
                &editor.buffers,
                CursorMovement::LinesBackward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Right => unwrap_or_none!(editor.buffer_views.get_mut(handle)).move_cursors(
                &editor.buffers,
                CursorMovement::ColumnsForward(1),
                CursorMovementKind::PositionAndAnchor,
            ),
            Key::Tab => editor.buffer_views.insert_text_at_cursor_positions(
                &mut editor.buffers,
                &mut editor.word_database,
                handle,
                "\t",
                &mut editor.events,
            ),
            Key::Enter => {
                let buffer_view = unwrap_or_none!(editor.buffer_views.get(handle));
                let cursor_count = buffer_view.cursors[..].len();
                let buffer_handle = buffer_view.buffer_handle;

                let mut buf = [0; 128];
                buf[0] = b'\n';

                for i in 0..cursor_count {
                    let position =
                        unwrap_or_none!(editor.buffer_views.get(handle)).cursors[i].position;
                    let buffer = unwrap_or_none!(editor.buffers.get(buffer_handle));

                    let mut len = 1;
                    let indentation_word = buffer
                        .content()
                        .word_at(BufferPosition::line_col(position.line_index, 0));
                    if indentation_word.kind == WordKind::Whitespace {
                        let indentation_len = position
                            .column_byte_index
                            .min(indentation_word.text.len())
                            .min(buf.len() - 1);
                        len += indentation_len;
                        buf[1..len]
                            .copy_from_slice(indentation_word.text[..indentation_len].as_bytes());
                    }

                    let text = std::str::from_utf8(&buf[..len]).unwrap_or("\n");

                    editor.buffer_views.insert_text_at_position(
                        &mut editor.buffers,
                        &mut editor.word_database,
                        handle,
                        position,
                        text,
                        &mut editor.events,
                    );
                }
            }
            Key::Char(c) => {
                let mut buf = [0; std::mem::size_of::<char>()];
                let s = c.encode_utf8(&mut buf);
                editor.buffer_views.insert_text_at_cursor_positions(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    handle,
                    s,
                    &mut editor.events,
                );
            }
            Key::Backspace => {
                unwrap_or_none!(editor.buffer_views.get_mut(handle)).move_cursors(
                    &editor.buffers,
                    CursorMovement::ColumnsBackward(1),
                    CursorMovementKind::PositionOnly,
                );
                editor.buffer_views.delete_in_cursor_ranges(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    handle,
                    &mut editor.events,
                );
            }
            Key::Delete => {
                unwrap_or_none!(editor.buffer_views.get_mut(handle)).move_cursors(
                    &editor.buffers,
                    CursorMovement::ColumnsForward(1),
                    CursorMovementKind::PositionOnly,
                );
                editor.buffer_views.delete_in_cursor_ranges(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    handle,
                    &mut editor.events,
                );
            }
            Key::Ctrl('w') => {
                unwrap_or_none!(editor.buffer_views.get_mut(handle)).move_cursors(
                    &editor.buffers,
                    CursorMovement::WordsBackward(1),
                    CursorMovementKind::PositionOnly,
                );
                editor.buffer_views.delete_in_cursor_ranges(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    handle,
                    &mut editor.events,
                );
            }
            Key::Ctrl('n') => {
                apply_completion(editor, handle, 1);
                return ModeOperation::None;
            }
            Key::Ctrl('p') => {
                apply_completion(editor, handle, -1);
                return ModeOperation::None;
            }
            _ => (),
        }

        let buffer_view = unwrap_or_none!(editor.buffer_views.get(handle));
        let buffer = unwrap_or_none!(editor.buffers.get(buffer_view.buffer_handle));
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
            editor.picker.filter(&mut editor.word_database, word.text);
            if editor.picker.height(usize::MAX) == 1 {
                editor.picker.clear_filtered();
            }
        } else {
            editor.picker.clear_filtered();
        }

        ModeOperation::None
    }
}

fn apply_completion(editor: &mut Editor, handle: BufferViewHandle, cursor_movement: isize) {
    editor.picker.move_cursor(cursor_movement);
    if let Some(entry) = editor.picker.current_entry(&mut editor.word_database) {
        editor.buffer_views.apply_completion(
            &mut editor.buffers,
            &mut editor.word_database,
            handle,
            entry.name,
            &mut editor.events,
        );
    }
}
