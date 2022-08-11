use crate::{
    buffer_position::BufferPositionIndex,
    buffer_view::CursorMovementKind,
    client::ClientHandle,
    command::CommandManager,
    cursor::{Cursor, CursorCollectionMutGuard},
    editor::{Editor, EditorContext, EditorFlow, KeysIterator},
    editor_utils::{readline_poll, LogKind, ReadLinePoll, REGISTER_INPUT, REGISTER_PROMPT},
    mode::{ModeKind, ModeState},
    navigation_history::NavigationHistory,
    pattern::Pattern,
};

pub struct State {
    pub on_client_keys:
        fn(&mut EditorContext, ClientHandle, &mut KeysIterator, ReadLinePoll) -> Option<EditorFlow>,
    previous_cursors: Vec<Cursor>,
    previous_main_cursor_index: usize,
    movement_kind: CursorMovementKind,
    continuation: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _, _| Some(EditorFlow::Continue),
            previous_cursors: Vec::new(),
            previous_main_cursor_index: 0,
            movement_kind: CursorMovementKind::PositionAndAnchor,
            continuation: String::new(),
        }
    }
}

impl ModeState for State {
    fn on_enter(editor: &mut Editor) {
        editor.registers.get_mut(REGISTER_INPUT).clear();
    }

    fn on_exit(editor: &mut Editor) {
        editor.mode.plugin_handle = None;
        editor.registers.get_mut(REGISTER_INPUT).clear();
    }

    fn on_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<EditorFlow> {
        let poll = readline_poll(
            ctx.editor.registers.get_mut(REGISTER_INPUT),
            &mut ctx.platform,
            &mut ctx.editor.string_pool,
            &ctx.editor.buffered_keys,
            keys,
        );
        let f = ctx.editor.mode.readline_state.on_client_keys;
        f(ctx, client_handle, keys, poll)
    }
}

pub mod search {
    use super::*;

    use crate::editor_utils::{REGISTER_INPUT, REGISTER_SEARCH};

    pub fn enter_mode(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        movement_kind: CursorMovementKind,
    ) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorFlow> {
            match poll {
                ReadLinePoll::Pending => {
                    update_search(ctx, client_handle);
                }
                ReadLinePoll::Submitted => {
                    let client = ctx.clients.get_mut(client_handle);
                    if let Some(buffer_view_handle) = client.buffer_view_handle() {
                        let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
                        let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);
                        let search_ranges = buffer.search_ranges();

                        if search_ranges.is_empty() {
                            restore_saved_position(ctx, client_handle);
                        } else {
                            let state = &ctx.editor.mode.readline_state;
                            let cursor_position = buffer_view.cursors.main_cursor().position;

                            {
                                let mut cursors = buffer_view.cursors.mut_guard();
                                cursors.clear();
                                for &cursor in &state.previous_cursors {
                                    cursors.add(cursor);
                                }
                                cursors.set_main_cursor_index(state.previous_main_cursor_index);
                            }

                            let cursor_anchor = match state.movement_kind {
                                CursorMovementKind::PositionAndAnchor => cursor_position,
                                CursorMovementKind::PositionOnly => {
                                    state.previous_cursors[state.previous_main_cursor_index].anchor
                                }
                            };

                            NavigationHistory::save_snapshot(client, &ctx.editor.buffer_views);
                            let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
                            {
                                let mut cursors = buffer_view.cursors.mut_guard();
                                cursors.clear();
                                cursors.add(Cursor {
                                    anchor: cursor_anchor,
                                    position: cursor_position,
                                });
                            }

                            ctx.editor.mode.normal_state.search_index = match search_ranges
                                .binary_search_by_key(&cursor_position, |r| r.from)
                            {
                                Ok(i) => i,
                                Err(i) => i,
                            };
                        }
                    }

                    let input = ctx
                        .editor
                        .string_pool
                        .acquire_with(ctx.editor.registers.get(REGISTER_INPUT));
                    ctx.editor.registers.set(REGISTER_SEARCH, &input);
                    ctx.editor.string_pool.release(input);

                    ctx.editor.enter_mode(ModeKind::default());
                }
                ReadLinePoll::Canceled => {
                    restore_saved_position(ctx, client_handle);
                    ctx.editor.enter_mode(ModeKind::default());
                }
            }

            Some(EditorFlow::Continue)
        }

        save_current_position(ctx, client_handle);
        ctx.editor.registers.set(REGISTER_PROMPT, "search:");
        update_search(ctx, client_handle);

        ctx.editor.mode.readline_state.movement_kind = movement_kind;
        ctx.editor.mode.readline_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    fn update_search(ctx: &mut EditorContext, client_handle: ClientHandle) {
        let handle = match ctx.clients.get_mut(client_handle).buffer_view_handle() {
            Some(handle) => handle,
            None => return,
        };
        let buffer_view = ctx.editor.buffer_views.get_mut(handle);
        let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle);

        let _ = ctx
            .editor
            .aux_pattern
            .compile_searcher(ctx.editor.registers.get(REGISTER_INPUT));
        buffer.set_search(&ctx.editor.aux_pattern);
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

        if let CursorMovementKind::PositionAndAnchor = ctx.editor.mode.normal_state.movement_kind {
            main_cursor.anchor = main_cursor.position;
        }
    }
}

fn on_submitted(
    ctx: &mut EditorContext,
    client_handle: ClientHandle,
    poll: ReadLinePoll,
    proc: fn(&mut EditorContext, ClientHandle),
) {
    match poll {
        ReadLinePoll::Pending => (),
        ReadLinePoll::Submitted => {
            proc(ctx, client_handle);
            ctx.editor.enter_mode(ModeKind::default());
        }
        ReadLinePoll::Canceled => ctx.editor.enter_mode(ModeKind::default()),
    }
}

pub mod filter_cursors {
    use super::*;

    use crate::{
        buffer::BufferContent, buffer_position::BufferRange, cursor::Cursor,
        editor_utils::REGISTER_SEARCH,
    };

    pub fn enter_filter_mode(ctx: &mut EditorContext) {
        ctx.editor.registers.set(REGISTER_PROMPT, "filter:");
        ctx.editor.mode.readline_state.on_client_keys = |ctx, client_handle, _, poll| {
            on_submitted(ctx, client_handle, poll, |ctx, client_handle| {
                on_event_impl(ctx, client_handle, true);
            });
            Some(EditorFlow::Continue)
        };
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    pub fn enter_except_mode(ctx: &mut EditorContext) {
        ctx.editor.registers.set(REGISTER_PROMPT, "except:");
        ctx.editor.mode.readline_state.on_client_keys = |ctx, client_handle, _, poll| {
            on_submitted(ctx, client_handle, poll, |ctx, client_handle| {
                on_event_impl(ctx, client_handle, false);
            });
            Some(EditorFlow::Continue)
        };
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    fn on_event_impl(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        keep_if_contains_pattern: bool,
    ) {
        fn range_contains_pattern(
            buffer: &BufferContent,
            range: BufferRange,
            pattern: &Pattern,
        ) -> bool {
            fn contains(selection: &str, pattern: &Pattern, anchor: Option<char>) -> bool {
                pattern.match_indices(selection, anchor).next().is_some()
            }

            let search_anchor = pattern.search_anchor();
            if range.from.line_index == range.to.line_index {
                let selection = &buffer.lines()[range.from.line_index as usize].as_str()
                    [range.from.column_byte_index as usize..range.to.column_byte_index as usize];
                contains(selection, pattern, search_anchor)
            } else {
                let selection = &buffer.lines()[range.from.line_index as usize].as_str()
                    [range.from.column_byte_index as usize..];
                if contains(selection, pattern, search_anchor) {
                    return true;
                }

                for line_index in (range.from.line_index + 1)..range.to.line_index {
                    let selection = buffer.lines()[line_index as usize].as_str();
                    if contains(selection, pattern, search_anchor) {
                        return true;
                    }
                }

                let selection = &buffer.lines()[range.to.line_index as usize].as_str()
                    [..range.to.column_byte_index as usize];
                contains(selection, pattern, search_anchor)
            }
        }

        let pattern = ctx.editor.registers.get(REGISTER_INPUT);
        let pattern = if pattern.is_empty() {
            ctx.editor.registers.get(REGISTER_SEARCH)
        } else {
            pattern
        };

        if let Err(error) = ctx.editor.aux_pattern.compile_searcher(pattern) {
            ctx.editor
                .logger
                .write(LogKind::Error)
                .fmt(format_args!("{}", error));
            return;
        }

        let handle = match ctx.clients.get(client_handle).buffer_view_handle() {
            Some(handle) => handle,
            None => return,
        };
        let buffer_view = ctx.editor.buffer_views.get_mut(handle);
        let buffer = ctx
            .editor
            .buffers
            .get_mut(buffer_view.buffer_handle)
            .content();

        let mut cursors = buffer_view.cursors.mut_guard();
        let main_cursor_position = cursors.main_cursor().position;

        for i in (0..cursors[..].len()).rev() {
            let range = cursors[i].to_range();
            if range_contains_pattern(buffer, range, &ctx.editor.aux_pattern)
                != keep_if_contains_pattern
            {
                cursors.swap_remove(i);
            }
        }

        if cursors[..].is_empty() {
            cursors.add(Cursor {
                anchor: main_cursor_position,
                position: main_cursor_position,
            });
        }

        cursors.set_main_cursor_near_position(main_cursor_position);
    }
}

pub mod split_cursors {
    use super::*;

    use crate::{buffer_position::BufferPosition, cursor::Cursor, editor_utils::{REGISTER_SEARCH, REGISTER_INPUT}};

    pub fn enter_by_pattern_mode(ctx: &mut EditorContext) {
        fn add_matches(
            cursors: &mut CursorCollectionMutGuard,
            line: &str,
            pattern: &Pattern,
            start_position: BufferPosition,
        ) {
            let search_anchor = pattern.search_anchor();
            for range in pattern.match_indices(line, search_anchor) {
                let mut anchor = start_position;
                anchor.column_byte_index += range.start as BufferPositionIndex;
                let mut position = start_position;
                position.column_byte_index += range.end as BufferPositionIndex;

                cursors.add(Cursor { anchor, position });
            }
        }

        ctx.editor.registers.set(REGISTER_PROMPT, "split-by:");
        ctx.editor.mode.readline_state.on_client_keys = |ctx, client_handle, _, poll| {
            on_submitted(ctx, client_handle, poll, |ctx, client_handle| {
                on_event_impl(ctx, client_handle, add_matches);
            });
            Some(EditorFlow::Continue)
        };
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    pub fn enter_by_separators_mode(ctx: &mut EditorContext) {
        fn add_matches(
            cursors: &mut CursorCollectionMutGuard,
            line: &str,
            pattern: &Pattern,
            start_position: BufferPosition,
        ) {
            let search_anchor = pattern.search_anchor();
            let mut index = 0;
            for range in pattern.match_indices(line, search_anchor) {
                if index != range.start {
                    let mut anchor = start_position;
                    anchor.column_byte_index += index as BufferPositionIndex;
                    let mut position = start_position;
                    position.column_byte_index += range.start as BufferPositionIndex;

                    cursors.add(Cursor { anchor, position });
                }

                index = range.end;
            }

            if index < line.len() {
                cursors.add(Cursor {
                    anchor: BufferPosition::line_col(
                        start_position.line_index,
                        start_position.column_byte_index + index as BufferPositionIndex,
                    ),
                    position: BufferPosition::line_col(
                        start_position.line_index,
                        start_position.column_byte_index + line.len() as BufferPositionIndex,
                    ),
                });
            }
        }

        ctx.editor.registers.set(REGISTER_PROMPT, "split-on:");
        ctx.editor.mode.readline_state.on_client_keys = |ctx, client_handle, _, poll| {
            on_submitted(ctx, client_handle, poll, |ctx, client_handle| {
                on_event_impl(ctx, client_handle, add_matches);
            });
            Some(EditorFlow::Continue)
        };
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    fn on_event_impl(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        add_matches: fn(&mut CursorCollectionMutGuard, &str, &Pattern, BufferPosition),
    ) {
        let pattern = ctx.editor.registers.get(REGISTER_INPUT);
        let pattern = if pattern.is_empty() {
            ctx.editor.registers.get(REGISTER_SEARCH)
        } else {
            pattern
        };

        if let Err(error) = ctx.editor.aux_pattern.compile_searcher(pattern) {
            ctx.editor
                .logger
                .write(LogKind::Error)
                .fmt(format_args!("{}", error));
            return;
        }

        let handle = match ctx.clients.get(client_handle).buffer_view_handle() {
            Some(handle) => handle,
            None => return,
        };
        let buffer_view = ctx.editor.buffer_views.get_mut(handle);
        let buffer = ctx
            .editor
            .buffers
            .get_mut(buffer_view.buffer_handle)
            .content();

        let mut cursors = buffer_view.cursors.mut_guard();
        let main_cursor_position = cursors.main_cursor().position;

        let mut i = cursors[..].len() - 1;
        loop {
            let cursor = cursors.swap_remove(i);
            let range = cursor.to_range();
            let new_cursors_start_index = cursors[..].len();

            let main_cursor_index = cursors.main_cursor_index();

            if range.from.line_index == range.to.line_index {
                let line = &buffer.lines()[range.from.line_index as usize].as_str()
                    [range.from.column_byte_index as usize..range.to.column_byte_index as usize];
                add_matches(&mut cursors, line, &ctx.editor.aux_pattern, range.from);
            } else {
                let line = &buffer.lines()[range.from.line_index as usize].as_str()
                    [range.from.column_byte_index as usize..];
                add_matches(&mut cursors, line, &ctx.editor.aux_pattern, range.from);

                for line_index in (range.from.line_index + 1)..range.to.line_index {
                    let line = buffer.lines()[line_index as usize].as_str();
                    add_matches(
                        &mut cursors,
                        line,
                        &ctx.editor.aux_pattern,
                        BufferPosition::line_col(line_index, 0),
                    );
                }

                let line = &buffer.lines()[range.to.line_index as usize].as_str()
                    [..range.to.column_byte_index as usize];
                add_matches(
                    &mut cursors,
                    line,
                    &ctx.editor.aux_pattern,
                    BufferPosition::line_col(range.to.line_index, 0),
                );
            }

            if cursor.position == range.from {
                for cursor in &mut cursors[new_cursors_start_index..] {
                    std::mem::swap(&mut cursor.anchor, &mut cursor.position);
                }
            }

            if i < main_cursor_index {
                let new_cursor_count = cursors[..].len() - new_cursors_start_index;
                cursors.set_main_cursor_index(main_cursor_index + new_cursor_count);
            }

            if i == 0 {
                break;
            }
            i -= 1;
        }

        if cursors[..].is_empty() {
            cursors.add(Cursor {
                anchor: main_cursor_position,
                position: main_cursor_position,
            });
        }

        cursors.set_main_cursor_near_position(main_cursor_position);
    }
}

pub mod goto {
    use super::*;

    use crate::{buffer_position::BufferPosition, cursor::Cursor, word_database::WordKind};

    pub fn enter_mode(ctx: &mut EditorContext, client_handle: ClientHandle) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorFlow> {
            match poll {
                ReadLinePoll::Pending => {
                    let line_number: usize = match ctx.editor.registers.get(REGISTER_INPUT).parse() {
                        Ok(number) => number,
                        Err(_) => return Some(EditorFlow::Continue),
                    };
                    let line_index = line_number.saturating_sub(1);

                    let handle = ctx.clients.get(client_handle).buffer_view_handle()?;
                    let buffer_view = ctx.editor.buffer_views.get_mut(handle);
                    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);
                    let buffer = buffer.content();

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
                }
                ReadLinePoll::Submitted => ctx.editor.enter_mode(ModeKind::default()),
                ReadLinePoll::Canceled => {
                    restore_saved_position(ctx, client_handle);
                    ctx.editor.enter_mode(ModeKind::default());
                }
            }
            Some(EditorFlow::Continue)
        }

        save_current_position(ctx, client_handle);
        ctx.editor.registers.set(REGISTER_PROMPT, "goto-line:");
        ctx.editor.mode.readline_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }
}

pub mod custom {
    use super::*;

    pub fn enter_mode(ctx: &mut EditorContext, continuation: &str) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorFlow> {
            match poll {
                ReadLinePoll::Pending => (),
                ReadLinePoll::Submitted => {
                    let continuation = &ctx.editor.mode.readline_state.continuation;
                    let continuation = ctx.editor.string_pool.acquire_with(continuation);
                    let result = CommandManager::eval(
                        ctx,
                        Some(client_handle),
                        "readline-continuation",
                        &continuation,
                    );
                    let flow = CommandManager::unwrap_eval_result(ctx, result);
                    ctx.editor.string_pool.release(continuation);
                    ctx.editor.enter_mode(ModeKind::default());
                    return Some(flow);
                }
                ReadLinePoll::Canceled => ctx.editor.enter_mode(ModeKind::default()),
            }
            Some(EditorFlow::Continue)
        }

        let state = &mut ctx.editor.mode.readline_state;
        state.on_client_keys = on_client_keys;
        state.continuation.clear();
        state.continuation.push_str(continuation);
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }
}

fn save_current_position(ctx: &mut EditorContext, client_handle: ClientHandle) {
    let buffer_view_handle = match ctx.clients.get(client_handle).buffer_view_handle() {
        Some(handle) => handle,
        None => return,
    };
    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
    let state = &mut ctx.editor.mode.readline_state;
    state.previous_cursors.clear();
    for &cursor in &buffer_view.cursors[..] {
        state.previous_cursors.push(cursor);
    }
    state.previous_main_cursor_index = buffer_view.cursors.main_cursor_index();
}

fn restore_saved_position(ctx: &mut EditorContext, client_handle: ClientHandle) {
    let buffer_view_handle = match ctx.clients.get(client_handle).buffer_view_handle() {
        Some(handle) => handle,
        None => return,
    };
    let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
    let mut cursors = buffer_view.cursors.mut_guard();
    cursors.clear();

    let state = &ctx.editor.mode.readline_state;
    for &cursor in &state.previous_cursors {
        cursors.add(cursor);
    }
    cursors.set_main_cursor_index(state.previous_main_cursor_index);
}

