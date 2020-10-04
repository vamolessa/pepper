use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer::BufferContent,
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{CursorMovement, CursorMovementKind},
    client_event::Key,
    cursor::Cursor,
    editor::KeysIterator,
    editor::StatusMessageKind,
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    navigation_history::{NavigationDirection, NavigationHistory},
    word_database::WordKind,
};

enum CharJump {
    None,
    Inclusive(char),
    Exclusive(char),
}

pub struct State {
    movement_kind: CursorMovementKind,
    last_char_jump: CharJump,
}

impl Default for State {
    fn default() -> Self {
        Self {
            movement_kind: CursorMovementKind::PositionAndAnchor,
            last_char_jump: CharJump::None,
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
                NavigationHistory::save_client_snapshot(
                    ctx.clients,
                    ctx.buffer_views,
                    ctx.target_client,
                );
                move_to_search_match(self, ctx, |len, r| {
                    let index = match r {
                        Ok(index) => index + 1,
                        Err(index) => index,
                    };
                    index % len
                });
            }
            Key::Char('p') => {
                NavigationHistory::save_client_snapshot(
                    ctx.clients,
                    ctx.buffer_views,
                    ctx.target_client,
                );
                move_to_search_match(self, ctx, |len, r| {
                    let index = match r {
                        Ok(index) => index,
                        Err(index) => index,
                    };
                    (index + len - 1) % len
                });
            }
            Key::Char('N') => {
                search_word_or_move_to_it(self, ctx, |len, r| {
                    let index = match r {
                        Ok(index) => index + 1,
                        Err(index) => index,
                    };
                    index % len
                });
            }
            Key::Char('P') => {
                search_word_or_move_to_it(self, ctx, |len, r| {
                    let index = match r {
                        Ok(index) => index,
                        Err(index) => index,
                    };
                    (index + len - 1) % len
                });
            }
            Key::Ctrl('n') => {
                NavigationHistory::move_in_history(
                    ctx.clients,
                    ctx.buffer_views,
                    ctx.target_client,
                    NavigationDirection::Forward,
                );
                self.movement_kind = CursorMovementKind::PositionAndAnchor;
            }
            Key::Ctrl('p') => {
                NavigationHistory::move_in_history(
                    ctx.clients,
                    ctx.buffer_views,
                    ctx.target_client,
                    NavigationDirection::Backward,
                );
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
                Key::Char(ch) => {
                    self.last_char_jump = CharJump::Inclusive(ch);
                    find_char(self, ctx, true);
                }
                _ => (),
            },
            Key::Char('F') => match keys.next() {
                Key::None => return ModeOperation::Pending,
                Key::Char(ch) => {
                    self.last_char_jump = CharJump::Inclusive(ch);
                    find_char(self, ctx, false);
                }
                _ => (),
            },
            Key::Char('t') => match keys.next() {
                Key::None => return ModeOperation::Pending,
                Key::Char(ch) => {
                    self.last_char_jump = CharJump::Exclusive(ch);
                    find_char(self, ctx, true);
                }
                _ => (),
            },
            Key::Char('T') => match keys.next() {
                Key::None => return ModeOperation::Pending,
                Key::Char(ch) => {
                    self.last_char_jump = CharJump::Exclusive(ch);
                    find_char(self, ctx, false);
                }
                _ => (),
            },
            Key::Char(';') => find_char(self, ctx, true),
            Key::Char(',') => find_char(self, ctx, false),
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
                Key::Char('c') => {
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
                Key::Char('n') => {
                    let cursors = &mut unwrap_or_none!(ctx.buffer_views.get_mut(handle)).cursors;
                    let index = cursors.main_cursor_index();
                    let mut cursors = cursors.mut_guard();
                    let cursor_count = cursors[..].len();
                    cursors.set_main_cursor_index((index + 1) % cursor_count);
                }
                Key::Char('p') => {
                    let cursors = &mut unwrap_or_none!(ctx.buffer_views.get_mut(handle)).cursors;
                    let index = cursors.main_cursor_index();
                    let mut cursors = cursors.mut_guard();
                    let cursor_count = cursors[..].len();
                    cursors.set_main_cursor_index((index + cursor_count - 1) % cursor_count);
                }
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

fn find_char(state: &State, ctx: &mut ModeContext, forward: bool) {
    let ch;
    let next_ch;
    match state.last_char_jump {
        CharJump::None => return,
        CharJump::Inclusive(c) => {
            ch = c;
            next_ch = forward;
        }
        CharJump::Exclusive(c) => {
            ch = c;
            next_ch = !forward;
        }
    };

    let handle = unwrap_or_return!(ctx.current_buffer_view_handle());
    let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
    let buffer = unwrap_or_return!(ctx.buffers.get(buffer_view.buffer_handle));

    for cursor in &mut buffer_view.cursors.mut_guard()[..] {
        let mut chars = buffer
            .content
            .line_at(cursor.position.line_index)
            .chars_from(cursor.position.column_byte_index);
        let element = match forward {
            false => chars.0.find(|(_, c)| *c == ch),
            true => chars.1.find(|(_, c)| *c == ch),
        };
        if let Some((i, c)) = element {
            cursor.position.column_byte_index = i;
            if next_ch {
                cursor.position.column_byte_index += c.len_utf8();
            }

            if let CursorMovementKind::PositionAndAnchor = state.movement_kind {
                cursor.anchor = cursor.position;
            }
        }
    }
}

fn move_to_search_match(
    state: &State,
    ctx: &mut ModeContext,
    index_selector: fn(usize, Result<usize, usize>) -> usize,
) {
    let handle = unwrap_or_return!(ctx.current_buffer_view_handle());
    let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
    let buffer = unwrap_or_return!(ctx.buffers.get_mut(buffer_view.buffer_handle));

    let search_ranges = buffer.search_ranges();
    if search_ranges.is_empty() {
        ctx.status_message
            .write_str(StatusMessageKind::Error, "no search result");
        return;
    }

    let cursors = &mut buffer_view.cursors;
    let main_position = cursors.main_cursor().position;

    let search_result = search_ranges.binary_search_by_key(&main_position, |r| r.from);
    let next_index = index_selector(search_ranges.len(), search_result);

    let mut cursors = cursors.mut_guard();
    let main_cursor = cursors.main_cursor();
    main_cursor.position = search_ranges[next_index].from;

    if let CursorMovementKind::PositionAndAnchor = state.movement_kind {
        main_cursor.anchor = main_cursor.position;
    }
}

fn search_word_or_move_to_it(
    state: &mut State,
    ctx: &mut ModeContext,
    index_selector: fn(usize, Result<usize, usize>) -> usize,
) {
    let handle = unwrap_or_return!(ctx.current_buffer_view_handle());
    let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
    let buffer = unwrap_or_return!(ctx.buffers.get_mut(buffer_view.buffer_handle));

    let main_position = buffer_view.cursors.main_cursor().position;
    let search_ranges = buffer.search_ranges();
    let current_range_index = search_ranges.binary_search_by_key(&main_position, |r| r.from);

    if search_ranges.is_empty()
        || current_range_index
            .map(|i| {
                let word = buffer.content.word_at(main_position);
                let word_range = BufferRange::between(word.position, word.end_position());
                search_ranges[i] != word_range
            })
            .unwrap_or(true)
    {
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
        let range_index = index_selector(search_ranges.len(), current_range_index);
        let range = search_ranges[range_index];

        buffer_view.cursors.mut_guard().add(Cursor {
            anchor: range.from,
            position: range.from,
        });
    }

    state.movement_kind = CursorMovementKind::PositionAndAnchor;
}
