use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer::BufferContent,
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{CursorMovement, CursorMovementKind},
    client_event::Key,
    cursor::Cursor,
    editor::KeysIterator,
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    word_database::WordKind,
};

pub struct State {
    movement_kind: CursorMovementKind,
}

impl Default for State {
    fn default() -> Self {
        Self {
            movement_kind: CursorMovementKind::PositionAndAnchor,
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
            Key::Char('N') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = unwrap_or_none!(ctx.buffers.get_mut(buffer_view.buffer_handle));

                let main_position = buffer_view.cursors.main_cursor().position;
                let search_ranges = buffer.search_ranges();
                let current_range_index =
                    search_ranges.binary_search_by_key(&main_position, |r| r.from);

                if search_ranges.is_empty() || current_range_index.is_err() {
                    buffer.set_search_with(|c| {
                        let word = c.word_at(main_position);

                        let mut cursors = buffer_view.cursors.mut_guard();
                        cursors.clear();
                        cursors.add(Cursor {
                            anchor: word.position,
                            position: word.position,
                        });

                        word.text
                    });
                } else {
                    let range_index = match current_range_index {
                        Ok(index) => index + 1,
                        Err(index) => index,
                    };
                    let range_index = range_index % search_ranges.len();
                    let range = search_ranges[range_index];

                    buffer_view.cursors.mut_guard().add(Cursor {
                        anchor: range.from,
                        position: range.from,
                    });
                }
                self.movement_kind = CursorMovementKind::PositionAndAnchor;
            }
            Key::Char('a') => {
                fn balanced_brackets(
                    buffer: &BufferContent,
                    cursors: &mut [Cursor],
                    left: char,
                    right: char,
                ) {
                    for cursor in cursors {
                        let range = buffer.find_balanced_chars_at(cursor.position, left, right);
                        if let Some(range) = range {
                            cursor.anchor = range.from;
                            cursor.position = range.to;
                        }
                    }
                }

                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = &unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle)).content;
                let mut cursors = buffer_view.cursors.mut_guard();

                match keys.next() {
                    Key::None => return ModeOperation::Pending,
                    Key::Char('w') | Key::Char('W') => {
                        for cursor in &mut cursors[..] {
                            let word = buffer.word_at(cursor.position);
                            cursor.anchor = word.position;
                            cursor.position = word.end_position();
                        }
                    }
                    Key::Char('(') | Key::Char(')') => {
                        balanced_brackets(buffer, &mut cursors[..], '(', ')')
                    }
                    Key::Char('[') | Key::Char(']') => {
                        balanced_brackets(buffer, &mut cursors[..], '[', ']')
                    }
                    Key::Char('{') | Key::Char('}') => {
                        balanced_brackets(buffer, &mut cursors[..], '{', '}')
                    }
                    Key::Char('<') | Key::Char('>') => {
                        balanced_brackets(buffer, &mut cursors[..], '<', '>')
                    }
                    Key::Char('|') => balanced_brackets(buffer, &mut cursors[..], '|', '|'),
                    Key::Char('"') => balanced_brackets(buffer, &mut cursors[..], '"', '"'),
                    Key::Char('\'') => balanced_brackets(buffer, &mut cursors[..], '\'', '\''),
                    _ => (),
                }

                self.movement_kind = CursorMovementKind::PositionOnly;
            }
            Key::Char('A') => {
                fn balanced_brackets(
                    buffer: &BufferContent,
                    cursors: &mut [Cursor],
                    left: char,
                    right: char,
                ) {
                    for cursor in cursors {
                        let range = buffer.find_balanced_chars_at(cursor.position, left, right);
                        if let Some(range) = range {
                            cursor.anchor = BufferPosition::line_col(
                                range.from.line_index,
                                range.from.column_byte_index - left.len_utf8(),
                            );
                            cursor.position = BufferPosition::line_col(
                                range.to.line_index,
                                range.to.column_byte_index + right.len_utf8(),
                            );
                        }
                    }
                }

                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = &unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle)).content;
                let mut cursors = buffer_view.cursors.mut_guard();

                match keys.next() {
                    Key::None => return ModeOperation::Pending,
                    Key::Char('w') | Key::Char('W') => {
                        for cursor in &mut cursors[..] {
                            let (word, mut left_words, mut right_words) =
                                buffer.words_from(cursor.position);
                            cursor.anchor = match left_words.next() {
                                Some(word) if word.kind == WordKind::Whitespace => word.position,
                                _ => word.position,
                            };
                            cursor.position = match right_words.next() {
                                Some(word) if word.kind == WordKind::Whitespace => {
                                    word.end_position()
                                }
                                _ => word.end_position(),
                            };
                        }
                    }
                    Key::Char('(') | Key::Char(')') => {
                        balanced_brackets(buffer, &mut cursors[..], '(', ')')
                    }
                    Key::Char('[') | Key::Char(']') => {
                        balanced_brackets(buffer, &mut cursors[..], '[', ']')
                    }
                    Key::Char('{') | Key::Char('}') => {
                        balanced_brackets(buffer, &mut cursors[..], '{', '}')
                    }
                    Key::Char('<') | Key::Char('>') => {
                        balanced_brackets(buffer, &mut cursors[..], '<', '>')
                    }
                    Key::Char('|') => balanced_brackets(buffer, &mut cursors[..], '|', '|'),
                    Key::Char('"') => balanced_brackets(buffer, &mut cursors[..], '"', '"'),
                    Key::Char('\'') => balanced_brackets(buffer, &mut cursors[..], '\'', '\''),
                    _ => (),
                }

                self.movement_kind = CursorMovementKind::PositionOnly;
            }
            Key::Char('g') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                match keys.next() {
                    Key::None => return ModeOperation::Pending,
                    Key::Char('g') => {
                        return ModeOperation::EnterMode(Mode::Goto(Default::default()))
                    }
                    Key::Char('h') => buffer_view.move_cursors(
                        ctx.buffers,
                        CursorMovement::Home,
                        self.movement_kind,
                    ),
                    Key::Char('j') => buffer_view.move_cursors(
                        ctx.buffers,
                        CursorMovement::LastLine,
                        self.movement_kind,
                    ),
                    Key::Char('k') => buffer_view.move_cursors(
                        ctx.buffers,
                        CursorMovement::FirstLine,
                        self.movement_kind,
                    ),
                    Key::Char('l') => buffer_view.move_cursors(
                        ctx.buffers,
                        CursorMovement::End,
                        self.movement_kind,
                    ),
                    Key::Char('m') => {
                        let buffer =
                            &unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle)).content;
                        for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                            let position = cursor.position;
                            let (left, right) = match buffer.line_at(position.line_index).as_str()
                                [position.column_byte_index..]
                                .chars()
                                .next()
                            {
                                Some('(') | Some(')') => ('(', ')'),
                                Some('[') | Some(']') => ('[', ']'),
                                Some('{') | Some('}') => ('{', '}'),
                                Some('<') | Some('>') => ('<', '>'),
                                Some('|') => ('|', '|'),
                                Some('"') => ('"', '"'),
                                Some('\'') => ('\'', '\''),
                                _ => continue,
                            };

                            if let Some(range) =
                                buffer.find_balanced_chars_at(position, left, right)
                            {
                                let from = BufferPosition::line_col(
                                    range.from.line_index,
                                    range.from.column_byte_index - left.len_utf8(),
                                );
                                let to = range.to;

                                if position == from {
                                    cursor.position = to;
                                } else if position == to {
                                    cursor.position = from;
                                }

                                if let CursorMovementKind::PositionAndAnchor = self.movement_kind {
                                    cursor.anchor = cursor.position;
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
            Key::Char('f') => match keys.next() {
                Key::None => return ModeOperation::Pending,
                Key::Char(c) => {
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

                    for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                        cursor.position.column_byte_index = buffer
                            .content
                            .line_at(cursor.position.line_index)
                            .next_char_from(cursor.position.column_byte_index, c)
                            .unwrap_or(cursor.position.column_byte_index);

                        if let CursorMovementKind::PositionAndAnchor = self.movement_kind {
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

                    for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                        cursor.position.column_byte_index = buffer
                            .content
                            .line_at(cursor.position.line_index)
                            .previous_char_from(cursor.position.column_byte_index, c)
                            .unwrap_or(cursor.position.column_byte_index);

                        if let CursorMovementKind::PositionAndAnchor = self.movement_kind {
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

                    for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                        cursor.position.column_byte_index = match buffer
                            .content
                            .line_at(cursor.position.line_index)
                            .next_char_from(cursor.position.column_byte_index, c)
                        {
                            Some(i) => i.saturating_sub(1),
                            None => cursor.position.column_byte_index,
                        };

                        if let CursorMovementKind::PositionAndAnchor = self.movement_kind {
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

                    for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                        let line = buffer.content.line_at(cursor.position.line_index);
                        cursor.position.column_byte_index =
                            match line.previous_char_from(cursor.position.column_byte_index, c) {
                                Some(i) => line.as_str().len().min(i + 1),
                                None => cursor.position.column_byte_index,
                            };

                        if let CursorMovementKind::PositionAndAnchor = self.movement_kind {
                            cursor.anchor = cursor.position;
                        }
                    }
                }
                _ => (),
            },
            Key::Char('v') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = &unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle)).content;

                let last_line_index = buffer.line_count().saturating_sub(1);
                for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                    if cursor.anchor <= cursor.position {
                        cursor.anchor.column_byte_index = 0;
                        if cursor.position.line_index < last_line_index {
                            cursor.position.line_index += 1;
                            cursor.position.column_byte_index = 0;
                        } else {
                            cursor.position.column_byte_index =
                                buffer.line_at(cursor.position.line_index).as_str().len();
                        }
                    } else {
                        cursor.anchor.column_byte_index =
                            buffer.line_at(cursor.anchor.line_index).as_str().len();
                        if cursor.position.line_index > 0 {
                            cursor.position.line_index -= 1;
                            cursor.position.column_byte_index =
                                buffer.line_at(cursor.position.line_index).as_str().len();
                        } else {
                            cursor.position.column_byte_index = 0;
                        }
                    }
                }
                self.movement_kind = CursorMovementKind::PositionOnly;
            }
            Key::Char('V') => {
                let mut had_selection = false;
                for cursor in &mut unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                    .cursors
                    .mut_guard()[..]
                {
                    if cursor.anchor != cursor.position {
                        cursor.anchor = cursor.position;
                        had_selection = true;
                    }
                }

                self.movement_kind = if had_selection {
                    CursorMovementKind::PositionAndAnchor
                } else {
                    CursorMovementKind::PositionOnly
                };
            }
            Key::Char('z') => {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
                let focused_line_index = buffer_view.cursors.main_cursor().position.line_index;
                let client = unwrap_or_none!(ctx.clients.get_mut(ctx.target_client));
                let height = client.height as usize;

                match keys.next() {
                    Key::None => return ModeOperation::Pending,
                    Key::Char('z') => client.scroll = focused_line_index.saturating_sub(height / 2),
                    Key::Char('j') => client.scroll = focused_line_index.saturating_sub(height),
                    Key::Char('k') => client.scroll = focused_line_index,
                    _ => (),
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
                self.movement_kind = CursorMovementKind::PositionAndAnchor;
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
            Key::Char('x') => match keys.next() {
                Key::None => return ModeOperation::Pending,
                Key::Char('x') => {
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                    let buffer =
                        &unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle)).content;

                    let mut cursors = buffer_view.cursors.mut_guard();
                    let cursor_count = cursors[..].len();

                    for i in 0..cursor_count {
                        let cursor = &mut cursors[i];
                        if cursor.anchor.line_index == cursor.position.line_index {
                            continue;
                        }

                        let range = BufferRange::between(cursor.anchor, cursor.position);
                        cursor.anchor = range.from;
                        cursor.position = BufferPosition::line_col(
                            range.from.line_index,
                            buffer.line_at(range.from.line_index).as_str().len(),
                        );

                        for line_index in (range.from.line_index + 1)..range.to.line_index {
                            let line_len = buffer.line_at(line_index).as_str().len();
                            cursors.add(Cursor {
                                anchor: BufferPosition::line_col(line_index, 0),
                                position: BufferPosition::line_col(line_index, line_len),
                            });
                        }

                        cursors.add(Cursor {
                            anchor: BufferPosition::line_col(range.to.line_index, 0),
                            position: range.to,
                        });
                    }
                    self.movement_kind = CursorMovementKind::PositionOnly;
                }
                Key::Char('m') => {
                    let cursors = &mut unwrap_or_none!(ctx.buffer_views.get_mut(handle)).cursors;
                    let main_cursor = *cursors.main_cursor();
                    let mut cursors = cursors.mut_guard();
                    cursors.clear();
                    cursors.add(main_cursor);
                    self.movement_kind = CursorMovementKind::PositionAndAnchor;
                }
                Key::Char('v') => {
                    for cursor in &mut unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                        .cursors
                        .mut_guard()[..]
                    {
                        cursor.anchor = cursor.position;
                    }
                    self.movement_kind = CursorMovementKind::PositionAndAnchor;
                }
                Key::Char('o') => {
                    for cursor in &mut unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                        .cursors
                        .mut_guard()[..]
                    {
                        std::mem::swap(&mut cursor.anchor, &mut cursor.position);
                    }
                }
                Key::Char('n') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                    .cursors
                    .next_main_cursor(),
                Key::Char('p') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                    .cursors
                    .previous_main_cursor(),
                Key::Char('/') => {
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));
                    let search_ranges = buffer.search_ranges();
                    if search_ranges.is_empty() {
                        return ModeOperation::None;
                    }

                    let mut cursors = buffer_view.cursors.mut_guard();
                    let cursor_count = cursors[..].len();

                    let mut has_selection = false;
                    for cursor in &cursors[..] {
                        if cursor.anchor != cursor.position {
                            has_selection = true;
                            break;
                        }
                    }

                    if has_selection {
                        for i in 0..cursor_count {
                            let cursor = &mut cursors[i];
                            let cursor_range = BufferRange::between(cursor.anchor, cursor.position);

                            let mut search_ranges = search_ranges.iter().filter(|r| {
                                r.from >= cursor_range.from && r.from <= cursor_range.to
                                    || r.to >= cursor_range.from && r.to <= cursor_range.to
                            });

                            if let Some(range) = search_ranges.next() {
                                cursor.anchor = range.from;
                                cursor.position = BufferPosition::line_col(
                                    range.to.line_index,
                                    range.to.column_byte_index + 1,
                                );
                            }

                            for range in search_ranges {
                                cursors.add(Cursor {
                                    anchor: range.from,
                                    position: BufferPosition::line_col(
                                        range.to.line_index,
                                        range.to.column_byte_index + 1,
                                    ),
                                });
                            }
                        }
                    } else {
                        cursors.clear();
                        for range in search_ranges {
                            cursors.add(Cursor {
                                anchor: range.from,
                                position: BufferPosition::line_col(
                                    range.to.line_index,
                                    range.to.column_byte_index + 1,
                                ),
                            });
                        }
                    }

                    self.movement_kind = CursorMovementKind::PositionOnly;
                }
                _ => (),
            },
            Key::Char('/') => return ModeOperation::EnterMode(Mode::Search(Default::default())),
            Key::Char('y') => {
                if let Ok(mut clipboard) = ClipboardContext::new() {
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
                    let mut text = String::new();
                    buffer_view.get_selection_text(ctx.buffers, &mut text);
                    if !text.is_empty() {
                        let _ = clipboard.set_contents(text);
                    }
                }
                self.movement_kind = CursorMovementKind::PositionAndAnchor;
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
                self.movement_kind = CursorMovementKind::PositionAndAnchor;
            }
            Key::Char('u') => {
                ctx.buffer_views
                    .undo(ctx.buffers, &ctx.config.syntaxes, handle);
                self.movement_kind = CursorMovementKind::PositionAndAnchor;
            }
            Key::Char('U') => {
                ctx.buffer_views
                    .redo(ctx.buffers, &ctx.config.syntaxes, handle);
                self.movement_kind = CursorMovementKind::PositionAndAnchor;
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
