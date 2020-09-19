use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer_view::{CursorMovement, CursorMovementKind},
    client_event::Key,
    cursor::Cursor,
    editor::KeysIterator,
    mode::{Mode, ModeContext, ModeOperation},
};

fn on_event_no_buffer(_: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    match keys.next() {
        Key::Char(':') => ModeOperation::EnterMode(Mode::Script),
        _ => ModeOperation::None,
    }
}

pub fn on_enter(_: &mut ModeContext) {}
pub fn on_exit(_: &mut ModeContext) {}

pub fn on_event(ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    let handle = match ctx.current_buffer_view_handle() {
        Some(handle) => handle,
        None => return on_event_no_buffer(ctx, keys),
    };

    match keys.next() {
        Key::Char('h') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::ColumnsBackward(1),
            CursorMovementKind::PositionWithAnchor,
        ),
        Key::Char('H') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::ColumnsBackward(1),
            CursorMovementKind::PositionOnly,
        ),
        Key::Char('j') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::LinesForward(1),
            CursorMovementKind::PositionWithAnchor,
        ),
        Key::Char('J') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::LinesForward(1),
            CursorMovementKind::PositionOnly,
        ),
        Key::Char('k') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::LinesBackward(1),
            CursorMovementKind::PositionWithAnchor,
        ),
        Key::Char('K') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::LinesBackward(1),
            CursorMovementKind::PositionOnly,
        ),
        Key::Char('l') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::ColumnsForward(1),
            CursorMovementKind::PositionWithAnchor,
        ),
        Key::Char('L') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::ColumnsForward(1),
            CursorMovementKind::PositionOnly,
        ),
        Key::Char('w') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::WordsForward(1),
            CursorMovementKind::PositionWithAnchor,
        ),
        Key::Char('W') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::WordsForward(1),
            CursorMovementKind::PositionOnly,
        ),
        Key::Char('b') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::WordsBackward(1),
            CursorMovementKind::PositionWithAnchor,
        ),
        Key::Char('B') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::WordsBackward(1),
            CursorMovementKind::PositionOnly,
        ),
        Key::Char('n') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_next_search_match(ctx.buffers, CursorMovementKind::PositionWithAnchor);
        }
        Key::Char('N') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_next_search_match(ctx.buffers, CursorMovementKind::PositionOnly);
        }
        Key::Char('p') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_previous_search_match(ctx.buffers, CursorMovementKind::PositionWithAnchor);
        }
        Key::Char('P') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_previous_search_match(ctx.buffers, CursorMovementKind::PositionOnly);
        }
        Key::Char('g') => match keys.next() {
            Key::None => return ModeOperation::Pending,
            Key::Char('z') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::FirstColumn,
                CursorMovementKind::PositionWithAnchor,
            ),
            Key::Char('Z') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::FirstColumn,
                CursorMovementKind::PositionOnly,
            ),
            Key::Char('h') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::Home,
                CursorMovementKind::PositionWithAnchor,
            ),
            Key::Char('H') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::Home,
                CursorMovementKind::PositionOnly,
            ),
            Key::Char('j') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::LastLine,
                CursorMovementKind::PositionWithAnchor,
            ),
            Key::Char('J') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::LastLine,
                CursorMovementKind::PositionOnly,
            ),
            Key::Char('k') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::FirstLine,
                CursorMovementKind::PositionWithAnchor,
            ),
            Key::Char('K') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::FirstLine,
                CursorMovementKind::PositionOnly,
            ),
            Key::Char('l') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::End,
                CursorMovementKind::PositionWithAnchor,
            ),
            Key::Char('L') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::End,
                CursorMovementKind::PositionOnly,
            ),
            _ => (),
        },
        Key::Char('s') => match keys.next() {
            Key::None => return ModeOperation::Pending,
            Key::Char('c') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                buffer_view.cursors.collapse_anchors();
            }
            Key::Char('d') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let main_cursor = buffer_view.cursors.main_cursor().clone();
                let mut cursors = buffer_view.cursors.mut_guard();
                cursors.clear();
                cursors.add_cursor(main_cursor);
            }
            Key::Char('o') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .cursors
                .swap_positions_and_anchors(),
            Key::Char('n') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = unwrap_or_none!(ctx.buffers.get_mut(buffer_view.buffer_handle));

                let main_cursor = buffer_view.cursors.main_cursor();
                let main_position = buffer_view.cursors.main_cursor().position;

                let search_ranges = buffer.search_ranges();
                if search_ranges.is_empty() {
                    let (range, word) = buffer.content.find_word_at(main_cursor.position);

                    ctx.input.clear();
                    ctx.input.push_str(word);
                    buffer.set_search(&ctx.input);

                    let mut cursors = buffer_view.cursors.mut_guard();
                    cursors.clear();
                    cursors.add_cursor(Cursor {
                        anchor: range.from,
                        position: range.from,
                    });
                } else {
                    let range_index =
                        match search_ranges.binary_search_by_key(&main_position, |r| r.from) {
                            Ok(index) => index + 1,
                            Err(index) => index,
                        };
                    let range_index = range_index % search_ranges.len();
                    let cursor_position = search_ranges[range_index].from;

                    buffer_view.cursors.mut_guard().add_cursor(Cursor {
                        anchor: cursor_position,
                        position: cursor_position,
                    });
                }
            }
            Key::Char('j') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));
                let mut cursor = *buffer_view.cursors.main_cursor();
                cursor.position.line_index += 1;
                cursor.position = buffer.content.saturate_position(cursor.position);
                cursor.anchor = cursor.position;
                buffer_view.cursors.mut_guard().add_cursor(cursor);
            }
            Key::Char('k') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));
                let mut cursor = *buffer_view.cursors.main_cursor();
                cursor.position.line_index = cursor.position.line_index.saturating_sub(1);
                cursor.position = buffer.content.saturate_position(cursor.position);
                cursor.anchor = cursor.position;
                buffer_view.cursors.mut_guard().add_cursor(cursor);
            }
            _ => (),
        },
        Key::Ctrl('d') => {
            let half_height = ctx
                .clients
                .get(ctx.target_client)
                .map(|c| c.height)
                .unwrap_or(0)
                / 2;
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::LinesForward(half_height as _),
                CursorMovementKind::PositionWithAnchor,
            );
        }
        Key::Ctrl('u') => {
            let half_height = ctx
                .clients
                .get(ctx.target_client)
                .map(|c| c.height)
                .unwrap_or(0)
                / 2;
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::LinesBackward(half_height as _),
                CursorMovementKind::PositionWithAnchor,
            );
        }
        Key::Char('d') => {
            ctx.buffer_views.delete_in_selection(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
            );
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).commit_edits(ctx.buffers);
        }
        Key::Char('i') => {
            ctx.buffer_views.delete_in_selection(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
            );
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).commit_edits(ctx.buffers);
            return ModeOperation::EnterMode(Mode::Insert);
        }
        Key::Char('/') => return ModeOperation::EnterMode(Mode::Search),
        Key::Char('y') => {
            if let Ok(mut clipboard) = ClipboardContext::new() {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
                let mut text = String::new();
                buffer_view.get_selection_text(ctx.buffers, &mut text);
                let _ = clipboard.set_contents(text);
            }
        }
        Key::Char('Y') => {
            ctx.buffer_views.delete_in_selection(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
            );
            if let Ok(text) = ClipboardContext::new().and_then(|mut c| c.get_contents()) {
                ctx.buffer_views.insert_text(
                    ctx.buffers,
                    ctx.word_database,
                    &ctx.config.syntaxes,
                    handle,
                    &text,
                );
            }
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).commit_edits(ctx.buffers);
        }
        Key::Char('u') => ctx
            .buffer_views
            .undo(ctx.buffers, &ctx.config.syntaxes, handle),
        Key::Char('U') => ctx
            .buffer_views
            .redo(ctx.buffers, &ctx.config.syntaxes, handle),
        _ => {
            keys.put_back();
            return on_event_no_buffer(ctx, keys);
        }
    };

    ModeOperation::None
}
