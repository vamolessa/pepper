use crate::{
    command::CommandManager,
    editor::KeysIterator,
    editor_utils::ReadLinePoll,
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    register::SEARCH_REGISTER,
};

pub struct State {
    on_client_keys: fn(&mut ModeContext, &mut KeysIterator, ReadLinePoll) -> Option<ModeOperation>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _| None,
        }
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.editor.read_line.input_mut().clear();
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.editor.read_line.input_mut().clear();
    }

    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<ModeOperation> {
        let poll = ctx
            .editor
            .read_line
            .poll(ctx.platform, &ctx.editor.buffered_keys, keys);
        let func = ctx.editor.mode.read_line_state.on_client_keys;
        func(ctx, keys, poll)
    }
}

pub mod search {
    use super::*;

    use crate::navigation_history::{NavigationDirection, NavigationHistory};

    pub fn enter_mode(ctx: &mut ModeContext) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => {
                    update_search(ctx);
                }
                ReadLinePoll::Submitted => {
                    let register = ctx.editor.registers.get_mut(SEARCH_REGISTER);
                    register.clear();
                    register.push_str(ctx.editor.read_line.input());
                    Mode::change_to(ctx, ModeKind::default());
                }
                ReadLinePoll::Canceled => {
                    NavigationHistory::move_in_history(
                        ctx.editor,
                        ctx.clients,
                        ctx.client_handle,
                        NavigationDirection::Backward,
                    );
                    Mode::change_to(ctx, ModeKind::default());
                }
            }

            None
        }

        NavigationHistory::save_client_snapshot(
            ctx.clients,
            ctx.client_handle,
            &mut ctx.editor.buffer_views,
        );
        ctx.editor.read_line.set_prompt("search:");
        update_search(ctx);

        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    fn update_search(ctx: &mut ModeContext) -> Option<()> {
        for buffer in ctx.editor.buffers.iter_mut() {
            buffer.set_search("");
        }

        let client = ctx.clients.get_mut(ctx.client_handle)?;
        let handle = client.buffer_view_handle()?;
        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
        let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle)?;
        buffer.set_search(&ctx.editor.read_line.input());
        let search_ranges = buffer.search_ranges();

        if search_ranges.is_empty() {
            return None;
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

        None
    }
}

#[inline]
fn on_submitted(ctx: &mut ModeContext, poll: ReadLinePoll, proc: fn(&mut ModeContext)) {
    match poll {
        ReadLinePoll::Pending => (),
        ReadLinePoll::Submitted => {
            proc(ctx);
            Mode::change_to(ctx, ModeKind::default());
        }
        ReadLinePoll::Canceled => Mode::change_to(ctx, ModeKind::default()),
    }
}

pub mod filter_cursors {
    use super::*;

    use crate::{buffer::BufferContent, buffer_position::BufferRange, cursor::Cursor};

    pub fn enter_filter_mode(ctx: &mut ModeContext) {
        ctx.editor.read_line.set_prompt("filter:");
        ctx.editor.mode.read_line_state.on_client_keys = |ctx, _, poll| {
            on_submitted(ctx, poll, |ctx| {
                on_event_impl(ctx, true);
            });
            None
        };
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    pub fn enter_except_mode(ctx: &mut ModeContext) {
        ctx.editor.read_line.set_prompt("except:");
        ctx.editor.mode.read_line_state.on_client_keys = |ctx, _, poll| {
            on_submitted(ctx, poll, |ctx| {
                on_event_impl(ctx, false);
            });
            None
        };
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    fn on_event_impl(ctx: &mut ModeContext, keep_if_contains_pattern: bool) -> Option<()> {
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

        let pattern = ctx.editor.read_line.input();
        let pattern = if pattern.is_empty() {
            ctx.editor.registers.get(SEARCH_REGISTER)
        } else {
            pattern
        };

        let handle = ctx.clients.get(ctx.client_handle)?.buffer_view_handle()?;
        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
        let buffer = ctx
            .editor
            .buffers
            .get_mut(buffer_view.buffer_handle)?
            .content();

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

        None
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

        ctx.editor.read_line.set_prompt("split-by:");
        ctx.editor.mode.read_line_state.on_client_keys = |ctx, _, poll| {
            on_submitted(ctx, poll, |ctx| {
                on_event_impl(ctx, add_matches);
            });
            None
        };
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

        ctx.editor.read_line.set_prompt("split-on:");
        ctx.editor.mode.read_line_state.on_client_keys = |ctx, _, poll| {
            on_submitted(ctx, poll, |ctx| {
                on_event_impl(ctx, add_matches);
            });
            None
        };
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    fn on_event_impl(
        ctx: &mut ModeContext,
        add_matches: fn(&mut CursorCollectionMutGuard, &str, &str, BufferPosition),
    ) -> Option<()> {
        let pattern = ctx.editor.read_line.input();
        let pattern = if pattern.is_empty() {
            ctx.editor.registers.get(SEARCH_REGISTER)
        } else {
            pattern
        };

        let handle = ctx.clients.get(ctx.client_handle)?.buffer_view_handle()?;
        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
        let buffer = ctx
            .editor
            .buffers
            .get_mut(buffer_view.buffer_handle)?
            .content();

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
        None
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
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => {
                    let line_number: usize = match ctx.editor.read_line.input().parse() {
                        Ok(number) => number,
                        Err(_) => return None,
                    };
                    let line_index = line_number.saturating_sub(1);

                    let handle = ctx.clients.get(ctx.client_handle)?.buffer_view_handle()?;
                    let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
                    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle)?;

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
                        ctx.editor,
                        ctx.clients,
                        ctx.client_handle,
                        NavigationDirection::Backward,
                    );
                    Mode::change_to(ctx, ModeKind::default());
                }
            }
            None
        }

        NavigationHistory::save_client_snapshot(
            ctx.clients,
            ctx.client_handle,
            &mut ctx.editor.buffer_views,
        );
        ctx.editor.read_line.set_prompt("goto-line:");
        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::ReadLine);
    }
}

pub mod custom {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => None,
                ReadLinePoll::Submitted => {
                    let continuation = std::mem::take(&mut ctx.editor.commands.continuation);
                    let operation = CommandManager::eval_body_and_print(
                        ctx.editor,
                        ctx.platform,
                        ctx.clients,
                        Some(ctx.client_handle),
                        &continuation,
                    )
                    .map(Into::into);
                    ctx.editor.string_pool.release(continuation);

                    Mode::change_to(ctx, ModeKind::default());
                    operation
                }
                ReadLinePoll::Canceled => {
                    Mode::change_to(ctx, ModeKind::default());
                    None
                }
            }
        }

        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::ReadLine);
    }
}
