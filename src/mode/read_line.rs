use crate::{
    buffer_position::BufferPositionIndex,
    buffer_view::CursorMovementKind,
    client::Client,
    cursor::CursorCollection,
    editor::KeysIterator,
    editor_utils::{parse_process_command, MessageKind, ReadLinePoll},
    lsp,
    mode::{Mode, ModeContext, ModeKind, ModeOperation, ModeState},
    pattern::Pattern,
    platform::SharedBuf,
};

pub struct State {
    on_client_keys: fn(&mut ModeContext, &mut KeysIterator, ReadLinePoll) -> Option<ModeOperation>,
    lsp_client_handle: Option<lsp::ClientHandle>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _| None,
            lsp_client_handle: None,
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
        let poll = ctx.editor.read_line.poll(
            ctx.platform,
            &mut ctx.editor.string_pool,
            &ctx.editor.buffered_keys,
            keys,
        );
        let func = ctx.editor.mode.read_line_state.on_client_keys;
        func(ctx, keys, poll)
    }
}

pub mod search {
    use super::*;

    use crate::{
        navigation_history::{NavigationMovement, NavigationHistory},
        register::SEARCH_REGISTER,
    };

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
                    if let Some(buffer_view) = ctx
                        .clients
                        .get(ctx.client_handle)
                        .and_then(Client::buffer_view_handle)
                        .and_then(|h| ctx.editor.buffer_views.get(h))
                    {
                        if let Some(buffer) = ctx.editor.buffers.get(buffer_view.buffer_handle) {
                            let search_ranges = buffer.search_ranges();
                            if search_ranges.is_empty() {
                                if let Some(client) = ctx.clients.get_mut(ctx.client_handle) {
                                    NavigationHistory::move_in_history(
                                        client,
                                        ctx.editor,
                                        NavigationMovement::Backward,
                                    );
                                }
                            } else {
                                let position = buffer_view.cursors.main_cursor().position;
                                ctx.editor.mode.normal_state.search_index = match search_ranges
                                    .binary_search_by_key(&position, |r| r.from)
                                {
                                    Ok(i) => i,
                                    Err(i) => i,
                                };
                            }
                        }
                    }

                    let register = ctx.editor.registers.get_mut(SEARCH_REGISTER);
                    register.clear();
                    register.push_str(ctx.editor.read_line.input());
                    Mode::change_to(ctx, ModeKind::default());
                }
                ReadLinePoll::Canceled => {
                    if let Some(client) = ctx.clients.get_mut(ctx.client_handle) {
                        NavigationHistory::move_in_history(
                            client,
                            ctx.editor,
                            NavigationMovement::Backward,
                        );
                    }
                    Mode::change_to(ctx, ModeKind::default());
                }
            }

            None
        }

        if let Some(client) = ctx.clients.get_mut(ctx.client_handle) {
            NavigationHistory::save_client_snapshot(client, &ctx.editor.buffer_views);
        }
        ctx.editor.read_line.set_prompt("search:");
        update_search(ctx);

        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    fn update_search(ctx: &mut ModeContext) -> Option<()> {
        ctx.editor.aux_pattern.clear();
        for buffer in ctx.editor.buffers.iter_mut() {
            buffer.set_search(&ctx.editor.aux_pattern);
        }

        let client = ctx.clients.get_mut(ctx.client_handle)?;
        let handle = client.buffer_view_handle()?;
        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
        let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle)?;

        let _ = ctx
            .editor
            .aux_pattern
            .compile_searcher(&ctx.editor.read_line.input());
        buffer.set_search(&ctx.editor.aux_pattern);
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

        if let CursorMovementKind::PositionAndAnchor = ctx.editor.mode.normal_state.movement_kind {
            main_cursor.anchor = main_cursor.position;
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

    use crate::{
        buffer::BufferContent, buffer_position::BufferRange, cursor::Cursor,
        register::SEARCH_REGISTER,
    };

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
            pattern: &Pattern,
        ) -> bool {
            #[inline]
            fn contains(selection: &str, pattern: &Pattern, anchor: Option<char>) -> bool {
                pattern.match_indices(selection, anchor).next().is_some()
            }

            let search_anchor = pattern.search_anchor();
            if range.from.line_index == range.to.line_index {
                let selection = &buffer.line_at(range.from.line_index as _).as_str()
                    [range.from.column_byte_index as usize..range.to.column_byte_index as usize];
                contains(selection, pattern, search_anchor)
            } else {
                let selection = &buffer.line_at(range.from.line_index as _).as_str()
                    [range.from.column_byte_index as usize..];
                if contains(selection, pattern, search_anchor) {
                    return true;
                }

                for line_index in (range.from.line_index + 1)..range.to.line_index {
                    let selection = buffer.line_at(line_index as _).as_str();
                    if contains(selection, pattern, search_anchor) {
                        return true;
                    }
                }

                let selection = &buffer.line_at(range.to.line_index as _).as_str()
                    [..range.to.column_byte_index as usize];
                contains(selection, pattern, search_anchor)
            }
        }

        let pattern = ctx.editor.read_line.input();
        let pattern = if pattern.is_empty() {
            ctx.editor.registers.get(SEARCH_REGISTER)
        } else {
            pattern
        };

        if let Err(error) = ctx.editor.aux_pattern.compile_searcher(pattern) {
            ctx.editor
                .status_bar
                .write(MessageKind::Error)
                .fmt(format_args!("{}", error));
            return None;
        }

        let handle = ctx.clients.get(ctx.client_handle)?.buffer_view_handle()?;
        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
        let buffer = ctx
            .editor
            .buffers
            .get_mut(buffer_view.buffer_handle)?
            .content();

        let mut cursors = buffer_view.cursors.mut_guard();
        let main_cursor_position = cursors.main_cursor().position;

        let mut filtered_cursors = [Cursor::zero(); CursorCollection::capacity()];
        let mut filtered_cursors_len = 0;

        for &cursor in &cursors[..] {
            if range_contains_pattern(buffer, cursor.to_range(), &ctx.editor.aux_pattern)
                == keep_if_contains_pattern
            {
                filtered_cursors[filtered_cursors_len] = cursor;
                filtered_cursors_len += 1;
            }
        }

        cursors.clear();
        for &cursor in &filtered_cursors[..filtered_cursors_len] {
            cursors.add(cursor);
        }

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

    use crate::{buffer_position::BufferPosition, cursor::Cursor, register::SEARCH_REGISTER};

    pub fn enter_by_pattern_mode(ctx: &mut ModeContext) {
        fn add_matches(
            cursors: &mut [Cursor],
            mut cursors_len: usize,
            line: &str,
            pattern: &Pattern,
            start_position: BufferPosition,
        ) -> usize {
            let search_anchor = pattern.search_anchor();
            for (index, s) in pattern.match_indices(line, search_anchor) {
                let mut anchor = start_position;
                anchor.column_byte_index += index as BufferPositionIndex;
                let mut position = anchor;
                position.column_byte_index += s.len() as BufferPositionIndex;

                if cursors_len >= cursors.len() {
                    return cursors.len();
                }
                cursors[cursors_len] = Cursor { anchor, position };
                cursors_len += 1;
            }

            cursors_len
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
            cursors: &mut [Cursor],
            mut cursors_len: usize,
            line: &str,
            pattern: &Pattern,
            start_position: BufferPosition,
        ) -> usize {
            let search_anchor = pattern.search_anchor();
            let mut index = 0;
            for (i, s) in pattern.match_indices(line, search_anchor) {
                if index != i {
                    let mut anchor = start_position;
                    anchor.column_byte_index += index as BufferPositionIndex;
                    let mut position = start_position;
                    position.column_byte_index += i as BufferPositionIndex;

                    if cursors_len >= cursors.len() {
                        return cursors.len();
                    }
                    cursors[cursors_len] = Cursor { anchor, position };
                    cursors_len += 1;
                }

                index = i + s.len();
            }

            if index < line.len() {
                if cursors_len >= cursors.len() {
                    return cursors.len();
                }
                cursors[cursors_len] = Cursor {
                    anchor: BufferPosition::line_col(
                        start_position.line_index,
                        start_position.column_byte_index + index as BufferPositionIndex,
                    ),
                    position: BufferPosition::line_col(
                        start_position.line_index,
                        start_position.column_byte_index + line.len() as BufferPositionIndex,
                    ),
                };
                cursors_len += 1;
            }

            cursors_len
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
        add_matches: fn(&mut [Cursor], usize, &str, &Pattern, BufferPosition) -> usize,
    ) -> Option<()> {
        let pattern = ctx.editor.read_line.input();
        let pattern = if pattern.is_empty() {
            ctx.editor.registers.get(SEARCH_REGISTER)
        } else {
            pattern
        };

        if let Err(error) = ctx.editor.aux_pattern.compile_searcher(pattern) {
            ctx.editor
                .status_bar
                .write(MessageKind::Error)
                .fmt(format_args!("{}", error));
            return None;
        }

        let handle = ctx.clients.get(ctx.client_handle)?.buffer_view_handle()?;
        let buffer_view = ctx.editor.buffer_views.get_mut(handle)?;
        let buffer = ctx
            .editor
            .buffers
            .get_mut(buffer_view.buffer_handle)?
            .content();

        let mut cursors = buffer_view.cursors.mut_guard();
        let main_cursor_position = cursors.main_cursor().position;

        let mut splitted_cursors = [Cursor::zero(); CursorCollection::capacity()];
        let mut splitted_cursors_len = 0;

        for cursor in &cursors[..] {
            let range = cursor.to_range();
            let new_cursors_start_index = splitted_cursors_len;

            if range.from.line_index == range.to.line_index {
                let line = &buffer.line_at(range.from.line_index as _).as_str()
                    [range.from.column_byte_index as usize..range.to.column_byte_index as usize];
                splitted_cursors_len = add_matches(
                    &mut splitted_cursors,
                    splitted_cursors_len,
                    line,
                    &ctx.editor.aux_pattern,
                    range.from,
                );
            } else {
                let line = &buffer.line_at(range.from.line_index as _).as_str()
                    [range.from.column_byte_index as usize..];
                splitted_cursors_len = add_matches(
                    &mut splitted_cursors,
                    splitted_cursors_len,
                    line,
                    &ctx.editor.aux_pattern,
                    range.from,
                );

                for line_index in (range.from.line_index + 1)..range.to.line_index {
                    let line = buffer.line_at(line_index as _).as_str();
                    splitted_cursors_len = add_matches(
                        &mut splitted_cursors,
                        splitted_cursors_len,
                        line,
                        &ctx.editor.aux_pattern,
                        BufferPosition::line_col(line_index, 0),
                    );
                }

                let line = &buffer.line_at(range.to.line_index as _).as_str()
                    [..range.to.column_byte_index as usize];
                splitted_cursors_len = add_matches(
                    &mut splitted_cursors,
                    splitted_cursors_len,
                    line,
                    &ctx.editor.aux_pattern,
                    BufferPosition::line_col(range.to.line_index, 0),
                );
            }

            if cursor.position == range.from {
                for cursor in &mut splitted_cursors[new_cursors_start_index..] {
                    std::mem::swap(&mut cursor.anchor, &mut cursor.position);
                }
            }
        }

        cursors.clear();
        for &cursor in &splitted_cursors[..splitted_cursors_len] {
            cursors.add(cursor);
        }

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
        navigation_history::{NavigationMovement, NavigationHistory},
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
                ReadLinePoll::Submitted => Mode::change_to(ctx, ModeKind::default()),
                ReadLinePoll::Canceled => {
                    if let Some(client) = ctx.clients.get_mut(ctx.client_handle) {
                        NavigationHistory::move_in_history(
                            client,
                            ctx.editor,
                            NavigationMovement::Backward,
                        );
                    }
                    Mode::change_to(ctx, ModeKind::default());
                }
            }
            None
        }

        if let Some(client) = ctx.clients.get_mut(ctx.client_handle) {
            NavigationHistory::save_client_snapshot(client, &ctx.editor.buffer_views);
        }
        ctx.editor.read_line.set_prompt("goto-line:");
        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::ReadLine);
    }
}

pub mod process {
    use super::*;

    pub fn enter_replace_mode(ctx: &mut ModeContext) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => None,
                ReadLinePoll::Submitted => {
                    spawn_process(ctx, true);
                    Mode::change_to(ctx, ModeKind::default());
                    None
                }
                ReadLinePoll::Canceled => {
                    Mode::change_to(ctx, ModeKind::default());
                    None
                }
            }
        }

        ctx.editor.read_line.set_prompt("replace-with-output:");
        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    pub fn enter_insert_mode(ctx: &mut ModeContext) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => None,
                ReadLinePoll::Submitted => {
                    spawn_process(ctx, false);
                    Mode::change_to(ctx, ModeKind::default());
                    None
                }
                ReadLinePoll::Canceled => {
                    Mode::change_to(ctx, ModeKind::default());
                    None
                }
            }
        }

        ctx.editor.read_line.set_prompt("insert-from-output:");
        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        Mode::change_to(ctx, ModeKind::ReadLine);
    }

    fn spawn_process(ctx: &mut ModeContext, pipe: bool) {
        let buffer_view_handle = match ctx
            .clients
            .get(ctx.client_handle)
            .and_then(|c| c.buffer_view_handle())
        {
            Some(handle) => handle,
            None => return,
        };
        let buffer_view = match ctx.editor.buffer_views.get_mut(buffer_view_handle) {
            Some(buffer_view) => buffer_view,
            None => return,
        };
        let content = match ctx.editor.buffers.get(buffer_view.buffer_handle) {
            Some(buffer) => buffer.content(),
            None => return,
        };

        const DEFAULT_SHARED_BUF: Option<SharedBuf> = None;
        let mut stdins = [DEFAULT_SHARED_BUF; CursorCollection::capacity()];

        if pipe {
            for (i, cursor) in buffer_view.cursors[..].iter().enumerate() {
                let range = cursor.to_range();

                let mut text = ctx.editor.string_pool.acquire();
                content.append_range_text_to_string(range, &mut text);

                let mut buf = ctx.platform.buf_pool.acquire();
                let writer = buf.write();
                writer.extend_from_slice(text.as_bytes());
                let buf = buf.share();
                ctx.platform.buf_pool.release(buf.clone());

                stdins[i] = Some(buf);

                ctx.editor.string_pool.release(text);
            }
        }

        buffer_view.delete_text_in_cursor_ranges(
            &mut ctx.editor.buffers,
            &mut ctx.editor.word_database,
            &mut ctx.editor.events,
        );

        ctx.editor.trigger_event_handlers(ctx.platform, ctx.clients);

        let command = ctx.editor.read_line.input();
        if let Some(buffer_view) = ctx.editor.buffer_views.get_mut(buffer_view_handle) {
            for (i, cursor) in buffer_view.cursors[..].iter().enumerate() {
                let command = match parse_process_command(&command) {
                    Some(command) => command,
                    None => continue,
                };

                ctx.editor.buffers.spawn_insert_process(
                    ctx.platform,
                    command,
                    buffer_view.buffer_handle,
                    cursor.position,
                    stdins[i].take(),
                );
            }
        }
    }
}

pub mod lsp_rename {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext, client_handle: lsp::ClientHandle, placeholder: &str) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<ModeOperation> {
            match poll {
                ReadLinePoll::Pending => None,
                ReadLinePoll::Submitted => {
                    if let Some(handle) = ctx.editor.mode.read_line_state.lsp_client_handle {
                        let platform = &mut *ctx.platform;
                        lsp::ClientManager::access(ctx.editor, handle, |e, c| {
                            c.finish_rename(e, platform);
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    None
                }
                ReadLinePoll::Canceled => {
                    if let Some(handle) = ctx.editor.mode.read_line_state.lsp_client_handle {
                        lsp::ClientManager::access(ctx.editor, handle, |_, c| {
                            c.cancel_current_request();
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    None
                }
            }
        }

        ctx.editor.read_line.set_prompt("rename:");
        let state = &mut ctx.editor.mode.read_line_state;
        state.on_client_keys = on_client_keys;
        state.lsp_client_handle = Some(client_handle);
        Mode::change_to(ctx, ModeKind::ReadLine);
        ctx.editor.read_line.input_mut().push_str(placeholder);
    }
}

