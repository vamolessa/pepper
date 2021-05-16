use std::{cmp::Ordering, fmt::Write, path::Path};

use crate::{
    buffer::{find_path_and_position_at, parse_path_and_position, BufferContent},
    buffer_position::{BufferPosition, BufferPositionIndex, BufferRange},
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    client::Client,
    cursor::{Cursor, CursorCollection},
    editor::{Editor, EditorControlFlow, KeysIterator},
    editor_utils::{hash_bytes, MessageKind},
    help::HELP_PREFIX,
    lsp,
    mode::{picker, read_line, Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    navigation_history::{NavigationDirection, NavigationHistory},
    platform::Key,
    register::{RegisterKey, AUTO_MACRO_REGISTER, SEARCH_REGISTER},
    word_database::WordKind,
};

enum CharJump {
    None,
    Inclusive(char),
    Exclusive(char),
}

pub struct State {
    pub movement_kind: CursorMovementKind,
    pub search_index: usize,
    last_char_jump: CharJump,
    is_recording_auto_macro: bool,
    pub count: u32,
    last_copy_hash: u64,
    last_copy_ranges: Vec<(BufferPositionIndex, BufferPositionIndex)>,
}

impl State {
    fn on_movement_keys(editor: &mut Editor, keys: &KeysIterator, from_index: usize) {
        let state = &mut editor.mode.normal_state;
        match state.movement_kind {
            CursorMovementKind::PositionAndAnchor => state.is_recording_auto_macro = false,
            CursorMovementKind::PositionOnly => {
                let auto_macro_register = editor.registers.get_mut(AUTO_MACRO_REGISTER);

                if !state.is_recording_auto_macro {
                    auto_macro_register.clear();
                }
                state.is_recording_auto_macro = true;

                if auto_macro_register.is_empty() && state.count > 0 {
                    let _ = write!(auto_macro_register, "{}", state.count);
                }

                for key in &editor.buffered_keys.as_slice()[from_index..keys.index()] {
                    let _ = write!(auto_macro_register, "{}", key);
                }
            }
        }
    }

    fn on_edit_keys(editor: &mut Editor, keys: &KeysIterator, from_index: usize) {
        let auto_macro_register = editor.registers.get_mut(AUTO_MACRO_REGISTER);
        let state = &mut editor.mode.normal_state;
        if !state.is_recording_auto_macro {
            auto_macro_register.clear();
        }
        state.is_recording_auto_macro = false;

        if auto_macro_register.is_empty() && state.count > 0 {
            let _ = write!(auto_macro_register, "{}", state.count);
        }

        for key in &editor.buffered_keys.as_slice()[from_index..keys.index()] {
            let _ = write!(auto_macro_register, "{}", key);
        }
    }

    fn on_event_no_buffer(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<ModeOperation> {
        let state = &mut ctx.editor.mode.normal_state;
        state.is_recording_auto_macro = false;
        match keys.next(&ctx.editor.buffered_keys) {
            Key::Char('q') => {
                if ctx.editor.recording_macro.take().is_none() {
                    match keys.next(&ctx.editor.buffered_keys) {
                        Key::None => return Some(ModeOperation::Pending),
                        Key::Char(c) => {
                            if let Some(key) = RegisterKey::from_char(c) {
                                ctx.editor.registers.get_mut(key).clear();
                                ctx.editor.recording_macro = Some(key);
                            }
                        }
                        _ => (),
                    }
                }
            }
            Key::Char('Q') => {
                ctx.editor.recording_macro = None;
                match keys.next(&ctx.editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char(c) => {
                        if let Some(key) = RegisterKey::from_char(c.to_ascii_lowercase()) {
                            for _ in 0..state.count.max(1) {
                                let keys = ctx.editor.registers.get(key);
                                match ctx.editor.buffered_keys.parse(keys) {
                                    Ok(keys) => {
                                        match ctx.editor.execute_keys(
                                            ctx.platform,
                                            ctx.clients,
                                            ctx.client_handle,
                                            keys,
                                        ) {
                                            EditorControlFlow::Continue => (),
                                            EditorControlFlow::Quit => {
                                                return Some(ModeOperation::Quit);
                                            }
                                            EditorControlFlow::QuitAll => {
                                                return Some(ModeOperation::QuitAll);
                                            }
                                        };
                                    }
                                    Err(error) => ctx
                                        .editor
                                        .status_bar
                                        .write(MessageKind::Error)
                                        .fmt(format_args!("{}", error)),
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
            Key::Char(':') => Mode::change_to(ctx, ModeKind::Command),
            Key::Char('g') | Key::Char('G') => {
                if state.count == 0 {
                    match keys.next(&ctx.editor.buffered_keys) {
                        Key::None => return Some(ModeOperation::Pending),
                        Key::Char('o') => picker::buffer::enter_mode(ctx),
                        Key::Char('b') => {
                            let client = ctx.clients.get_mut(ctx.client_handle)?;
                            if let Some(buffer_view_handle) = client.previous_buffer_view_handle() {
                                client.set_buffer_view_handle(
                                    Some(buffer_view_handle),
                                    &mut ctx.editor.events,
                                );
                            }
                        }
                        Key::Char('B') => {
                            let previous_client_handle = ctx.clients.previous_focused_client()?;
                            let previous_client = ctx.clients.get_mut(previous_client_handle)?;
                            let buffer_view_handle = previous_client.buffer_view_handle();
                            let previous_buffer_view_handle =
                                previous_client.previous_buffer_view_handle();
                            previous_client.set_buffer_view_handle(
                                previous_buffer_view_handle,
                                &mut ctx.editor.events,
                            );

                            let client = ctx.clients.get_mut(ctx.client_handle)?;
                            client
                                .set_buffer_view_handle(buffer_view_handle, &mut ctx.editor.events);
                        }
                        _ => (),
                    }
                }
            }
            Key::Char(c) => {
                if let Some(n) = c.to_digit(10) {
                    state.count = state.count.saturating_mul(10).saturating_add(n);
                    return None;
                }
            }
            _ => (),
        }

        ctx.editor.mode.normal_state.count = 0;
        None
    }

    fn on_client_keys_with_buffer_view(
        ctx: &mut ModeContext,
        keys: &mut KeysIterator,
        handle: BufferViewHandle,
    ) -> Option<ModeOperation> {
        let state = &mut ctx.editor.mode.normal_state;
        let keys_from_index = keys.index();
        match keys.next(&ctx.editor.buffered_keys) {
            Key::Char('h') => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::ColumnsBackward(state.count.max(1) as _),
                state.movement_kind,
                ctx.editor.config.tab_size,
            ),
            Key::Char('j') => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::LinesForward(state.count.max(1) as _),
                state.movement_kind,
                ctx.editor.config.tab_size,
            ),
            Key::Char('k') => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::LinesBackward(state.count.max(1) as _),
                state.movement_kind,
                ctx.editor.config.tab_size,
            ),
            Key::Char('l') => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::ColumnsForward(state.count.max(1) as _),
                state.movement_kind,
                ctx.editor.config.tab_size,
            ),
            Key::Char('w') => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::WordsForward(state.count.max(1) as _),
                state.movement_kind,
                ctx.editor.config.tab_size,
            ),
            Key::Char('b') => ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                &ctx.editor.buffers,
                CursorMovement::WordsBackward(state.count.max(1) as _),
                state.movement_kind,
                ctx.editor.config.tab_size,
            ),
            Key::Char('n') => {
                let count = state.count.max(1);
                move_to_search_match(ctx, |len, r| {
                    let index = match r {
                        Ok(index) => index + count as usize,
                        Err(index) => index + count as usize - 1,
                    };
                    index % len
                });
            }
            Key::Char('p') => {
                let count = state.count.max(1) as usize;
                move_to_search_match(ctx, |len, r| {
                    let index = match r {
                        Ok(index) => index,
                        Err(index) => index,
                    };
                    (index + len - count % len) % len
                });
            }
            Key::Char('N') => {
                search_word_or_move_to_it(ctx, |len, r| {
                    let index = match r {
                        Ok(index) => index + 1,
                        Err(index) => index,
                    };
                    index % len
                });
            }
            Key::Char('P') => {
                search_word_or_move_to_it(ctx, |len, r| {
                    let index = match r {
                        Ok(index) => index,
                        Err(index) => index,
                    };
                    (index + len - 1) % len
                });
            }
            Key::Ctrl('n') => {
                state.movement_kind = CursorMovementKind::PositionAndAnchor;
                NavigationHistory::move_in_history(
                    ctx.editor,
                    ctx.clients,
                    ctx.client_handle,
                    NavigationDirection::Forward,
                );
            }
            Key::Ctrl('p') => {
                state.movement_kind = CursorMovementKind::PositionAndAnchor;
                NavigationHistory::move_in_history(
                    ctx.editor,
                    ctx.clients,
                    ctx.client_handle,
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

                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?.content();
                let mut cursors = buffer_view.cursors.mut_guard();

                match keys.next(&ctx.editor.buffered_keys) {
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
                            anchor: BufferPosition::zero(),
                            position: BufferPosition::line_col(
                                last_line_index as _,
                                last_line_len as _,
                            ),
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
                    Key::Char('`') => delimiter_pair(buffer, &mut cursors[..], '`'),
                    _ => (),
                }

                state.movement_kind = CursorMovementKind::PositionOnly;
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
                                range.from.column_byte_index
                                    - left.len_utf8() as BufferPositionIndex,
                            );
                            cursor.position = BufferPosition::line_col(
                                range.to.line_index,
                                range.to.column_byte_index
                                    + right.len_utf8() as BufferPositionIndex,
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
                                range.from.column_byte_index
                                    - delimiter.len_utf8() as BufferPositionIndex,
                            );
                            cursor.position = BufferPosition::line_col(
                                range.to.line_index,
                                range.to.column_byte_index
                                    + delimiter.len_utf8() as BufferPositionIndex,
                            );
                        }
                    }
                }

                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?.content();
                let mut cursors = buffer_view.cursors.mut_guard();

                match keys.next(&ctx.editor.buffered_keys) {
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
                            anchor: BufferPosition::zero(),
                            position: BufferPosition::line_col(
                                last_line_index as _,
                                last_line_len as _,
                            ),
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

                state.movement_kind = CursorMovementKind::PositionOnly;
            }
            Key::Char('g') => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                match keys.next(&ctx.editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char('g') => {
                        if state.count > 0 {
                            NavigationHistory::save_client_snapshot(
                                ctx.clients,
                                ctx.client_handle,
                                &mut ctx.editor.buffer_views,
                            );
                            let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                            let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?;
                            let buffer = buffer.content();
                            let line_index = state.count - 1;
                            let mut position = BufferPosition::line_col(line_index as _, 0);
                            position = buffer.saturate_position(position);
                            let word = buffer.word_at(position);
                            if word.kind == WordKind::Whitespace {
                                position = word.end_position();
                            }

                            let mut cursors = buffer_view.cursors.mut_guard();
                            cursors.clear();
                            cursors.add(Cursor {
                                anchor: position,
                                position,
                            });
                        } else {
                            read_line::goto::enter_mode(ctx);
                        }
                    }
                    Key::Char('h') => buffer_view.move_cursors(
                        &ctx.editor.buffers,
                        CursorMovement::Home,
                        state.movement_kind,
                        ctx.editor.config.tab_size,
                    ),
                    Key::Char('j') => {
                        NavigationHistory::save_client_snapshot(
                            ctx.clients,
                            ctx.client_handle,
                            &ctx.editor.buffer_views,
                        );
                        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                        buffer_view.move_cursors(
                            &ctx.editor.buffers,
                            CursorMovement::LastLine,
                            state.movement_kind,
                            ctx.editor.config.tab_size,
                        );
                    }
                    Key::Char('k') => {
                        NavigationHistory::save_client_snapshot(
                            ctx.clients,
                            ctx.client_handle,
                            &ctx.editor.buffer_views,
                        );
                        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                        buffer_view.move_cursors(
                            &ctx.editor.buffers,
                            CursorMovement::FirstLine,
                            state.movement_kind,
                            ctx.editor.config.tab_size,
                        );
                    }
                    Key::Char('l') => buffer_view.move_cursors(
                        &ctx.editor.buffers,
                        CursorMovement::End,
                        state.movement_kind,
                        ctx.editor.config.tab_size,
                    ),
                    Key::Char('i') => buffer_view.move_cursors(
                        &ctx.editor.buffers,
                        CursorMovement::HomeNonWhitespace,
                        state.movement_kind,
                        ctx.editor.config.tab_size,
                    ),
                    Key::Char('m') => {
                        let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?.content();
                        for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                            let mut position = cursor.position;

                            let line = buffer.line_at(position.line_index as _).as_str();
                            let cursor_char = if position.column_byte_index < line.len() as _ {
                                match line[position.column_byte_index as usize..].chars().next() {
                                    Some(c) => c,
                                    None => continue,
                                }
                            } else {
                                match line.char_indices().next_back() {
                                    Some((i, c)) => {
                                        position.column_byte_index = i as _;
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
                                d @ '|' | d @ '"' | d @ '\'' | d @ '`' => {
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

                                if let CursorMovementKind::PositionAndAnchor = state.movement_kind {
                                    cursor.anchor = cursor.position;
                                }
                            }
                        }
                    }
                    Key::Char('f') => {
                        let buffer_handle = buffer_view.buffer_handle;

                        let mut len = 0;
                        let mut ranges = [BufferRange::zero(); CursorCollection::capacity()];
                        for cursor in &buffer_view.cursors[..] {
                            ranges[len] = cursor.to_range();
                            len += 1;
                        }

                        let mut jumped = false;
                        let mut path_buf = ctx.editor.string_pool.acquire();
                        let fallback_line_index = state.count.saturating_sub(1) as _;
                        for range in &ranges[..len] {
                            let line_index = range.from.line_index;
                            if range.to.line_index != line_index {
                                continue;
                            }

                            let buffer = ctx.editor.buffers.get(buffer_handle)?;

                            let line = buffer.content().line_at(line_index as _).as_str();

                            let from = range.from.column_byte_index;
                            let to = range.to.column_byte_index;

                            let (path, position) = if from < to {
                                parse_path_and_position(&line[from as usize..to as usize])
                            } else {
                                find_path_and_position_at(line, from as _)
                            };
                            let position = match position {
                                Some(position) => position,
                                None => BufferPosition::line_col(fallback_line_index, 0),
                            };

                            path_buf.clear();
                            if Path::new(path).is_relative() {
                                if buffer.path.starts_with(HELP_PREFIX) {
                                    path_buf.push_str(HELP_PREFIX);
                                } else if let Some(parent) =
                                    buffer.path.parent().and_then(Path::to_str)
                                {
                                    if !parent.is_empty() {
                                        path_buf.push_str(parent);
                                        path_buf.push('/');
                                    }
                                }
                            }
                            path_buf.push_str(path);

                            let path = Path::new(&path_buf);
                            if !path.starts_with(HELP_PREFIX) && !path.exists() {
                                ctx.editor
                                    .status_bar
                                    .write(MessageKind::Error)
                                    .fmt(format_args!("file {:?} does not exist", path));
                                continue;
                            }

                            if !jumped {
                                NavigationHistory::save_client_snapshot(
                                    ctx.clients,
                                    ctx.client_handle,
                                    &ctx.editor.buffer_views,
                                );
                            }

                            let handle = ctx
                                .editor
                                .buffer_view_handle_from_path(ctx.client_handle, path);
                            if let Some(buffer_view) = ctx.editor.buffer_views.get_mut(handle) {
                                let mut cursors = buffer_view.cursors.mut_guard();
                                cursors.clear();
                                cursors.add(Cursor {
                                    anchor: position,
                                    position,
                                });
                            }

                            if jumped {
                                continue;
                            }
                            jumped = true;

                            ctx.editor.mode.normal_state.movement_kind =
                                CursorMovementKind::PositionAndAnchor;
                            if let Some(client) = ctx.clients.get_mut(ctx.client_handle) {
                                client.set_buffer_view_handle(Some(handle), &mut ctx.editor.events);
                            }
                        }
                        ctx.editor.string_pool.release(path_buf);
                    }
                    _ => {
                        keys.put_back();
                        keys.put_back();
                        return Self::on_event_no_buffer(ctx, keys);
                    }
                }
            }
            Key::Char('[') => match keys.next(&ctx.editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char('[') => match keys.next(&ctx.editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char(ch) => {
                        state.last_char_jump = CharJump::Inclusive(ch);
                        find_char(ctx, false);
                    }
                    _ => (),
                },
                Key::Char(']') => match keys.next(&ctx.editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char(ch) => {
                        state.last_char_jump = CharJump::Exclusive(ch);
                        find_char(ctx, false);
                    }
                    _ => (),
                },
                _ => (),
            },
            Key::Char(']') => match keys.next(&ctx.editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char('[') => match keys.next(&ctx.editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char(ch) => {
                        state.last_char_jump = CharJump::Exclusive(ch);
                        find_char(ctx, true);
                    }
                    _ => (),
                },
                Key::Char(']') => match keys.next(&ctx.editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char(ch) => {
                        state.last_char_jump = CharJump::Inclusive(ch);
                        find_char(ctx, true);
                    }
                    _ => (),
                },
                _ => (),
            },
            Key::Char('{') => {
                find_char(ctx, false);
            }
            Key::Char('}') => {
                find_char(ctx, true);
            }
            Key::Char('v') => {
                state.movement_kind = match state.movement_kind {
                    CursorMovementKind::PositionAndAnchor => CursorMovementKind::PositionOnly,
                    CursorMovementKind::PositionOnly => {
                        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                        for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                            cursor.anchor = cursor.position;
                        }
                        CursorMovementKind::PositionAndAnchor
                    }
                };
            }
            Key::Char('V') => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?.content();

                let count = state.count.max(1);
                let last_line_index = buffer.line_count().saturating_sub(1);
                for cursor in &mut buffer_view.cursors.mut_guard()[..] {
                    if cursor.anchor <= cursor.position {
                        cursor.anchor.column_byte_index = 0;
                        cursor.position.line_index += count as BufferPositionIndex;
                        if cursor.position.line_index <= last_line_index as _ {
                            cursor.position.column_byte_index = 0;
                        } else {
                            cursor.position.line_index = last_line_index as _;
                            cursor.position.column_byte_index = buffer
                                .line_at(cursor.position.line_index as _)
                                .as_str()
                                .len()
                                as _;
                        }
                    } else {
                        cursor.anchor.column_byte_index =
                            buffer.line_at(cursor.anchor.line_index as _).as_str().len() as _;
                        if cursor.position.line_index >= count as _ {
                            cursor.position.line_index -= count as BufferPositionIndex;
                            cursor.position.column_byte_index = buffer
                                .line_at(cursor.position.line_index as _)
                                .as_str()
                                .len()
                                as _;
                        } else {
                            cursor.position.line_index = 0;
                            cursor.position.column_byte_index = 0;
                        }
                    }
                }
                state.movement_kind = CursorMovementKind::PositionOnly;
            }
            Key::Char('z') => {
                let buffer_view = ctx.editor.buffer_views.get(handle)?;
                let focused_line_index = buffer_view.cursors.main_cursor().position.line_index;
                let client = ctx.clients.get_mut(ctx.client_handle)?;
                let height = client.height;

                match keys.next(&ctx.editor.buffered_keys) {
                    Key::None => return Some(ModeOperation::Pending),
                    Key::Char('z') => {
                        client.scroll.0 = 0;
                        client.scroll.1 = focused_line_index.saturating_sub((height / 2) as _);
                    }
                    Key::Char('j') => {
                        client.scroll.0 = 0;
                        client.scroll.1 = focused_line_index.saturating_sub(height as _);
                    }
                    Key::Char('k') => {
                        client.scroll.0 = 0;
                        client.scroll.1 = focused_line_index;
                    }
                    _ => (),
                }
            }
            Key::Ctrl('j') => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?.content();
                let mut cursors = buffer_view.cursors.mut_guard();

                for cursor in &mut cursors[..] {
                    let position = match buffer
                        .lines()
                        .enumerate()
                        .skip(cursor.position.line_index as usize + 1)
                        .filter(|(_, l)| l.as_str().chars().all(|c| c.is_whitespace()))
                        .nth(state.count.max(1).saturating_sub(1) as _)
                    {
                        Some((i, line)) => {
                            BufferPosition::line_col(i as _, line.as_str().len() as _)
                        }
                        None => {
                            let line_index = buffer.line_count() - 1;
                            let column_byte_index = buffer
                                .line_at(line_index)
                                .as_str()
                                .find(|c: char| !c.is_whitespace())
                                .unwrap_or(0);
                            BufferPosition::line_col(line_index as _, column_byte_index as _)
                        }
                    };
                    cursor.position = position;
                    if let CursorMovementKind::PositionAndAnchor = state.movement_kind {
                        cursor.anchor = cursor.position;
                    }
                }
            }
            Key::Ctrl('k') => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?.content();
                let mut cursors = buffer_view.cursors.mut_guard();

                for cursor in &mut cursors[..] {
                    let position = match buffer
                        .lines()
                        .enumerate()
                        .rev()
                        .skip(buffer.line_count() - cursor.position.line_index as usize)
                        .filter(|(_, l)| l.as_str().chars().all(|c| c.is_whitespace()))
                        .nth(state.count.max(1).saturating_sub(1) as _)
                    {
                        Some((i, line)) => {
                            BufferPosition::line_col(i as _, line.as_str().len() as _)
                        }
                        None => {
                            let column_byte_index = buffer
                                .line_at(0)
                                .as_str()
                                .find(|c: char| !c.is_whitespace())
                                .unwrap_or(0);
                            BufferPosition::line_col(0, column_byte_index as _)
                        }
                    };
                    cursor.position = position;
                    if let CursorMovementKind::PositionAndAnchor = state.movement_kind {
                        cursor.anchor = cursor.position;
                    }
                }
            }
            Key::Ctrl('d') => {
                let half_height = ctx
                    .clients
                    .get(ctx.client_handle)
                    .map(|c| c.height)
                    .unwrap_or(0)
                    / 2;
                ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::LinesForward(
                        half_height as usize * state.count.max(1) as usize,
                    ),
                    state.movement_kind,
                    ctx.editor.config.tab_size,
                );
            }
            Key::Ctrl('u') => {
                let half_height = ctx
                    .clients
                    .get(ctx.client_handle)
                    .map(|c| c.height)
                    .unwrap_or(0)
                    / 2;
                ctx.editor.buffer_views.get_mut(handle)?.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::LinesBackward(
                        half_height as usize * state.count.max(1) as usize,
                    ),
                    state.movement_kind,
                    ctx.editor.config.tab_size,
                );
            }
            Key::Char('d') => {
                let buffer_view = ctx.editor.buffer_views.get(handle)?;
                buffer_view.delete_text_in_cursor_ranges(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );

                ctx.editor
                    .buffers
                    .get_mut(buffer_view.buffer_handle)?
                    .commit_edits();
                state.movement_kind = CursorMovementKind::PositionAndAnchor;
                Self::on_edit_keys(ctx.editor, keys, keys_from_index);
                return None;
            }
            Key::Char('i') => {
                let buffer_view = ctx.editor.buffer_views.get(handle)?;
                buffer_view.delete_text_in_cursor_ranges(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );

                Self::on_edit_keys(ctx.editor, keys, keys_from_index);
                Mode::change_to(ctx, ModeKind::Insert);
                return None;
            }
            Key::Char('<') => {
                let buffer_view = ctx.editor.buffer_views.get(handle)?;
                let cursor_count = buffer_view.cursors[..].len();
                let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle)?;
                let count = state.count.max(1);

                for i in 0..cursor_count {
                    let range = ctx.editor.buffer_views.get(handle)?.cursors[i].to_range();
                    for line_index in range.from.line_index..=range.to.line_index {
                        let line = buffer.content().line_at(line_index as _).as_str();
                        let mut indentation_column_index = 0;

                        for _ in 0..count {
                            let mut chars = line[indentation_column_index..].char_indices();
                            indentation_column_index += match chars.next() {
                                Some((i, c @ '\t')) => i + c.len_utf8(),
                                Some((i, c @ ' ')) => {
                                    match chars
                                        .take(ctx.editor.config.tab_size.get() as usize - 1)
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
                            BufferPosition::line_col(line_index, indentation_column_index as _),
                        );
                        buffer.delete_range(
                            &mut ctx.editor.word_database,
                            range,
                            &mut ctx.editor.events,
                        );
                    }
                }

                buffer.commit_edits();
                Self::on_edit_keys(ctx.editor, keys, keys_from_index);
                return None;
            }
            Key::Char('>') => {
                let cursor_count = ctx.editor.buffer_views.get(handle)?.cursors[..].len();

                let extender = if ctx.editor.config.indent_with_tabs {
                    let count = state.count.max(1) as _;
                    std::iter::repeat('\t').take(count)
                } else {
                    let tab_size = ctx.editor.config.tab_size.get() as usize;
                    let count = state.count.max(1) as usize * tab_size;
                    std::iter::repeat(' ').take(count)
                };

                let buffer_view = ctx.editor.buffer_views.get(handle)?;
                let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle)?;

                let mut buf = ctx.editor.string_pool.acquire();
                buf.extend(extender);
                for i in 0..cursor_count {
                    let range = ctx.editor.buffer_views.get(handle)?.cursors[i].to_range();
                    for line_index in range.from.line_index..=range.to.line_index {
                        buffer.insert_text(
                            &mut ctx.editor.word_database,
                            BufferPosition::line_col(line_index, 0),
                            &buf,
                            &mut ctx.editor.events,
                        );
                    }
                }
                ctx.editor.string_pool.release(buf);

                buffer.commit_edits();
                Self::on_edit_keys(ctx.editor, keys, keys_from_index);
                return None;
            }
            Key::Char('c') | Key::Char('C') => match keys.next(&ctx.editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char('c') => {
                    for cursor in
                        &mut ctx.editor.buffer_views.get_mut(handle)?.cursors.mut_guard()[..]
                    {
                        std::mem::swap(&mut cursor.anchor, &mut cursor.position);
                    }
                }
                Key::Char('l') => {
                    let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?.content();

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
                                buffer.line_at(range.from.line_index as _).as_str().len() as _,
                            );

                            for line_index in (range.from.line_index + 1)..range.to.line_index {
                                let line_len = buffer.line_at(line_index as _).as_str().len();
                                cursors.add(Cursor {
                                    anchor: BufferPosition::line_col(line_index, 0),
                                    position: BufferPosition::line_col(line_index, line_len as _),
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
                                let line_len = buffer.line_at(line_index as _).as_str().len();
                                cursors.add(Cursor {
                                    anchor: BufferPosition::line_col(line_index, line_len as _),
                                    position: BufferPosition::line_col(line_index, 0),
                                });
                            }

                            cursors.add(Cursor {
                                anchor: BufferPosition::line_col(
                                    range.from.line_index,
                                    buffer.line_at(range.from.line_index as _).as_str().len() as _,
                                ),
                                position: range.from,
                            });
                        }
                    }
                }
                Key::Char('d') => {
                    let cursors = &mut ctx.editor.buffer_views.get_mut(handle)?.cursors;
                    let main_cursor = *cursors.main_cursor();
                    let mut cursors = cursors.mut_guard();
                    cursors.clear();
                    cursors.add(main_cursor);
                    state.movement_kind = CursorMovementKind::PositionAndAnchor;
                }
                Key::Char('v') => {
                    state.movement_kind = CursorMovementKind::PositionOnly;
                }
                Key::Char('V') => {
                    for cursor in
                        &mut ctx.editor.buffer_views.get_mut(handle)?.cursors.mut_guard()[..]
                    {
                        cursor.anchor = cursor.position;
                    }
                    state.movement_kind = CursorMovementKind::PositionAndAnchor;
                }
                Key::Char('j') => {
                    let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?;
                    let mut cursors = buffer_view.cursors.mut_guard();

                    if let Some(cursor) = cursors[..].last() {
                        let mut position = cursor.to_range().to;

                        for _ in 0..state.count.max(1) {
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
                    let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?;
                    let mut cursors = buffer_view.cursors.mut_guard();

                    if let Some(cursor) = cursors[..].first() {
                        let mut position = cursor.to_range().from;

                        for _ in 0..state.count.max(1) {
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
                    let cursors = &mut ctx.editor.buffer_views.get_mut(handle)?.cursors;
                    let index = cursors.main_cursor_index();
                    let mut cursors = cursors.mut_guard();
                    let cursor_count = cursors[..].len();
                    let offset = state.count.max(1) as usize;
                    cursors.set_main_cursor_index((index + offset) % cursor_count);
                    let ranges = &mut ctx.editor.mode.normal_state.last_copy_ranges;
                    if !ranges.is_empty() {
                        let offset = offset % ranges.len();
                        ranges.rotate_right(offset);
                    }
                }
                Key::Char('p') => {
                    let cursors = &mut ctx.editor.buffer_views.get_mut(handle)?.cursors;
                    let index = cursors.main_cursor_index();
                    let mut cursors = cursors.mut_guard();
                    let cursor_count = cursors[..].len();
                    let offset = state.count.max(1) as usize % cursor_count;
                    cursors.set_main_cursor_index((index + cursor_count - offset) % cursor_count);
                    let ranges = &mut ctx.editor.mode.normal_state.last_copy_ranges;
                    if !ranges.is_empty() {
                        let offset = offset % ranges.len();
                        ranges.rotate_left(offset);
                    }
                }
                Key::Char('f') => read_line::filter_cursors::enter_filter_mode(ctx),
                Key::Char('F') => read_line::filter_cursors::enter_except_mode(ctx),
                Key::Char('s') => read_line::split_cursors::enter_by_pattern_mode(ctx),
                Key::Char('S') => read_line::split_cursors::enter_by_separators_mode(ctx),
                _ => (),
            },
            Key::Char('r') => match keys.next(&ctx.editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char('n') => {
                    move_to_diagnostic(ctx, true);
                }
                Key::Char('p') => {
                    move_to_diagnostic(ctx, false);
                }
                _ => (),
            },
            Key::Char('s') => read_line::search::enter_mode(ctx),
            Key::Char('y') => {
                let mut text = ctx.editor.string_pool.acquire();
                copy_text(ctx, handle, &mut text);
                if !text.is_empty() {
                    ctx.platform
                        .write_to_clipboard(&ctx.editor.registers, &text);
                }
                ctx.editor.string_pool.release(text);
            }
            Key::Char('Y') => {
                let mut text = ctx.editor.string_pool.acquire();
                ctx.platform
                    .read_from_clipboard(&ctx.editor.registers, &mut text);
                paste_text(ctx, handle, &text);
                ctx.editor.string_pool.release(text);
                return None;
            }
            Key::Ctrl('y') => match keys.next(&ctx.editor.buffered_keys) {
                Key::None => return Some(ModeOperation::Pending),
                Key::Char(c) => {
                    let key = c.to_ascii_lowercase();
                    if key == c {
                        if let Some(key) = RegisterKey::from_char(key) {
                            let mut text = ctx.editor.string_pool.acquire();
                            copy_text(ctx, handle, &mut text);
                            if !text.is_empty() {
                                let register = ctx.editor.registers.get_mut(key);
                                register.clear();
                                register.push_str(&text);
                            }
                            ctx.editor.string_pool.release(text);
                        }
                    } else {
                        if let Some(key) = RegisterKey::from_char(key) {
                            let register = ctx.editor.registers.get(key);
                            let text = ctx.editor.string_pool.acquire_with(register);
                            paste_text(ctx, handle, &text);
                            ctx.editor.string_pool.release(text);
                            return None;
                        }
                    }
                }
                _ => (),
            },
            Key::Char('u') => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                buffer_view.undo(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );
                state.movement_kind = CursorMovementKind::PositionAndAnchor;
                return None;
            }
            Key::Char('U') => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                buffer_view.redo(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );
                state.movement_kind = CursorMovementKind::PositionAndAnchor;
                return None;
            }
            _ => {
                keys.put_back();
                return Self::on_event_no_buffer(ctx, keys);
            }
        }

        Self::on_movement_keys(ctx.editor, keys, keys_from_index);
        ctx.editor.mode.normal_state.count = 0;
        None
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            movement_kind: CursorMovementKind::PositionAndAnchor,
            search_index: 0,
            last_char_jump: CharJump::None,
            is_recording_auto_macro: false,
            count: 0,
            last_copy_hash: 0,
            last_copy_ranges: Vec::new(),
        }
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        let state = &mut ctx.editor.mode.normal_state;
        state.movement_kind = CursorMovementKind::PositionAndAnchor;
        state.is_recording_auto_macro = false;
        state.count = 0;
    }

    fn on_exit(_: &mut ModeContext) {}

    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<ModeOperation> {
        fn show_hovered_diagnostic(ctx: &mut ModeContext) -> Option<()> {
            let handle = ctx.clients.get(ctx.client_handle)?.buffer_view_handle()?;
            if !ctx.editor.status_bar.message().1.is_empty() {
                return None;
            }
            let buffer_view = ctx.editor.buffer_views.get(handle)?;
            let main_position = buffer_view.cursors.main_cursor().position;

            for client in ctx.editor.lsp.clients() {
                let diagnostics = client
                    .diagnostics()
                    .buffer_diagnostics(buffer_view.buffer_handle);

                if let Ok(index) = diagnostics.binary_search_by(|d| {
                    let range = d.range;
                    if range.to < main_position {
                        Ordering::Less
                    } else if range.from > main_position {
                        Ordering::Greater
                    } else {
                        Ordering::Equal
                    }
                }) {
                    ctx.editor
                        .status_bar
                        .write(MessageKind::Info)
                        .str(&diagnostics[index].message);
                    break;
                }
            }

            None
        }

        match ctx
            .clients
            .get(ctx.client_handle)
            .and_then(Client::buffer_view_handle)
        {
            Some(handle) => {
                let op = Self::on_client_keys_with_buffer_view(ctx, keys, handle);
                if let None = op {
                    show_hovered_diagnostic(ctx);
                }
                op
            }
            None => Self::on_event_no_buffer(ctx, keys),
        }
    }
}

fn copy_text(
    ctx: &mut ModeContext,
    buffer_view_handle: BufferViewHandle,
    text: &mut String,
) -> Option<()> {
    let state = &mut ctx.editor.mode.normal_state;
    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle)?;
    buffer_view.append_selection_text(&ctx.editor.buffers, text, &mut state.last_copy_ranges);
    if !text.is_empty() {
        state.last_copy_hash = hash_bytes(text.bytes());
    }
    state.movement_kind = CursorMovementKind::PositionAndAnchor;
    None
}

fn paste_text(
    ctx: &mut ModeContext,
    buffer_view_handle: BufferViewHandle,
    text: &str,
) -> Option<()> {
    let state = &mut ctx.editor.mode.normal_state;
    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle)?;
    buffer_view.delete_text_in_cursor_ranges(
        &mut ctx.editor.buffers,
        &mut ctx.editor.word_database,
        &mut ctx.editor.events,
    );

    state.movement_kind = CursorMovementKind::PositionAndAnchor;
    state.is_recording_auto_macro = false;

    ctx.editor.trigger_event_handlers(ctx.platform, ctx.clients);

    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle)?;
    let hash = ctx.editor.mode.normal_state.last_copy_hash;
    let ranges = &ctx.editor.mode.normal_state.last_copy_ranges[..];
    let cursors = &buffer_view.cursors[..];
    if hash == hash_bytes(text.bytes()) && ranges.len() == cursors.len() {
        if let Some(buffer) = ctx.editor.buffers.get_mut(buffer_view.buffer_handle) {
            for (range, cursor) in ranges.iter().zip(cursors.iter()).rev() {
                let text = &text[range.0 as usize..range.1 as usize];
                buffer.insert_text(
                    &mut ctx.editor.word_database,
                    cursor.position,
                    text,
                    &mut ctx.editor.events,
                );
            }
        }
    } else {
        buffer_view.insert_text_at_cursor_positions(
            &mut ctx.editor.buffers,
            &mut ctx.editor.word_database,
            &text,
            &mut ctx.editor.events,
        );
    }

    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle)?;
    ctx.editor
        .buffers
        .get_mut(buffer_view.buffer_handle)?
        .commit_edits();
    None
}

fn find_char(ctx: &mut ModeContext, forward: bool) -> Option<()> {
    let state = &ctx.editor.mode.normal_state;
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

    let handle = ctx.clients.get(ctx.client_handle)?.buffer_view_handle()?;
    let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?;

    let count = state.count.max(1) as _;
    for cursor in &mut buffer_view.cursors.mut_guard()[..] {
        let (left_chars, right_chars) = buffer
            .content()
            .line_at(cursor.position.line_index as _)
            .chars_from(cursor.position.column_byte_index as _);

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
            cursor.position.column_byte_index = i as _;
            if next_ch {
                cursor.position.column_byte_index += c.len_utf8() as BufferPositionIndex;
            }

            if let CursorMovementKind::PositionAndAnchor = state.movement_kind {
                cursor.anchor = cursor.position;
            }
        }
    }

    None
}

fn move_to_search_match<F>(ctx: &mut ModeContext, index_selector: F) -> Option<()>
where
    F: FnOnce(usize, Result<usize, usize>) -> usize,
{
    NavigationHistory::save_client_snapshot(
        ctx.clients,
        ctx.client_handle,
        &mut ctx.editor.buffer_views,
    );

    let client = ctx.clients.get_mut(ctx.client_handle)?;
    let handle = client.buffer_view_handle()?;
    let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
    let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle)?;

    let mut search_ranges = buffer.search_ranges();
    if search_ranges.is_empty() {
        let search = ctx.editor.registers.get(SEARCH_REGISTER);
        if !search.is_empty() {
            buffer.set_search(search);
            search_ranges = buffer.search_ranges();
        }

        if search_ranges.is_empty() {
            ctx.editor
                .status_bar
                .write(MessageKind::Error)
                .str("no search result");
            return None;
        }
    }

    let state = &mut ctx.editor.mode.normal_state;
    let cursors = &mut buffer_view.cursors;

    let main_position = cursors.main_cursor().position;
    let search_result = search_ranges.binary_search_by_key(&main_position, |r| r.from);
    state.search_index = index_selector(search_ranges.len(), search_result);

    let mut cursors = cursors.mut_guard();
    let main_cursor = cursors.main_cursor();
    main_cursor.position = search_ranges[state.search_index].from;

    if let CursorMovementKind::PositionAndAnchor = ctx.editor.mode.normal_state.movement_kind {
        main_cursor.anchor = main_cursor.position;
    }

    None
}

fn search_word_or_move_to_it(
    ctx: &mut ModeContext,
    index_selector: fn(usize, Result<usize, usize>) -> usize,
) -> Option<()> {
    let state = &mut ctx.editor.mode.normal_state;
    let handle = ctx.clients.get(ctx.client_handle)?.buffer_view_handle()?;
    let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
    let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle)?;

    let main_cursor = &buffer_view.cursors.main_cursor();
    let main_position = main_cursor.position;
    let main_range = main_cursor.to_range();

    let valid_range = main_range.from.line_index == main_range.to.line_index
        && main_range.from.column_byte_index != main_range.to.column_byte_index;
    let search_ranges = buffer.search_ranges();
    let current_range_index = search_ranges.binary_search_by_key(&main_position, |r| r.from);

    if valid_range || search_ranges.is_empty() || current_range_index.is_err() {
        let (position, text) = if valid_range {
            let line = buffer
                .content()
                .line_at(main_position.line_index as _)
                .as_str();
            let text = &line[main_range.from.column_byte_index as usize
                ..main_range.to.column_byte_index as usize];
            (main_range.from, text)
        } else {
            let word = buffer.content().word_at(main_position);
            (word.position, word.text)
        };

        let mut cursors = buffer_view.cursors.mut_guard();
        cursors.clear();
        cursors.add(Cursor {
            anchor: position,
            position,
        });

        let register = ctx.editor.registers.get_mut(SEARCH_REGISTER);
        register.clear();
        register.push_str(text);

        buffer.set_search(register);

        drop(cursors);
        let main_position = buffer_view.cursors.main_cursor().position;
        state.search_index = match buffer
            .search_ranges()
            .binary_search_by_key(&main_position, |r| r.from)
        {
            Ok(i) => i,
            Err(i) => i,
        };
    } else {
        NavigationHistory::save_client_snapshot(
            ctx.clients,
            ctx.client_handle,
            &mut ctx.editor.buffer_views,
        );

        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
        let mut range_index = current_range_index;

        let mut cursors = buffer_view.cursors.mut_guard();
        for _ in 0..state.count.max(1) {
            let i = index_selector(search_ranges.len(), range_index);
            let range = search_ranges[i];
            range_index = Ok(i);

            cursors.add(Cursor {
                anchor: range.from,
                position: range.from,
            });
        }
    }

    ctx.editor.mode.normal_state.movement_kind = CursorMovementKind::PositionAndAnchor;
    None
}

fn move_to_diagnostic(ctx: &mut ModeContext, forward: bool) -> Option<()> {
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

    let handle = ctx.clients.get(ctx.client_handle)?.buffer_view_handle()?;
    let buffer_view = ctx.editor.buffer_views.get(handle)?;
    let main_position = buffer_view.cursors.main_cursor().position;

    let mut diagnostics = DirectedIter::new(
        ctx.editor
            .lsp
            .clients()
            .flat_map(|c| c.diagnostics().iter()),
        forward,
    );
    let mut next_diagnostic = None;

    for (path, buffer_handle, diagnostics) in &mut diagnostics {
        if buffer_handle != Some(buffer_view.buffer_handle) {
            continue;
        }

        if forward {
            for d in diagnostics.iter() {
                if d.range.from > main_position {
                    next_diagnostic = Some((path, buffer_handle, d.range.from));
                    break;
                }
            }
        } else {
            for d in diagnostics.iter().rev() {
                if d.range.from < main_position {
                    next_diagnostic = Some((path, buffer_handle, d.range.from));
                    break;
                }
            }
        }
        break;
    }

    fn select_diagnostic_position(
        diagnostics: &[lsp::Diagnostic],
        forward: bool,
    ) -> BufferPosition {
        if forward {
            diagnostics[0].range.from
        } else {
            diagnostics[diagnostics.len() - 1].range.from
        }
    }

    if let None = next_diagnostic {
        next_diagnostic = diagnostics
            .next()
            .map(|(p, h, d)| (p, h, select_diagnostic_position(d, forward)));
    }

    if let None = next_diagnostic {
        let mut iter = DirectedIter::new(
            ctx.editor
                .lsp
                .clients()
                .flat_map(|c| c.diagnostics().iter()),
            forward,
        );
        next_diagnostic = iter
            .next()
            .map(|(p, h, d)| (p, h, select_diagnostic_position(d, forward)));
    }

    drop(diagnostics);

    let (path, buffer_handle, position) = next_diagnostic?;
    let buffer_view_handle = match buffer_handle {
        Some(buffer_handle) => ctx
            .editor
            .buffer_views
            .buffer_view_handle_from_buffer_handle(ctx.client_handle, buffer_handle),
        None => {
            let path = path.to_str().unwrap_or("");
            let path = ctx.editor.string_pool.acquire_with(path);
            let handle = ctx
                .editor
                .buffer_view_handle_from_path(ctx.client_handle, Path::new(&path));
            ctx.editor.string_pool.release(path);
            handle
        }
    };

    NavigationHistory::save_client_snapshot(
        ctx.clients,
        ctx.client_handle,
        &mut ctx.editor.buffer_views,
    );

    let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle)?;
    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?;
    let position = buffer.content().saturate_position(position);

    let mut cursors = buffer_view.cursors.mut_guard();
    cursors.clear();
    cursors.add(Cursor {
        anchor: position,
        position,
    });

    drop(cursors);
    drop(buffer_view);

    ctx.editor.mode.normal_state.movement_kind = CursorMovementKind::PositionAndAnchor;
    ctx.clients
        .get_mut(ctx.client_handle)?
        .set_buffer_view_handle(Some(buffer_view_handle), &mut ctx.editor.events);

    None
}

