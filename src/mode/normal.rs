use std::cmp::Ordering;

use crate::platform::Key;

use crate::{
    buffer::BufferContent,
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    client::{Client, ClientManager, TargetClient},
    cursor::Cursor,
    editor::{Editor, KeysIterator, StatusMessageKind},
    lsp::LspDiagnostic,
    mode::{picker, read_line, Mode, ModeKind, ModeOperation, ModeState},
    navigation_history::{NavigationDirection, NavigationHistory},
    register::{RegisterKey, AUTO_MACRO_REGISTER, SEARCH_REGISTER},
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
    is_recording_auto_macro: bool,
    pub count: u32,
}

impl State {
    fn on_movement_keys(editor: &mut Editor, keys: &KeysIterator, from_index: usize) {
        let this = &mut editor.mode.normal_state;
        match this.movement_kind {
            CursorMovementKind::PositionAndAnchor => this.is_recording_auto_macro = false,
            CursorMovementKind::PositionOnly => {
                if !this.is_recording_auto_macro {
                    editor.registers.set(AUTO_MACRO_REGISTER, "");
                }
                this.is_recording_auto_macro = true;

                if editor.registers.get(AUTO_MACRO_REGISTER).is_empty() && this.count > 0 {
                    editor
                        .registers
                        .append_fmt(AUTO_MACRO_REGISTER, format_args!("{}", this.count));
                }

                for key in &editor.buffered_keys.as_slice()[from_index..keys.index()] {
                    editor
                        .registers
                        .append_fmt(AUTO_MACRO_REGISTER, format_args!("{}", key));
                }
            }
        }
    }

    fn on_edit_keys(editor: &mut Editor, keys: &KeysIterator, from_index: usize) {
        let this = &mut editor.mode.normal_state;
        if !this.is_recording_auto_macro {
            editor.registers.set(AUTO_MACRO_REGISTER, "");
        }
        this.is_recording_auto_macro = false;

        if editor.registers.get(AUTO_MACRO_REGISTER).is_empty() && this.count > 0 {
            editor
                .registers
                .append_fmt(AUTO_MACRO_REGISTER, format_args!("{}", this.count));
        }

        for key in &editor.buffered_keys.as_slice()[from_index..keys.index()] {
            editor
                .registers
                .append_fmt(AUTO_MACRO_REGISTER, format_args!("{}", key));
        }
    }

    fn on_event_no_buffer(
        editor: &mut Editor,
        clients: &mut ClientManager,
        target: TargetClient,
        keys: &mut KeysIterator,
    ) -> Option<ModeOperation> {
        let this = &mut editor.mode.normal_state;
        this.is_recording_auto_macro = false;
        match keys.next(&editor.buffered_keys) {
            Key::Char('q') => match editor.recording_macro.take() {
                Some(_) => editor.recording_macro = None,
                None => match keys.next(&editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char(c) => {
                        if let Some(key) = RegisterKey::from_char(c) {
                            editor.registers.set(key, "");
                            editor.recording_macro = Some(key);
                        }
                    }
                    _ => (),
                },
            },
            Key::Char('Q') => {
                editor.recording_macro = None;
                match keys.next(&editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char(c) => {
                        // TODO: try just moving the recorded keys to the key queue register
                        if let Some(key) = RegisterKey::from_char(c.to_ascii_lowercase()) {
                            return Some(ModeOperation::ExecuteMacro(key));
                        }
                    }
                    _ => (),
                }
            }
            Key::F(4) => return Some(ModeOperation::Quit), // TODO: hack to close the editor until we get command mode working
            // TODO: implement command mode
            //Key::Char(':') => Mode::change_to(editor, clients, target, ModeKind::Script),
            Key::Char('g') => match keys.next(&editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char('b') => picker::buffer::enter_mode(editor, clients, target),
                Key::Char('a') => {
                    let previous_buffer_view_handle =
                        clients.get(target)?.previous_buffer_view_handle();
                    clients.set_buffer_view_handle(editor, target, previous_buffer_view_handle);
                }
                _ => (),
            },
            Key::Char(c) => {
                if let Some(n) = c.to_digit(10) {
                    this.count = this.count.saturating_mul(10).saturating_add(n);
                }
            }
            _ => (),
        }

        None
    }

    fn on_client_keys_with_buffer_view(
        editor: &mut Editor,
        clients: &mut ClientManager,
        target: TargetClient,
        keys: &mut KeysIterator,
        handle: BufferViewHandle,
    ) -> Option<ModeOperation> {
        let this = &mut editor.mode.normal_state;
        let keys_from_index = keys.index();
        match keys.next(&editor.buffered_keys) {
            Key::Char('h') => editor.buffer_views.get_mut(handle)?.move_cursors(
                &editor.buffers,
                CursorMovement::ColumnsBackward(this.count.max(1) as _),
                this.movement_kind,
            ),
            Key::Char('j') => editor.buffer_views.get_mut(handle)?.move_cursors(
                &editor.buffers,
                CursorMovement::LinesForward(this.count.max(1) as _),
                this.movement_kind,
            ),
            Key::Char('k') => editor.buffer_views.get_mut(handle)?.move_cursors(
                &editor.buffers,
                CursorMovement::LinesBackward(this.count.max(1) as _),
                this.movement_kind,
            ),
            Key::Char('l') => editor.buffer_views.get_mut(handle)?.move_cursors(
                &editor.buffers,
                CursorMovement::ColumnsForward(this.count.max(1) as _),
                this.movement_kind,
            ),
            Key::Char('w') => editor.buffer_views.get_mut(handle)?.move_cursors(
                &editor.buffers,
                CursorMovement::WordsForward(this.count.max(1) as _),
                this.movement_kind,
            ),
            Key::Char('b') => editor.buffer_views.get_mut(handle)?.move_cursors(
                &editor.buffers,
                CursorMovement::WordsBackward(this.count.max(1) as _),
                this.movement_kind,
            ),
            Key::Char('n') => {
                let count = this.count.max(1);
                move_to_search_match(editor, clients, target, |len, r| {
                    let index = match r {
                        Ok(index) => index + count as usize,
                        Err(index) => index + count as usize - 1,
                    };
                    index % len
                });
            }
            Key::Char('p') => {
                let count = this.count.max(1) as usize;
                move_to_search_match(editor, clients, target, |len, r| {
                    let index = match r {
                        Ok(index) => index,
                        Err(index) => index,
                    };
                    (index + len - count % len) % len
                });
            }
            Key::Char('N') => {
                search_word_or_move_to_it(editor, clients, target, |len, r| {
                    let index = match r {
                        Ok(index) => index + 1,
                        Err(index) => index,
                    };
                    index % len
                });
            }
            Key::Char('P') => {
                search_word_or_move_to_it(editor, clients, target, |len, r| {
                    let index = match r {
                        Ok(index) => index,
                        Err(index) => index,
                    };
                    (index + len - 1) % len
                });
            }
            Key::Ctrl('n') => {
                this.movement_kind = CursorMovementKind::PositionAndAnchor;
                NavigationHistory::move_in_history(
                    editor,
                    clients,
                    target,
                    NavigationDirection::Forward,
                );
            }
            Key::Ctrl('p') => {
                this.movement_kind = CursorMovementKind::PositionAndAnchor;
                NavigationHistory::move_in_history(
                    editor,
                    clients,
                    target,
                    NavigationDirection::Backward,
                );
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

                fn delimiter_pair(buffer: &BufferContent, cursors: &mut [Cursor], delimiter: char) {
                    for cursor in cursors {
                        let range = buffer.find_delimiter_pair_at(cursor.position, delimiter);
                        if let Some(range) = range {
                            cursor.anchor = range.from;
                            cursor.position = range.to;
                        }
                    }
                }

                let buffer_view = editor.buffer_views.get_mut(handle)?;
                let buffer = editor.buffers.get(buffer_view.buffer_handle)?.content();
                let mut cursors = buffer_view.cursors.mut_guard();

                match keys.next(&editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char('w') | Key::Char('W') => {
                        for cursor in &mut cursors[..] {
                            let word = buffer.word_at(cursor.position);
                            cursor.anchor = word.position;
                            cursor.position = word.end_position();
                        }
                    }
                    Key::Char('a') | Key::Char('A') => {
                        let last_line_index = buffer.line_count() - 1;
                        let last_line_len = buffer.line_at(last_line_index).as_str().len();

                        cursors.clear();
                        cursors.add(Cursor {
                            anchor: BufferPosition::line_col(0, 0),
                            position: BufferPosition::line_col(last_line_index, last_line_len),
                        });
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
                    Key::Char('|') => delimiter_pair(buffer, &mut cursors[..], '|'),
                    Key::Char('"') => delimiter_pair(buffer, &mut cursors[..], '"'),
                    Key::Char('\'') => delimiter_pair(buffer, &mut cursors[..], '\''),
                    _ => (),
                }

                this.movement_kind = CursorMovementKind::PositionOnly;
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

                fn delimiter_pair(buffer: &BufferContent, cursors: &mut [Cursor], delimiter: char) {
                    for cursor in cursors {
                        let range = buffer.find_delimiter_pair_at(cursor.position, delimiter);
                        if let Some(range) = range {
                            cursor.anchor = BufferPosition::line_col(
                                range.from.line_index,
                                range.from.column_byte_index - delimiter.len_utf8(),
                            );
                            cursor.position = BufferPosition::line_col(
                                range.to.line_index,
                                range.to.column_byte_index + delimiter.len_utf8(),
                            );
                        }
                    }
                }

                let buffer_view = editor.buffer_views.get_mut(handle)?;
                let buffer = editor.buffers.get(buffer_view.buffer_handle)?.content();
                let mut cursors = buffer_view.cursors.mut_guard();

                match keys.next(&editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
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
                    Key::Char('a') | Key::Char('A') => {
                        let last_line_index = buffer.line_count() - 1;
                        let last_line_len = buffer.line_at(last_line_index).as_str().len();

                        cursors.clear();
                        cursors.add(Cursor {
                            anchor: BufferPosition::line_col(0, 0),
                            position: BufferPosition::line_col(last_line_index, last_line_len),
                        });
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
                    Key::Char('|') => delimiter_pair(buffer, &mut cursors[..], '|'),
                    Key::Char('"') => delimiter_pair(buffer, &mut cursors[..], '"'),
                    Key::Char('\'') => delimiter_pair(buffer, &mut cursors[..], '\''),
                    _ => (),
                }

                this.movement_kind = CursorMovementKind::PositionOnly;
            }
            Key::Char('g') => {
                let buffer_view = editor.buffer_views.get_mut(handle)?;
                match keys.next(&editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char('g') => read_line::goto::enter_mode(editor, clients, target),
                    Key::Char('h') => buffer_view.move_cursors(
                        &editor.buffers,
                        CursorMovement::Home,
                        this.movement_kind,
                    ),
                    Key::Char('j') => buffer_view.move_cursors(
                        &editor.buffers,
                        CursorMovement::LastLine,
                        this.movement_kind,
                    ),
                    Key::Char('k') => buffer_view.move_cursors(
                        &editor.buffers,
                        CursorMovement::FirstLine,
                        this.movement_kind,
                    ),
                    Key::Char('l') => buffer_view.move_cursors(
                        &editor.buffers,
                        CursorMovement::End,
                        this.movement_kind,
                    ),
                    Key::Char('i') => buffer_view.move_cursors(
                        &editor.buffers,
                        CursorMovement::HomeNonWhitespace,
                        this.movement_kind,
                    ),
                    Key::Char('m') => {
                        let buffer = editor.buffers.get(buffer_view.buffer_handle)?.content();
                        for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                            let mut position = cursor.position;

                            let line = buffer.line_at(position.line_index).as_str();
                            let cursor_char = if position.column_byte_index < line.len() {
                                match line[position.column_byte_index..].chars().next() {
                                    Some(c) => c,
                                    None => continue,
                                }
                            } else {
                                match line.char_indices().next_back() {
                                    Some((i, c)) => {
                                        position.column_byte_index = i;
                                        c
                                    }
                                    None => continue,
                                }
                            };

                            let range = match cursor_char {
                                '(' | ')' => buffer.find_balanced_chars_at(position, '(', ')'),
                                '[' | ']' => buffer.find_balanced_chars_at(position, '[', ']'),
                                '{' | '}' => buffer.find_balanced_chars_at(position, '{', '}'),
                                '<' | '>' => buffer.find_balanced_chars_at(position, '<', '>'),
                                d @ '|' | d @ '"' | d @ '\'' => {
                                    buffer.find_delimiter_pair_at(position, d)
                                }
                                _ => continue,
                            };

                            if let Some(range) = range {
                                let from = BufferPosition::line_col(
                                    range.from.line_index,
                                    range.from.column_byte_index - 1,
                                );
                                let to = range.to;

                                if position == from {
                                    cursor.position = to;
                                } else if position == to {
                                    cursor.position = from;
                                }

                                if let CursorMovementKind::PositionAndAnchor = this.movement_kind {
                                    cursor.anchor = cursor.position;
                                }
                            }
                        }
                    }
                    _ => {
                        keys.put_back();
                        keys.put_back();
                        return Self::on_event_no_buffer(editor, clients, target, keys);
                    }
                }
            }
            Key::Char('f') => match keys.next(&editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char(ch) => {
                    this.last_char_jump = CharJump::Inclusive(ch);
                    find_char(editor, clients, target, true);
                }
                _ => (),
            },
            Key::Char('F') => match keys.next(&editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char(ch) => {
                    this.last_char_jump = CharJump::Inclusive(ch);
                    find_char(editor, clients, target, false);
                }
                _ => (),
            },
            Key::Char('t') => match keys.next(&editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char(ch) => {
                    this.last_char_jump = CharJump::Exclusive(ch);
                    find_char(editor, clients, target, true);
                }
                _ => (),
            },
            Key::Char('T') => match keys.next(&editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char(ch) => {
                    this.last_char_jump = CharJump::Exclusive(ch);
                    find_char(editor, clients, target, false);
                }
                _ => (),
            },
            Key::Char(';') => {
                find_char(editor, clients, target, true);
            }
            Key::Char(',') => {
                find_char(editor, clients, target, false);
            }
            Key::Char('v') => {
                let mut had_selection = false;
                for cursor in &mut editor.buffer_views.get_mut(handle)?.cursors.mut_guard()[..] {
                    if cursor.anchor != cursor.position {
                        cursor.anchor = cursor.position;
                        had_selection = true;
                    }
                }

                this.movement_kind = if had_selection {
                    CursorMovementKind::PositionAndAnchor
                } else {
                    CursorMovementKind::PositionOnly
                };
            }
            Key::Char('V') => {
                let buffer_view = editor.buffer_views.get_mut(handle)?;
                let buffer = editor.buffers.get(buffer_view.buffer_handle)?.content();

                let count = this.count.max(1);
                let last_line_index = buffer.line_count().saturating_sub(1);
                for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                    if cursor.anchor <= cursor.position {
                        cursor.anchor.column_byte_index = 0;
                        cursor.position.line_index += count as usize;
                        if cursor.position.line_index <= last_line_index {
                            cursor.position.column_byte_index = 0;
                        } else {
                            cursor.position.line_index = last_line_index;
                            cursor.position.column_byte_index =
                                buffer.line_at(cursor.position.line_index).as_str().len();
                        }
                    } else {
                        cursor.anchor.column_byte_index =
                            buffer.line_at(cursor.anchor.line_index).as_str().len();
                        if cursor.position.line_index >= count as usize {
                            cursor.position.line_index -= count as usize;
                            cursor.position.column_byte_index =
                                buffer.line_at(cursor.position.line_index).as_str().len();
                        } else {
                            cursor.position.line_index = 0;
                            cursor.position.column_byte_index = 0;
                        }
                    }
                }
                this.movement_kind = CursorMovementKind::PositionOnly;
            }
            Key::Char('z') => {
                let buffer_view = editor.buffer_views.get(handle)?;
                let focused_line_index = buffer_view.cursors.main_cursor().position.line_index;
                let client = clients.get_mut(target)?;
                let height = client.height as usize;

                match keys.next(&editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char('z') => client.scroll = focused_line_index.saturating_sub(height / 2),
                    Key::Char('j') => client.scroll = focused_line_index.saturating_sub(height),
                    Key::Char('k') => client.scroll = focused_line_index,
                    _ => (),
                }
            }
            Key::Ctrl('d') => {
                let half_height = clients.get(target).map(|c| c.height).unwrap_or(0) / 2;
                editor.buffer_views.get_mut(handle)?.move_cursors(
                    &editor.buffers,
                    CursorMovement::LinesForward(half_height as _),
                    this.movement_kind,
                );
            }
            Key::Ctrl('u') => {
                let half_height = clients.get(target).map(|c| c.height).unwrap_or(0) / 2;
                editor.buffer_views.get_mut(handle)?.move_cursors(
                    &editor.buffers,
                    CursorMovement::LinesBackward(half_height as _),
                    this.movement_kind,
                );
            }
            Key::Char('d') => {
                editor.buffer_views.delete_in_cursor_ranges(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    handle,
                    &mut editor.events,
                );
                let buffer_view = editor.buffer_views.get(handle)?;
                editor
                    .buffers
                    .get_mut(buffer_view.buffer_handle)?
                    .commit_edits();
                this.movement_kind = CursorMovementKind::PositionAndAnchor;
                Self::on_edit_keys(editor, keys, keys_from_index);
                return None;
            }
            Key::Char('i') => {
                editor.buffer_views.delete_in_cursor_ranges(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    handle,
                    &mut editor.events,
                );

                Self::on_edit_keys(editor, keys, keys_from_index);

                Mode::change_to(editor, clients, target, ModeKind::Insert);
                return None;
            }
            Key::Char('<') => {
                let buffer_view = editor.buffer_views.get(handle)?;
                let cursor_count = buffer_view.cursors[..].len();
                let buffer_handle = buffer_view.buffer_handle;
                let count = this.count.max(1);

                for i in 0..cursor_count {
                    let range = editor.buffer_views.get(handle)?.cursors[i].as_range();
                    for line_index in range.from.line_index..=range.to.line_index {
                        let buffer = editor.buffers.get(buffer_handle)?;
                        let line = buffer.content().line_at(line_index).as_str();
                        let mut indentation_column_index = 0;

                        for _ in 0..count {
                            let mut chars = line[indentation_column_index..].char_indices();
                            indentation_column_index += match chars.next() {
                                Some((i, c @ '\t')) => i + c.len_utf8(),
                                Some((i, c @ ' ')) => {
                                    match chars
                                        .take(editor.config.values.tab_size.get() as usize - 1)
                                        .take_while(|(_, c)| *c == ' ')
                                        .last()
                                    {
                                        Some((i, _)) => i + c.len_utf8(),
                                        None => i + c.len_utf8(),
                                    }
                                }
                                _ => break,
                            };
                        }
                        let range = BufferRange::between(
                            BufferPosition::line_col(line_index, 0),
                            BufferPosition::line_col(line_index, indentation_column_index),
                        );
                        editor.buffer_views.delete_in_range(
                            &mut editor.buffers,
                            &mut editor.word_database,
                            handle,
                            range,
                            &mut editor.events,
                        );
                    }
                }
                let buffer_view = editor.buffer_views.get(handle)?;
                editor
                    .buffers
                    .get_mut(buffer_view.buffer_handle)?
                    .commit_edits();

                Self::on_edit_keys(editor, keys, keys_from_index);
                return None;
            }
            Key::Char('>') => {
                let cursor_count = editor.buffer_views.get(handle)?.cursors[..].len();
                let count;
                let fill;
                if editor.config.values.indent_with_tabs {
                    count = this.count.max(1) as usize;
                    fill = b'\t';
                } else {
                    count =
                        this.count.max(1) as usize * editor.config.values.tab_size.get() as usize;
                    fill = b' ';
                }

                let buf = [fill; 128];
                let len = buf.len().min(count);
                let indentation = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };

                for i in 0..cursor_count {
                    let range = editor.buffer_views.get(handle)?.cursors[i].as_range();
                    for line_index in range.from.line_index..=range.to.line_index {
                        editor.buffer_views.insert_text_at_position(
                            &mut editor.buffers,
                            &mut editor.word_database,
                            handle,
                            BufferPosition::line_col(line_index, 0),
                            indentation,
                            &mut editor.events,
                        );
                    }
                }
                let buffer_view = editor.buffer_views.get(handle)?;
                editor
                    .buffers
                    .get_mut(buffer_view.buffer_handle)?
                    .commit_edits();

                Self::on_edit_keys(editor, keys, keys_from_index);
                return None;
            }
            Key::Char('c') | Key::Char('C') => match keys.next(&editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char('c') => {
                    let buffer_view = editor.buffer_views.get_mut(handle)?;
                    let buffer = editor.buffers.get(buffer_view.buffer_handle)?.content();

                    let mut cursors = buffer_view.cursors.mut_guard();
                    let cursor_count = cursors[..].len();

                    for i in 0..cursor_count {
                        let cursor = &mut cursors[i];
                        if cursor.anchor.line_index == cursor.position.line_index {
                            continue;
                        }

                        let range = BufferRange::between(cursor.anchor, cursor.position);
                        if range.to == cursor.position {
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
                        } else {
                            cursor.anchor = range.to;
                            cursor.position = BufferPosition::line_col(range.to.line_index, 0);

                            for line_index in (range.from.line_index + 1)..range.to.line_index {
                                let line_len = buffer.line_at(line_index).as_str().len();
                                cursors.add(Cursor {
                                    anchor: BufferPosition::line_col(line_index, line_len),
                                    position: BufferPosition::line_col(line_index, 0),
                                });
                            }

                            cursors.add(Cursor {
                                anchor: BufferPosition::line_col(
                                    range.from.line_index,
                                    buffer.line_at(range.from.line_index).as_str().len(),
                                ),
                                position: range.from,
                            });
                        }
                    }
                    this.movement_kind = CursorMovementKind::PositionOnly;
                }
                Key::Char('d') => {
                    let cursors = &mut editor.buffer_views.get_mut(handle)?.cursors;
                    let main_cursor = *cursors.main_cursor();
                    let mut cursors = cursors.mut_guard();
                    cursors.clear();
                    cursors.add(main_cursor);
                    this.movement_kind = CursorMovementKind::PositionAndAnchor;
                }
                Key::Char('v') => {
                    this.movement_kind = CursorMovementKind::PositionOnly;
                }
                Key::Char('V') => {
                    for cursor in &mut editor.buffer_views.get_mut(handle)?.cursors.mut_guard()[..]
                    {
                        cursor.anchor = cursor.position;
                    }
                    this.movement_kind = CursorMovementKind::PositionAndAnchor;
                }
                Key::Char('o') => {
                    for cursor in &mut editor.buffer_views.get_mut(handle)?.cursors.mut_guard()[..]
                    {
                        std::mem::swap(&mut cursor.anchor, &mut cursor.position);
                    }
                }
                Key::Char('j') => {
                    let buffer_view = editor.buffer_views.get_mut(handle)?;
                    let buffer = editor.buffers.get(buffer_view.buffer_handle)?;
                    let mut cursors = buffer_view.cursors.mut_guard();

                    if let Some(cursor) = cursors[..].last() {
                        let mut position = cursor.as_range().to;

                        for _ in 0..this.count.max(1) {
                            position.line_index += 1;
                            position = buffer.content().saturate_position(position);

                            cursors.add(Cursor {
                                anchor: position,
                                position,
                            });
                        }
                    }
                }
                Key::Char('k') => {
                    let buffer_view = editor.buffer_views.get_mut(handle)?;
                    let buffer = editor.buffers.get(buffer_view.buffer_handle)?;
                    let mut cursors = buffer_view.cursors.mut_guard();

                    if let Some(cursor) = cursors[..].first() {
                        let mut position = cursor.as_range().from;

                        for _ in 0..this.count.max(1) {
                            position.line_index = position.line_index.saturating_sub(1);
                            position = buffer.content().saturate_position(position);

                            cursors.add(Cursor {
                                anchor: position,
                                position,
                            });
                        }
                    }
                }
                Key::Char('n') => {
                    let cursors = &mut editor.buffer_views.get_mut(handle)?.cursors;
                    let index = cursors.main_cursor_index();
                    let mut cursors = cursors.mut_guard();
                    let cursor_count = cursors[..].len();
                    let offset = this.count.max(1) as usize;
                    cursors.set_main_cursor_index((index + offset) % cursor_count);
                }
                Key::Char('p') => {
                    let cursors = &mut editor.buffer_views.get_mut(handle)?.cursors;
                    let index = cursors.main_cursor_index();
                    let mut cursors = cursors.mut_guard();
                    let cursor_count = cursors[..].len();
                    let offset = this.count.max(1) as usize % cursor_count;
                    cursors.set_main_cursor_index((index + cursor_count - offset) % cursor_count);
                }
                Key::Char('f') => {
                    read_line::filter_cursors::enter_filter_mode(editor, clients, target)
                }
                Key::Char('F') => {
                    read_line::filter_cursors::enter_except_mode(editor, clients, target)
                }
                Key::Char('s') => {
                    read_line::split_cursors::enter_by_pattern_mode(editor, clients, target)
                }
                Key::Char('S') => {
                    read_line::split_cursors::enter_by_separators_mode(editor, clients, target)
                }
                _ => (),
            },
            Key::Char('r') => match keys.next(&editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char('n') => {
                    move_to_diagnostic(editor, clients, target, true);
                }
                Key::Char('p') => {
                    move_to_diagnostic(editor, clients, target, false);
                }
                Key::Char('r') => editor
                    .status_bar
                    .write_str(StatusMessageKind::Info, "rename not yet implemented"),
                _ => (),
            },
            Key::Char('s') => read_line::search::enter_mode(editor, clients, target),
            Key::Char('y') => {
                let buffer_view = editor.buffer_views.get(handle)?;
                let mut text = String::new();
                buffer_view.get_selection_text(&editor.buffers, &mut text);
                if !text.is_empty() {
                    // TODO: implement clipboard
                    //platform.write_to_clipboard(&text);
                }
                this.movement_kind = CursorMovementKind::PositionAndAnchor;
            }
            Key::Char('Y') => {
                editor.buffer_views.delete_in_cursor_ranges(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    handle,
                    &mut editor.events,
                );

                let mut text = String::new();
                // TODO: implement clipboard
                //if platform.read_from_clipboard(&mut text) {
                editor.buffer_views.insert_text_at_cursor_positions(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    handle,
                    &text,
                    &mut editor.events,
                );
                //}

                let buffer_view = editor.buffer_views.get(handle)?;
                editor
                    .buffers
                    .get_mut(buffer_view.buffer_handle)?
                    .commit_edits();
                this.movement_kind = CursorMovementKind::PositionAndAnchor;

                this.is_recording_auto_macro = false;
                return None;
            }
            Key::Ctrl('y') => match keys.next(&editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char(c) => {
                    editor.buffer_views.delete_in_cursor_ranges(
                        &mut editor.buffers,
                        &mut editor.word_database,
                        handle,
                        &mut editor.events,
                    );
                    if let Some(key) = RegisterKey::from_char(c) {
                        let register = editor.registers.get(key);
                        editor.buffer_views.insert_text_at_cursor_positions(
                            &mut editor.buffers,
                            &mut editor.word_database,
                            handle,
                            register,
                            &mut editor.events,
                        );
                    }
                    let buffer_view = editor.buffer_views.get(handle)?;
                    editor
                        .buffers
                        .get_mut(buffer_view.buffer_handle)?
                        .commit_edits();
                    this.movement_kind = CursorMovementKind::PositionAndAnchor;

                    this.is_recording_auto_macro = false;
                    return None;
                }
                _ => (),
            },
            Key::Char('u') => {
                editor.buffer_views.undo(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    &mut editor.events,
                    handle,
                );
                this.movement_kind = CursorMovementKind::PositionAndAnchor;
                return None;
            }
            Key::Char('U') => {
                editor.buffer_views.redo(
                    &mut editor.buffers,
                    &mut editor.word_database,
                    &mut editor.events,
                    handle,
                );
                this.movement_kind = CursorMovementKind::PositionAndAnchor;
                return None;
            }
            _ => {
                keys.put_back();
                return Self::on_event_no_buffer(editor, clients, target, keys);
            }
        }

        Self::on_movement_keys(editor, keys, keys_from_index);

        editor.mode.normal_state.count = 0;
        None
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            movement_kind: CursorMovementKind::PositionAndAnchor,
            last_char_jump: CharJump::None,
            is_recording_auto_macro: false,
            count: 0,
        }
    }
}

impl ModeState for State {
    fn on_client_keys(
        editor: &mut Editor,
        clients: &mut ClientManager,
        target: TargetClient,
        keys: &mut KeysIterator,
    ) -> Option<ModeOperation> {
        fn show_hovered_diagnostic_in_status_bar(
            editor: &mut Editor,
            clients: &mut ClientManager,
            target: TargetClient,
        ) -> Option<()> {
            let handle = clients.get(target)?.buffer_view_handle()?;
            if !editor.status_bar.message().1.is_empty() {
                return None;
            }
            let buffer_view = editor.buffer_views.get(handle)?;
            let main_position = buffer_view.cursors.main_cursor().position;

            for client in editor.lsp.clients() {
                let diagnostics = client
                    .diagnostics()
                    .buffer_diagnostics(buffer_view.buffer_handle);

                if let Ok(index) = diagnostics.binary_search_by(|d| {
                    let range = d.utf16_range;
                    if range.to < main_position {
                        Ordering::Less
                    } else if range.from > main_position {
                        Ordering::Greater
                    } else {
                        Ordering::Equal
                    }
                }) {
                    editor
                        .status_bar
                        .write_str(StatusMessageKind::Info, &diagnostics[index].message);
                    break;
                }
            }

            None
        }

        match clients.get(target).and_then(Client::buffer_view_handle) {
            Some(handle) => {
                let op =
                    Self::on_client_keys_with_buffer_view(editor, clients, target, keys, handle);
                if let None = op {
                    show_hovered_diagnostic_in_status_bar(editor, clients, target);
                }
                op
            }
            None => Self::on_event_no_buffer(editor, clients, target, keys),
        }
    }
}

fn find_char(
    editor: &mut Editor,
    clients: &mut ClientManager,
    target: TargetClient,
    forward: bool,
) -> Option<()> {
    let state = &editor.mode.normal_state;
    let skip;
    let ch;
    let next_ch;
    match state.last_char_jump {
        CharJump::None => return None,
        CharJump::Inclusive(c) => {
            ch = c;
            next_ch = forward;
            skip = 0;
        }
        CharJump::Exclusive(c) => {
            ch = c;
            next_ch = !forward;
            skip = 1;
        }
    };

    let handle = clients.get(target)?.buffer_view_handle()?;
    let buffer_view = editor.buffer_views.get_mut(handle)?;
    let buffer = editor.buffers.get(buffer_view.buffer_handle)?;

    let count = state.count.max(1) as _;
    for cursor in &mut buffer_view.cursors.mut_guard()[..] {
        let (left_chars, right_chars) = buffer
            .content()
            .line_at(cursor.position.line_index)
            .chars_from(cursor.position.column_byte_index);

        let element = match forward {
            false => left_chars
                .skip(skip)
                .filter(|(_, c)| *c == ch)
                .take(count)
                .last(),
            true => right_chars
                .skip(skip)
                .filter(|(_, c)| *c == ch)
                .take(count)
                .last(),
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

    None
}

fn move_to_search_match<F>(
    editor: &mut Editor,
    clients: &mut ClientManager,
    target: TargetClient,
    index_selector: F,
) -> Option<()>
where
    F: FnOnce(usize, Result<usize, usize>) -> usize,
{
    NavigationHistory::save_client_snapshot(clients, target, &mut editor.buffer_views);

    let client = clients.get_mut(target)?;
    let handle = client.buffer_view_handle()?;
    let buffer_view = editor.buffer_views.get_mut(handle)?;
    let buffer = editor.buffers.get_mut(buffer_view.buffer_handle)?;

    let mut search_ranges = buffer.search_ranges();
    if search_ranges.is_empty() {
        let search = editor.registers.get(SEARCH_REGISTER);
        if !search.is_empty() {
            buffer.set_search(search);
            search_ranges = buffer.search_ranges();
        }

        if search_ranges.is_empty() {
            editor
                .status_bar
                .write_str(StatusMessageKind::Error, "no search result");
            return None;
        }
    }

    let cursors = &mut buffer_view.cursors;

    let main_position = cursors.main_cursor().position;
    let search_result = search_ranges.binary_search_by_key(&main_position, |r| r.from);
    let next_index = index_selector(search_ranges.len(), search_result);

    let mut cursors = cursors.mut_guard();
    let main_cursor = cursors.main_cursor();
    main_cursor.position = search_ranges[next_index].from;

    let line_index = main_cursor.position.line_index;
    let height = client.height as usize;
    if line_index < client.scroll || line_index >= client.scroll + height {
        client.scroll = line_index.saturating_sub(height / 2);
    }

    if let CursorMovementKind::PositionAndAnchor = editor.mode.normal_state.movement_kind {
        main_cursor.anchor = main_cursor.position;
    }

    None
}

fn search_word_or_move_to_it(
    editor: &mut Editor,
    clients: &mut ClientManager,
    target: TargetClient,
    index_selector: fn(usize, Result<usize, usize>) -> usize,
) -> Option<()> {
    let handle = clients.get(target)?.buffer_view_handle()?;
    let buffer_view = editor.buffer_views.get_mut(handle)?;
    let buffer = editor.buffers.get_mut(buffer_view.buffer_handle)?;

    let main_position = buffer_view.cursors.main_cursor().position;
    let search_ranges = buffer.search_ranges();
    let current_range_index = search_ranges.binary_search_by_key(&main_position, |r| r.from);

    if search_ranges.is_empty() || current_range_index.is_err() {
        let search_word = buffer.set_search_with(|c| {
            let word = c.word_at(main_position);

            let mut cursors = buffer_view.cursors.mut_guard();
            cursors.clear();
            cursors.add(Cursor {
                anchor: word.position,
                position: word.position,
            });

            word.text
        });

        editor.registers.set(SEARCH_REGISTER, search_word);
    } else {
        NavigationHistory::save_client_snapshot(clients, target, &mut editor.buffer_views);

        let buffer_view = editor.buffer_views.get_mut(handle)?;
        let mut range_index = current_range_index;

        for _ in 0..editor.mode.normal_state.count.max(1) {
            let i = index_selector(search_ranges.len(), range_index);
            let range = search_ranges[i];
            range_index = Ok(i);

            buffer_view.cursors.mut_guard().add(Cursor {
                anchor: range.from,
                position: range.from,
            });
        }
    }

    editor.mode.normal_state.movement_kind = CursorMovementKind::PositionAndAnchor;
    None
}

fn move_to_diagnostic(
    editor: &mut Editor,
    clients: &mut ClientManager,
    target: TargetClient,
    forward: bool,
) -> Option<()> {
    enum DirectedIter<I> {
        Forward(I),
        Backward(I),
    }
    impl<I> DirectedIter<I> {
        pub fn new(iter: I, forward: bool) -> Self {
            if forward {
                Self::Forward(iter)
            } else {
                Self::Backward(iter)
            }
        }
    }
    impl<I, E> Iterator for DirectedIter<I>
    where
        I: DoubleEndedIterator<Item = E>,
    {
        type Item = E;
        fn next(&mut self) -> Option<Self::Item> {
            match self {
                Self::Forward(iter) => iter.next(),
                Self::Backward(iter) => iter.next_back(),
            }
        }
    }

    let handle = clients.get(target)?.buffer_view_handle()?;
    let buffer_view = editor.buffer_views.get(handle)?;
    let main_position = buffer_view.cursors.main_cursor().position;

    let mut diagnostics = DirectedIter::new(
        editor.lsp.clients().flat_map(|c| c.diagnostics().iter()),
        forward,
    );
    let mut next_diagnostic = None;

    for (path, buffer_handle, diagnostics) in &mut diagnostics {
        if buffer_handle != Some(buffer_view.buffer_handle) {
            continue;
        }

        if forward {
            for d in diagnostics.iter() {
                let range = d.utf16_range;
                if range.from > main_position {
                    next_diagnostic = Some((path, buffer_handle, range.from));
                    break;
                }
            }
        } else {
            for d in diagnostics.iter().rev() {
                let range = d.utf16_range;
                if range.from < main_position {
                    next_diagnostic = Some((path, buffer_handle, range.from));
                    break;
                }
            }
        }
        break;
    }

    fn select_diagnostic_position(diagnostics: &[LspDiagnostic], forward: bool) -> BufferPosition {
        if forward {
            diagnostics[0].utf16_range.from
        } else {
            diagnostics[diagnostics.len() - 1].utf16_range.from
        }
    }

    if let None = next_diagnostic {
        next_diagnostic = diagnostics
            .next()
            .map(|(p, h, d)| (p, h, select_diagnostic_position(d, forward)));
    }

    if let None = next_diagnostic {
        let mut iter = DirectedIter::new(
            editor.lsp.clients().flat_map(|c| c.diagnostics().iter()),
            forward,
        );
        next_diagnostic = iter
            .next()
            .map(|(p, h, d)| (p, h, select_diagnostic_position(d, forward)));
    }

    drop(diagnostics);

    let (path, buffer_handle, position) = next_diagnostic?;
    let buffer_view_handle = match buffer_handle {
        Some(buffer_handle) => editor
            .buffer_views
            .buffer_view_handle_from_buffer_handle(target, buffer_handle),
        None => match editor.buffer_views.buffer_view_handle_from_path(
            target,
            &mut editor.buffers,
            &mut editor.word_database,
            &editor.current_directory,
            path,
            None,
            &mut editor.events,
        ) {
            Ok(handle) => handle,
            Err(error) => {
                editor.status_bar.write_error(&error);
                return None;
            }
        },
    };

    NavigationHistory::save_client_snapshot(clients, target, &mut editor.buffer_views);

    let buffer_view = editor.buffer_views.get_mut(buffer_view_handle)?;
    let buffer = editor.buffers.get(buffer_view.buffer_handle)?;
    let position = buffer.content().saturate_position(position);

    let mut cursors = buffer_view.cursors.mut_guard();
    cursors.clear();
    cursors.add(Cursor {
        anchor: position,
        position,
    });

    drop(cursors);
    drop(buffer_view);

    clients.set_buffer_view_handle(editor, target, Some(buffer_view_handle));
    editor.mode.normal_state.movement_kind = CursorMovementKind::PositionAndAnchor;

    None
}
