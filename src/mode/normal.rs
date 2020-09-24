use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer::Buffer,
    buffer_position::BufferPosition,
    buffer_view::{CursorMovement, CursorMovementKind},
    client_event::Key,
    cursor::Cursor,
    editor::KeysIterator,
    mode::{Mode, ModeContext, ModeOperation, ModeState},
};

pub struct State {
    movement_kind: CursorMovementKind,
}

impl Default for State {
    fn default() -> Self {
        Self {
            movement_kind: CursorMovementKind::PositionThenAnchor,
        }
    }
}

impl ModeState for State {
    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        let handle = match ctx.current_buffer_view_handle() {
            Some(handle) => handle,
            None => return on_event_no_buffer(ctx, keys),
        };

        match keys.next() {
            Key::Char('h') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::ColumnsBackward(1),
                self.movement_kind,
            ),
            Key::Char('j') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::LinesForward(1),
                self.movement_kind,
            ),
            Key::Char('k') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::LinesBackward(1),
                self.movement_kind,
            ),
            Key::Char('l') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::ColumnsForward(1),
                self.movement_kind,
            ),
            Key::Char('w') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::WordsForward(1),
                self.movement_kind,
            ),
            Key::Char('b') => unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                CursorMovement::WordsBackward(1),
                self.movement_kind,
            ),
            Key::Char('n') => {
                unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                    .move_to_next_search_match(ctx.buffers, self.movement_kind);
            }
            Key::Char('p') => {
                unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                    .move_to_previous_search_match(ctx.buffers, self.movement_kind);
            }
            Key::Char('a') => {
                fn balanced_brackets(
                    buffer: &Buffer,
                    cursors: &mut [Cursor],
                    left: char,
                    right: char,
                ) {
                    for cursor in cursors {
                        let range =
                            buffer
                                .content
                                .find_balanced_chars_at(cursor.position, left, right);
                        if let Some(range) = range {
                            cursor.anchor = range.from;
                            cursor.position = range.to;
                        }
                    }
                }

                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));
                let mut cursors = buffer_view.cursors.mut_guard();

                match keys.next() {
                    Key::None => return ModeOperation::Pending,
                    Key::Char('w') => {
                        for cursor in &mut cursors[..] {
                            let (range, _) = buffer.content.find_word_at(cursor.position);
                            cursor.anchor = range.from;
                            cursor.position = range.to;
                        }
                    }
                    Key::Char('(') => balanced_brackets(buffer, &mut cursors[..], '(', ')'),
                    Key::Char('[') => balanced_brackets(buffer, &mut cursors[..], '[', ']'),
                    Key::Char('{') => balanced_brackets(buffer, &mut cursors[..], '{', '}'),
                    _ => (),
                }

                self.movement_kind = CursorMovementKind::PositionThenAnchor;
            }
            Key::Char('g') => {
                match keys.next() {
                    Key::None => return ModeOperation::Pending,
                    Key::Char('g') => {
                        return ModeOperation::EnterMode(Mode::Goto(Default::default()))
                    }
                    Key::Char('h') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                        .move_cursors(ctx.buffers, CursorMovement::Home, self.movement_kind),
                    Key::Char('j') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                        .move_cursors(ctx.buffers, CursorMovement::LastLine, self.movement_kind),
                    Key::Char('k') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                        .move_cursors(ctx.buffers, CursorMovement::FirstLine, self.movement_kind),
                    Key::Char('l') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                        .move_cursors(ctx.buffers, CursorMovement::End, self.movement_kind),
                    Key::Char('n') => {
                        let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                        let buffer =
                            unwrap_or_none!(ctx.buffers.get_mut(buffer_view.buffer_handle));

                        let main_cursor = buffer_view.cursors.main_cursor();
                        let main_position = buffer_view.cursors.main_cursor().position;

                        let (range, word) = buffer.content.find_word_at(main_cursor.position);

                        let search_ranges = buffer.search_ranges();
                        let is_on_search_word = search_ranges
                            .binary_search_by_key(&range.from, |r| r.from)
                            .is_ok();

                        if !search_ranges.is_empty() && is_on_search_word {
                            let range_index = match search_ranges
                                .binary_search_by_key(&main_position, |r| r.from)
                            {
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
                        self.movement_kind = CursorMovementKind::PositionThenAnchor;
                    }
                    _ => (),
                }
            }
            Key::Char('f') => match keys.next() {
                Key::None => return ModeOperation::Pending,
                Key::Char(c) => {
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

                    let mut cursors = buffer_view.cursors.mut_guard();
                    for cursor in &mut cursors[..] {
                        cursor.position.column_byte_index = buffer
                            .content
                            .line_at(cursor.position.line_index)
                            .next_char_from(cursor.position.column_byte_index, c)
                            .unwrap_or(cursor.position.column_byte_index);

                        if let CursorMovementKind::PositionThenAnchor = self.movement_kind {
                            cursor.anchor = cursor.position;
                        }
                    }
                }
                _ => (),
            },
            Key::Char('F') => match keys.next() {
                Key::None => return ModeOperation::Pending,
                Key::Char(c) => {
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

                    let mut cursors = buffer_view.cursors.mut_guard();
                    for cursor in &mut cursors[..] {
                        cursor.position.column_byte_index = buffer
                            .content
                            .line_at(cursor.position.line_index)
                            .previous_char_from(cursor.position.column_byte_index, c)
                            .unwrap_or(cursor.position.column_byte_index);

                        if let CursorMovementKind::PositionThenAnchor = self.movement_kind {
                            cursor.anchor = cursor.position;
                        }
                    }
                }
                _ => (),
            },
            Key::Char('t') => match keys.next() {
                Key::None => return ModeOperation::Pending,
                Key::Char(c) => {
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

                    let mut cursors = buffer_view.cursors.mut_guard();
                    for cursor in &mut cursors[..] {
                        cursor.position.column_byte_index = match buffer
                            .content
                            .line_at(cursor.position.line_index)
                            .next_char_from(cursor.position.column_byte_index, c)
                        {
                            Some(i) => i.saturating_sub(1),
                            None => cursor.position.column_byte_index,
                        };

                        if let CursorMovementKind::PositionThenAnchor = self.movement_kind {
                            cursor.anchor = cursor.position;
                        }
                    }
                }
                _ => (),
            },
            Key::Char('T') => match keys.next() {
                Key::None => return ModeOperation::Pending,
                Key::Char(c) => {
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

                    let mut cursors = buffer_view.cursors.mut_guard();
                    for cursor in &mut cursors[..] {
                        let line = buffer.content.line_at(cursor.position.line_index);
                        cursor.position.column_byte_index =
                            match line.previous_char_from(cursor.position.column_byte_index, c) {
                                Some(i) => line.as_str().len().min(i + 1),
                                None => cursor.position.column_byte_index,
                            };

                        if let CursorMovementKind::PositionThenAnchor = self.movement_kind {
                            cursor.anchor = cursor.position;
                        }
                    }
                }
                _ => (),
            },
            Key::Char('x') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

                let mut cursors = buffer_view.cursors.mut_guard();
                for cursor in &mut cursors[..] {
                    let line_len = buffer
                        .content
                        .line_at(cursor.position.line_index)
                        .as_str()
                        .len();

                    if cursor.anchor < cursor.position {
                        cursor.anchor.column_byte_index = 0;
                        cursor.position.column_byte_index = line_len;
                    } else {
                        cursor.anchor.column_byte_index = line_len;
                        cursor.position.column_byte_index = 0;
                    }
                }
            }
            Key::Char('X') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

                let mut cursors = buffer_view.cursors.mut_guard();
                let cursor_count = cursors[..].len();

                for i in 0..cursor_count {
                    let cursor = &mut cursors[i];
                    let line_range = if cursor.anchor < cursor.position {
                        cursor.anchor.line_index..cursor.position.line_index
                    } else {
                        (cursor.position.line_index + 1)..(cursor.anchor.line_index + 1)
                    };
                    cursor.anchor = cursor.position;

                    for line_index in line_range {
                        let position = BufferPosition::line_col(
                            line_index,
                            buffer.content.line_at(line_index).first_word_start(),
                        );

                        cursors.add(Cursor {
                            anchor: position,
                            position,
                        });
                    }
                }

                self.movement_kind = CursorMovementKind::PositionThenAnchor;
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
                    self.movement_kind,
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
                    self.movement_kind,
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
                self.movement_kind = CursorMovementKind::PositionThenAnchor;
            }
            Key::Char('i') => {
                ctx.buffer_views.delete_in_selection(
                    ctx.buffers,
                    ctx.word_database,
                    &ctx.config.syntaxes,
                    handle,
                );
                unwrap_or_none!(ctx.buffer_views.get_mut(handle)).commit_edits(ctx.buffers);
                return ModeOperation::EnterMode(Mode::Insert(Default::default()));
            }
            Key::Char(';') => {
                let cursors = &mut unwrap_or_none!(ctx.buffer_views.get_mut(handle)).cursors;
                let main_cursor = *cursors.main_cursor();
                let mut cursors = cursors.mut_guard();
                cursors.clear();
                cursors.add(main_cursor);
                self.movement_kind = CursorMovementKind::PositionThenAnchor;
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

                self.movement_kind = if had_selection {
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
            Key::Char('_') => {
                let mut cursors = unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                    .cursors
                    .mut_guard();
                for cursor in &mut cursors[..] {
                    cursor.anchor = cursor.position;
                }
                self.movement_kind = CursorMovementKind::PositionThenAnchor;
            }
            Key::Char('(') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .cursors
                .previous_main_cursor(),
            Key::Char(')') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .cursors
                .next_main_cursor(),
            Key::Char('/') => return ModeOperation::EnterMode(Mode::Search(Default::default())),
            Key::Char('y') => {
                if let Ok(mut clipboard) = ClipboardContext::new() {
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
                    let mut text = String::new();
                    buffer_view.get_selection_text(ctx.buffers, &mut text);
                    let _ = clipboard.set_contents(text);
                }
                self.movement_kind = CursorMovementKind::PositionThenAnchor;
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
                self.movement_kind = CursorMovementKind::PositionThenAnchor;
            }
            Key::Char('u') => {
                ctx.buffer_views
                    .undo(ctx.buffers, &ctx.config.syntaxes, handle);
                self.movement_kind = CursorMovementKind::PositionThenAnchor;
            }
            Key::Char('U') => {
                ctx.buffer_views
                    .redo(ctx.buffers, &ctx.config.syntaxes, handle);
                self.movement_kind = CursorMovementKind::PositionThenAnchor;
            }
            _ => {
                keys.put_back();
                return on_event_no_buffer(ctx, keys);
            }
        };

        ModeOperation::None
    }
}

fn on_event_no_buffer(_: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    match keys.next() {
        Key::Char(':') => ModeOperation::EnterMode(Mode::Script(Default::default())),
        _ => ModeOperation::None,
    }
}
