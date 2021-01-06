use crate::{
    editor::{KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    register::SEARCH_REGISTER,
};

pub struct State {
    on_client_keys: fn(&mut ModeContext, &mut KeysIterator, ReadLinePoll),
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _| (),
        }
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.read_line.set_input("");
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.read_line.set_input("");
    }

    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        let poll = ctx.read_line.poll(keys);
        let func = ctx.mode.read_line_state.on_client_keys;
        func(ctx, keys, poll);
        ModeOperation::None
    }
}

pub mod search {
    use super::*;

    use crate::navigation_history::{NavigationDirection, NavigationHistory};

    pub fn enter_mode(ctx: &mut ModeContext) {
        fn on_client_keys(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
            match poll {
                ReadLinePoll::Pending => {
                    update_search(ctx);
                }
                ReadLinePoll::Submitted => {
                    ctx.registers.set(SEARCH_REGISTER, ctx.read_line.input());
                    Mode::change_to(ctx, ModeKind::default());
                }
                ReadLinePoll::Canceled => {
                    NavigationHistory::move_in_history(
                        ctx.clients,
                        ctx.buffer_views,
                        ctx.target_client,
                        NavigationDirection::Backward,
                    );
                    Mode::change_to(ctx, ModeKind::default());
                }
            }
        }

        NavigationHistory::save_client_snapshot(ctx.clients, ctx.buffer_views, ctx.target_client);
        ctx.read_line.set_prompt("search:");
        update_search(ctx);

        ctx.mode.read_line_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    fn update_search(ctx: &mut ModeContext) {
        for buffer in ctx.buffers.iter_mut() {
            buffer.set_search("");
        }

        let client = unwrap_or_return!(ctx.clients.get_mut(ctx.target_client));
        let handle = unwrap_or_return!(client.current_buffer_view_handle());
        let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
        let buffer = unwrap_or_return!(ctx.buffers.get_mut(buffer_view.buffer_handle));
        buffer.set_search(&ctx.read_line.input());
        let search_ranges = buffer.search_ranges();

        if search_ranges.is_empty() {
            return;
        }

        let mut cursors = buffer_view.cursors.mut_guard();
        let main_cursor = cursors.main_cursor();
        match search_ranges.binary_search_by_key(&main_cursor.position, |r| r.from) {
            Ok(i) => main_cursor.position = search_ranges[i].from,
            Err(0) => main_cursor.position = search_ranges[0].from,
            Err(i) => {
                if i == search_ranges.len() {
                    main_cursor.position = search_ranges[search_ranges.len() - 1].from;
                } else {
                    let before = search_ranges[i - 1].from;
                    let after = search_ranges[i].from;

                    let main_line_index = main_cursor.position.line_index;
                    if main_line_index - before.line_index < after.line_index - main_line_index {
                        main_cursor.position = before;
                    } else {
                        main_cursor.position = after;
                    }
                }
            }
        }

        main_cursor.anchor = main_cursor.position;

        let main_line_index = main_cursor.position.line_index;
        let height = client.height as usize;
        if main_line_index < client.scroll || main_line_index >= client.scroll + height {
            client.scroll = main_line_index.saturating_sub(height / 2);
        }
    }
}

macro_rules! on_submitted {
    ($ctx:expr, $poll:expr => $value:expr) => {
        match $poll {
            ReadLinePoll::Pending => (),
            ReadLinePoll::Submitted => {
                $value;
                Mode::change_to($ctx, ModeKind::default());
            }
            ReadLinePoll::Canceled => Mode::change_to($ctx, ModeKind::default()),
        }
    };
}

pub mod filter_cursors {
    use super::*;

    use crate::{buffer::BufferContent, buffer_position::BufferRange, cursor::Cursor};

    pub fn enter_filter_mode(ctx: &mut ModeContext) {
        ctx.read_line.set_prompt("filter:");
        ctx.mode.read_line_state.on_client_keys =
            |ctx, _, poll| on_submitted!(ctx, poll => on_event_impl(ctx, true));
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    pub fn enter_except_mode(ctx: &mut ModeContext) {
        ctx.read_line.set_prompt("except:");
        ctx.mode.read_line_state.on_client_keys =
            |ctx, _, poll| on_submitted!(ctx, poll => on_event_impl(ctx, false));
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    fn on_event_impl(ctx: &mut ModeContext, keep_if_contains_pattern: bool) {
        fn range_contains_pattern(
            buffer: &BufferContent,
            range: BufferRange,
            pattern: &str,
        ) -> bool {
            if range.from.line_index == range.to.line_index {
                let line = &buffer.line_at(range.from.line_index).as_str()
                    [range.from.column_byte_index..range.to.column_byte_index];
                line.contains(pattern)
            } else {
                let line =
                    &buffer.line_at(range.from.line_index).as_str()[range.from.column_byte_index..];
                if line.contains(pattern) {
                    return true;
                }

                for line_index in (range.from.line_index + 1)..range.to.line_index {
                    let line = buffer.line_at(line_index).as_str();
                    if line.contains(pattern) {
                        return true;
                    }
                }

                let line =
                    &buffer.line_at(range.to.line_index).as_str()[..range.to.column_byte_index];
                line.contains(pattern)
            }
        }

        let pattern = ctx.read_line.input();
        let pattern = if pattern.is_empty() {
            ctx.registers.get(SEARCH_REGISTER)
        } else {
            pattern
        };

        let handle = unwrap_or_return!(ctx.current_buffer_view_handle());
        let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
        let buffer = unwrap_or_return!(ctx.buffers.get_mut(buffer_view.buffer_handle)).content();

        let mut cursors = buffer_view.cursors.mut_guard();
        let main_cursor_position = cursors.main_cursor().position;
        let cursor_count = cursors[..].len();

        for i in 0..cursor_count {
            let cursor = cursors[i];
            if range_contains_pattern(buffer, cursor.as_range(), pattern)
                == keep_if_contains_pattern
            {
                cursors.add(cursor);
            }
        }

        cursors.remove_range(..cursor_count);

        if cursors[..].is_empty() {
            cursors.add(Cursor {
                anchor: main_cursor_position,
                position: main_cursor_position,
            });
        }
    }
}

pub mod split_cursors {
    use super::*;

    use crate::{
        buffer_position::BufferPosition,
        cursor::{Cursor, CursorCollectionMutGuard},
    };

    pub fn enter_by_pattern_mode(ctx: &mut ModeContext) {
        fn add_matches(
            cursors: &mut CursorCollectionMutGuard,
            line: &str,
            pattern: &str,
            start_position: BufferPosition,
        ) {
            for (index, s) in line.match_indices(pattern) {
                let mut from = start_position;
                from.column_byte_index += index;
                let mut to = from;
                to.column_byte_index += s.len();

                cursors.add(Cursor {
                    anchor: from,
                    position: to,
                });
            }
        }

        ctx.read_line.set_prompt("split-by:");
        ctx.mode.read_line_state.on_client_keys =
            |ctx, _, poll| on_submitted!(ctx, poll => on_event_impl(ctx, add_matches));
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    pub fn enter_by_separators_mode(ctx: &mut ModeContext) {
        fn add_matches(
            cursors: &mut CursorCollectionMutGuard,
            line: &str,
            pattern: &str,
            start_position: BufferPosition,
        ) {
            let mut index = start_position.column_byte_index;
            for (i, s) in line.match_indices(pattern) {
                let i = i + start_position.column_byte_index;
                if index != i {
                    cursors.add(Cursor {
                        anchor: BufferPosition::line_col(start_position.line_index, index),
                        position: BufferPosition::line_col(start_position.line_index, i),
                    });
                }

                index = i + s.len();
            }

            if index != start_position.column_byte_index + line.len() {
                cursors.add(Cursor {
                    anchor: BufferPosition::line_col(start_position.line_index, index),
                    position: BufferPosition::line_col(start_position.line_index, line.len()),
                });
            }
        }

        ctx.read_line.set_prompt("split-on:");
        ctx.mode.read_line_state.on_client_keys =
            |ctx, _, poll| on_submitted!(ctx, poll => on_event_impl(ctx, add_matches));
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    fn on_event_impl(
        ctx: &mut ModeContext,
        add_matches: fn(&mut CursorCollectionMutGuard, &str, &str, BufferPosition),
    ) {
        let pattern = ctx.read_line.input();
        let pattern = if pattern.is_empty() {
            ctx.registers.get(SEARCH_REGISTER)
        } else {
            pattern
        };

        let handle = unwrap_or_return!(ctx.current_buffer_view_handle());
        let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
        let buffer = unwrap_or_return!(ctx.buffers.get_mut(buffer_view.buffer_handle)).content();

        let mut cursors = buffer_view.cursors.mut_guard();
        let main_cursor_position = cursors.main_cursor().position;
        let cursor_count = cursors[..].len();

        for i in 0..cursor_count {
            let cursor = cursors[i];
            let range = cursor.as_range();

            if range.from.line_index == range.to.line_index {
                let line = &buffer.line_at(range.from.line_index).as_str()
                    [range.from.column_byte_index..range.to.column_byte_index];
                add_matches(&mut cursors, line, pattern, range.from);
            } else {
                let line =
                    &buffer.line_at(range.from.line_index).as_str()[range.from.column_byte_index..];
                add_matches(&mut cursors, line, pattern, range.from);

                for line_index in (range.from.line_index + 1)..range.to.line_index {
                    let line = buffer.line_at(line_index).as_str();
                    add_matches(
                        &mut cursors,
                        line,
                        pattern,
                        BufferPosition::line_col(line_index, 0),
                    );
                }

                let line =
                    &buffer.line_at(range.to.line_index).as_str()[..range.to.column_byte_index];
                add_matches(
                    &mut cursors,
                    line,
                    pattern,
                    BufferPosition::line_col(range.to.line_index, 0),
                );
            }

            if cursor.position == range.from {
                for cursor in &mut cursors[cursor_count..] {
                    std::mem::swap(&mut cursor.anchor, &mut cursor.position);
                }
            }
        }

        cursors.remove_range(..cursor_count);

        if cursors[..].is_empty() {
            cursors.add(Cursor {
                anchor: main_cursor_position,
                position: main_cursor_position,
            });
        }
    }
}

pub mod goto {
    use super::*;

    use crate::{
        buffer_position::BufferPosition,
        cursor::Cursor,
        navigation_history::{NavigationDirection, NavigationHistory},
        word_database::WordKind,
    };

    pub fn enter_mode(ctx: &mut ModeContext) {
        fn on_client_keys(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
            match poll {
                ReadLinePoll::Pending => {
                    let line_number: usize = match ctx.read_line.input().parse() {
                        Ok(number) => number,
                        Err(_) => return,
                    };
                    let line_index = line_number.saturating_sub(1);

                    let handle = unwrap_or_return!(ctx.current_buffer_view_handle());
                    let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
                    let buffer = unwrap_or_return!(ctx.buffers.get(buffer_view.buffer_handle));

                    let mut position = BufferPosition::line_col(line_index, 0);
                    let (first_word, _, mut right_words) = buffer.content().words_from(position);
                    if first_word.kind == WordKind::Whitespace {
                        if let Some(word) = right_words.next() {
                            position = word.position;
                        }
                    }

                    let mut cursors = buffer_view.cursors.mut_guard();
                    cursors.clear();
                    cursors.add(Cursor {
                        anchor: position,
                        position,
                    });
                }
                ReadLinePoll::Submitted => Mode::change_to(ctx, ModeKind::default()),
                ReadLinePoll::Canceled => {
                    NavigationHistory::move_in_history(
                        ctx.clients,
                        ctx.buffer_views,
                        ctx.target_client,
                        NavigationDirection::Backward,
                    );
                    Mode::change_to(ctx, ModeKind::default());
                }
            }
        }

        NavigationHistory::save_client_snapshot(ctx.clients, ctx.buffer_views, ctx.target_client);
        ctx.read_line.set_prompt("goto-line:");
        ctx.mode.read_line_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::ReadLine);
    }
}

pub mod custom {
    use super::*;

    use crate::script::{ScriptCallback, ScriptContext, ScriptValue};

    pub fn enter_mode(ctx: &mut ScriptContext, callback: ScriptCallback) {
        fn on_client_keys(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
            let previous_mode_kind = ctx.mode.kind();
            let (engine, mut script_ctx) = ctx.into_script_context();
            let result = engine.as_ref_with_ctx(&mut script_ctx, |engine, ctx, guard| {
                let input = match poll {
                    ReadLinePoll::Pending => return Ok(()),
                    ReadLinePoll::Submitted => {
                        ScriptValue::String(engine.create_string(ctx.read_line.input().as_bytes())?)
                    }
                    ReadLinePoll::Canceled => ScriptValue::Nil,
                };

                if let Some(callback) = ctx.script_callbacks.read_line.take() {
                    callback.call(engine, &guard, input)?;
                    callback.dispose(engine)?;
                }

                Ok(())
            });

            match result {
                Ok(()) => {
                    if ctx.mode.kind() == previous_mode_kind {
                        Mode::change_to(ctx, ModeKind::default());
                    }
                }
                Err(error) => {
                    ctx.status_message.write_error(&error);
                    Mode::change_to(ctx, ModeKind::default());
                }
            }
        }

        ctx.script_callbacks.read_line = Some(callback);
        ctx.mode.read_line_state.on_client_keys = on_client_keys;
        //Mode::change_to(ctx, ModeKind::ReadLine);
    }
}
