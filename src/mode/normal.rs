use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer_view::{CursorMovement, CursorMovementKind},
    client_event::Key,
    cursor::Cursor,
    editor::KeysIterator,
    mode::{Mode, ModeContext, ModeOperation},
};

pub struct State {
    pub movement_kind: CursorMovementKind,
}

impl Default for State {
    fn default() -> Self {
        Self {
            movement_kind: CursorMovementKind::PositionThenAnchor,
        }
    }
}

fn on_event_no_buffer(_: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    match keys.next() {
        Key::Char(':') => ModeOperation::EnterMode(Mode::Script),
        _ => ModeOperation::None,
    }
}

pub fn on_enter(_: &mut State, _: &mut ModeContext) {}
pub fn on_exit(state: &mut State, _: &mut ModeContext) {
    *state = State::default();
}

pub fn on_event(
    state: &mut State,
    ctx: &mut ModeContext,
    keys: &mut KeysIterator,
) -> ModeOperation {
    let handle = match ctx.current_buffer_view_handle() {
        Some(handle) => handle,
        None => return on_event_no_buffer(ctx, keys),
    };

    match keys.next() {
        Key::Char('h') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::ColumnsBackward(1),
            state.movement_kind,
        ),
        Key::Char('j') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::LinesForward(1),
            state.movement_kind,
        ),
        Key::Char('k') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::LinesBackward(1),
            state.movement_kind,
        ),
        Key::Char('l') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::ColumnsForward(1),
            state.movement_kind,
        ),
        Key::Char('w') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::WordsForward(1),
            state.movement_kind,
        ),
        Key::Char('b') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
            ctx.buffers,
            CursorMovement::WordsBackward(1),
            state.movement_kind,
        ),
        Key::Char('n') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_next_search_match(ctx.buffers, state.movement_kind);
        }
        Key::Char('p') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_previous_search_match(ctx.buffers, state.movement_kind);
        }
        Key::Char('g') => match keys.next() {
            Key::None => return ModeOperation::Pending,
            Key::Char('h') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::Home,
                state.movement_kind,
            ),
            Key::Char('j') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::LastLine,
                state.movement_kind,
            ),
            Key::Char('k') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::FirstLine,
                state.movement_kind,
            ),
            Key::Char('l') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::End,
                state.movement_kind,
            ),
            Key::Char('n') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = unwrap_or_none!(ctx.buffers.get_mut(buffer_view.buffer_handle));

                let main_cursor = buffer_view.cursors.main_cursor();
                let main_position = buffer_view.cursors.main_cursor().position;

                let (range, word) = buffer.content.find_word_at(main_cursor.position);

                let search_ranges = buffer.search_ranges();
                let is_on_search_word = search_ranges
                    .binary_search_by_key(&range.from, |r| r.from)
                    .is_ok();

                if !search_ranges.is_empty() && is_on_search_word {
                    let range_index =
                        match search_ranges.binary_search_by_key(&main_position, |r| r.from) {
                            Ok(index) => index + 1,
                            Err(index) => index,
                        };
                    let range_index = range_index % search_ranges.len();
                    let cursor_position = search_ranges[range_index].from;

                    buffer_view.cursors.mut_guard().add(Cursor {
                        anchor: cursor_position,
                        position: cursor_position,
                    });
                } else {
                    ctx.input.clear();
                    ctx.input.push_str(word);
                    buffer.set_search(&ctx.input);

                    let mut cursors = buffer_view.cursors.mut_guard();
                    cursors.clear();
                    cursors.add(Cursor {
                        anchor: range.from,
                        position: range.from,
                    });
                }
                state.movement_kind = CursorMovementKind::PositionThenAnchor;
            }
            _ => (),
        },
        Key::Char('x') => {
            let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
            let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

            let mut cursors = buffer_view.cursors.mut_guard();
            for cursor in &mut cursors[..] {
                if cursor.anchor < cursor.position {
                    cursor.anchor.column_index = 0;
                    cursor.position.column_index = buffer
                        .content
                        .line_at(cursor.position.line_index)
                        .char_count();
                } else {
                    cursor.position.column_index = 0;
                    cursor.anchor.column_index = buffer
                        .content
                        .line_at(cursor.position.line_index)
                        .char_count();
                }
            }
        }
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
                state.movement_kind,
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
                state.movement_kind,
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
            state.movement_kind = CursorMovementKind::PositionThenAnchor;
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
        Key::Char(';') => {
            let cursors = &mut unwrap_or_none!(ctx.buffer_views.get_mut(handle)).cursors;
            let main_cursor = *cursors.main_cursor();
            let mut cursors = cursors.mut_guard();
            cursors.clear();
            cursors.add(main_cursor);
            state.movement_kind = CursorMovementKind::PositionThenAnchor;
        }
        Key::Char('v') => {
            let mut cursors = unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .cursors
                .mut_guard();

            let mut had_selection = false;
            for cursor in &mut cursors[..] {
                if cursor.anchor != cursor.position {
                    cursor.anchor = cursor.position;
                    had_selection = true;
                }
            }

            state.movement_kind = if had_selection {
                CursorMovementKind::PositionThenAnchor
            } else {
                CursorMovementKind::PositionOnly
            };
        }
        Key::Char('V') => {
            let mut cursors = unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .cursors
                .mut_guard();
            for cursor in &mut cursors[..] {
                std::mem::swap(&mut cursor.anchor, &mut cursor.position);
            }
        }
        Key::Char('(') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
            .cursors
            .previous_main_cursor(),
        Key::Char(')') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
            .cursors
            .next_main_cursor(),
        Key::Char('/') => return ModeOperation::EnterMode(Mode::Search),
        Key::Char('y') => {
            if let Ok(mut clipboard) = ClipboardContext::new() {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
                let mut text = String::new();
                buffer_view.get_selection_text(ctx.buffers, &mut text);
                let _ = clipboard.set_contents(text);
            }
            state.movement_kind = CursorMovementKind::PositionThenAnchor;
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
            state.movement_kind = CursorMovementKind::PositionThenAnchor;
        }
        Key::Char('u') => {
            ctx.buffer_views
                .undo(ctx.buffers, &ctx.config.syntaxes, handle);
            state.movement_kind = CursorMovementKind::PositionThenAnchor;
        }
        Key::Char('U') => {
            ctx.buffer_views
                .redo(ctx.buffers, &ctx.config.syntaxes, handle);
            state.movement_kind = CursorMovementKind::PositionThenAnchor;
        }
        _ => {
            keys.put_back();
            return on_event_no_buffer(ctx, keys);
        }
    };

    ModeOperation::None
}
