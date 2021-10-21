use std::process::Stdio;

use crate::{
    buffer::{BufferCollection, BufferHandle},
    buffer_position::{BufferPosition, BufferPositionIndex},
    buffer_view::CursorMovementKind,
    client::ClientHandle,
    cursor::{Cursor, CursorCollection},
    editor::{Editor, EditorContext, EditorControlFlow, KeysIterator},
    editor_utils::{parse_process_command, MessageKind, ReadLinePoll, ResidualStrBytes},
    events::EditorEventQueue,
    mode::{ModeKind, ModeState},
    pattern::Pattern,
    platform::{PlatformRequest, PooledBuf, ProcessTag},
    plugin::PluginHandle,
    word_database::WordDatabase,
};

pub struct State {
    pub on_client_keys: fn(
        &mut EditorContext,
        ClientHandle,
        &mut KeysIterator,
        ReadLinePoll,
    ) -> Option<EditorControlFlow>,
    pub plugin_handle: Option<PluginHandle>,
    previous_position: BufferPosition,
    find_pattern_command: String,
    find_pattern_buffer_handle: Option<BufferHandle>,
    find_pattern_residual_bytes: ResidualStrBytes,
}

impl State {
    pub(crate) fn on_buffer_close(&mut self, buffer_handle: BufferHandle) {
        if self.find_pattern_buffer_handle == Some(buffer_handle) {
            self.find_pattern_buffer_handle = None;
        }
    }

    pub(crate) fn on_process_output(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        bytes: &[u8],
        events: &mut EditorEventQueue,
    ) {
        if let Some(buffer_handle) = self.find_pattern_buffer_handle {
            let mut buf = Default::default();
            let texts = self
                .find_pattern_residual_bytes
                .receive_bytes(&mut buf, bytes);

            let buffer = buffers.get_mut(buffer_handle);
            for text in texts {
                let position = buffer.content().end();
                buffer.insert_text(word_database, position, text, events);
            }
        }
    }

    pub(crate) fn on_process_exit(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
    ) {
        self.on_process_output(buffers, word_database, &[], events);

        self.find_pattern_command.clear();
        self.find_pattern_buffer_handle = None;
        self.find_pattern_residual_bytes = ResidualStrBytes::default();
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _, _| Some(EditorControlFlow::Continue),
            plugin_handle: None,
            previous_position: BufferPosition::zero(),
            find_pattern_command: String::new(),
            find_pattern_buffer_handle: None,
            find_pattern_residual_bytes: ResidualStrBytes::default(),
        }
    }
}

impl ModeState for State {
    fn on_enter(editor: &mut Editor) {
        editor.read_line.input_mut().clear();
    }

    fn on_exit(editor: &mut Editor) {
        let state = &mut editor.mode.read_line_state;
        state.plugin_handle = None;
        state.find_pattern_command.clear();
        editor.read_line.input_mut().clear();
    }

    fn on_client_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<EditorControlFlow> {
        let poll = ctx.editor.read_line.poll(
            &mut ctx.platform,
            &mut ctx.editor.string_pool,
            &ctx.editor.buffered_keys,
            keys,
        );
        let f = ctx.editor.mode.read_line_state.on_client_keys;
        f(ctx, client_handle, keys, poll)
    }
}

pub mod search {
    use super::*;

    use crate::editor_utils::SEARCH_REGISTER;

    pub fn enter_mode(ctx: &mut EditorContext, client_handle: ClientHandle) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => {
                    update_search(ctx, client_handle);
                }
                ReadLinePoll::Submitted => {
                    if let Some(buffer_view) = ctx
                        .clients
                        .get(client_handle)
                        .buffer_view_handle()
                        .map(|h| ctx.editor.buffer_views.get(h))
                    {
                        let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);
                        let search_ranges = buffer.search_ranges();
                        if search_ranges.is_empty() {
                            restore_saved_position(ctx, client_handle);
                        } else {
                            let position = buffer_view.cursors.main_cursor().position;
                            ctx.editor.mode.normal_state.search_index =
                                match search_ranges.binary_search_by_key(&position, |r| r.from) {
                                    Ok(i) => i,
                                    Err(i) => i,
                                };
                        }
                    }

                    let register = ctx.editor.registers.get_mut(SEARCH_REGISTER);
                    register.clear();
                    register.push_str(ctx.editor.read_line.input());
                    ctx.editor.enter_mode(ModeKind::default());
                }
                ReadLinePoll::Canceled => {
                    restore_saved_position(ctx, client_handle);
                    ctx.editor.enter_mode(ModeKind::default());
                }
            }

            Some(EditorControlFlow::Continue)
        }

        save_current_position(ctx, client_handle);
        ctx.editor.read_line.set_prompt("search:");
        update_search(ctx, client_handle);

        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
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
            .compile_searcher(&ctx.editor.read_line.input());
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
        editor_utils::SEARCH_REGISTER,
    };

    pub fn enter_filter_mode(ctx: &mut EditorContext) {
        ctx.editor.read_line.set_prompt("filter:");
        ctx.editor.mode.read_line_state.on_client_keys = |ctx, client_handle, _, poll| {
            on_submitted(ctx, client_handle, poll, |ctx, client_handle| {
                on_event_impl(ctx, client_handle, true);
            });
            Some(EditorControlFlow::Continue)
        };
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    pub fn enter_except_mode(ctx: &mut EditorContext) {
        ctx.editor.read_line.set_prompt("except:");
        ctx.editor.mode.read_line_state.on_client_keys = |ctx, client_handle, _, poll| {
            on_submitted(ctx, client_handle, poll, |ctx, client_handle| {
                on_event_impl(ctx, client_handle, false);
            });
            Some(EditorControlFlow::Continue)
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
    }
}

pub mod split_cursors {
    use super::*;

    use crate::{buffer_position::BufferPosition, cursor::Cursor, editor_utils::SEARCH_REGISTER};

    pub fn enter_by_pattern_mode(ctx: &mut EditorContext) {
        fn add_matches(
            cursors: &mut [Cursor],
            mut cursors_len: usize,
            line: &str,
            pattern: &Pattern,
            start_position: BufferPosition,
        ) -> usize {
            let search_anchor = pattern.search_anchor();
            for range in pattern.match_indices(line, search_anchor) {
                let mut anchor = start_position;
                anchor.column_byte_index += range.start as BufferPositionIndex;
                let mut position = start_position;
                position.column_byte_index += range.end as BufferPositionIndex;

                if cursors_len >= cursors.len() {
                    return cursors.len();
                }
                cursors[cursors_len] = Cursor { anchor, position };
                cursors_len += 1;
            }

            cursors_len
        }

        ctx.editor.read_line.set_prompt("split-by:");
        ctx.editor.mode.read_line_state.on_client_keys = |ctx, client_handle, _, poll| {
            on_submitted(ctx, client_handle, poll, |ctx, client_handle| {
                on_event_impl(ctx, client_handle, add_matches);
            });
            Some(EditorControlFlow::Continue)
        };
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    pub fn enter_by_separators_mode(ctx: &mut EditorContext) {
        fn add_matches(
            cursors: &mut [Cursor],
            mut cursors_len: usize,
            line: &str,
            pattern: &Pattern,
            start_position: BufferPosition,
        ) -> usize {
            let search_anchor = pattern.search_anchor();
            let mut index = 0;
            for range in pattern.match_indices(line, search_anchor) {
                if index != range.start {
                    let mut anchor = start_position;
                    anchor.column_byte_index += index as BufferPositionIndex;
                    let mut position = start_position;
                    position.column_byte_index += range.start as BufferPositionIndex;

                    if cursors_len >= cursors.len() {
                        return cursors.len();
                    }
                    cursors[cursors_len] = Cursor { anchor, position };
                    cursors_len += 1;
                }

                index = range.end;
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
        ctx.editor.mode.read_line_state.on_client_keys = |ctx, client_handle, _, poll| {
            on_submitted(ctx, client_handle, poll, |ctx, client_handle| {
                on_event_impl(ctx, client_handle, add_matches);
            });
            Some(EditorControlFlow::Continue)
        };
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    fn on_event_impl(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        add_matches: fn(&mut [Cursor], usize, &str, &Pattern, BufferPosition) -> usize,
    ) {
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

        let mut splitted_cursors = [Cursor::zero(); CursorCollection::capacity()];
        let mut splitted_cursors_len = 0;

        for cursor in &cursors[..] {
            let range = cursor.to_range();
            let new_cursors_start_index = splitted_cursors_len;

            if range.from.line_index == range.to.line_index {
                let line = &buffer.lines()[range.from.line_index as usize].as_str()
                    [range.from.column_byte_index as usize..range.to.column_byte_index as usize];
                splitted_cursors_len = add_matches(
                    &mut splitted_cursors,
                    splitted_cursors_len,
                    line,
                    &ctx.editor.aux_pattern,
                    range.from,
                );
            } else {
                let line = &buffer.lines()[range.from.line_index as usize].as_str()
                    [range.from.column_byte_index as usize..];
                splitted_cursors_len = add_matches(
                    &mut splitted_cursors,
                    splitted_cursors_len,
                    line,
                    &ctx.editor.aux_pattern,
                    range.from,
                );

                for line_index in (range.from.line_index + 1)..range.to.line_index {
                    let line = buffer.lines()[line_index as usize].as_str();
                    splitted_cursors_len = add_matches(
                        &mut splitted_cursors,
                        splitted_cursors_len,
                        line,
                        &ctx.editor.aux_pattern,
                        BufferPosition::line_col(line_index, 0),
                    );
                }

                let line = &buffer.lines()[range.to.line_index as usize].as_str()
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
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => {
                    let line_number: usize = match ctx.editor.read_line.input().parse() {
                        Ok(number) => number,
                        Err(_) => return Some(EditorControlFlow::Continue),
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
            Some(EditorControlFlow::Continue)
        }

        save_current_position(ctx, client_handle);
        ctx.editor.read_line.set_prompt("goto-line:");
        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }
}

pub mod process {
    use super::*;

    pub fn enter_replace_mode(ctx: &mut EditorContext) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => {
                    spawn_process(ctx, client_handle, true);
                    ctx.editor.enter_mode(ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
                ReadLinePoll::Canceled => {
                    ctx.editor.enter_mode(ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
            }
        }

        ctx.editor.read_line.set_prompt("replace-with-output:");
        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    pub fn enter_insert_mode(ctx: &mut EditorContext) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => {
                    spawn_process(ctx, client_handle, false);
                    ctx.editor.enter_mode(ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
                ReadLinePoll::Canceled => {
                    ctx.editor.enter_mode(ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
            }
        }

        ctx.editor.read_line.set_prompt("insert-from-output:");
        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    pub fn enter_run_mode(ctx: &mut EditorContext) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            _: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => {
                    let command = ctx.editor.read_line.input();
                    if let Some(mut command) = parse_process_command(command) {
                        command.stdin(Stdio::null());
                        command.stdout(Stdio::null());
                        command.stderr(Stdio::null());

                        ctx.platform
                            .requests
                            .enqueue(PlatformRequest::SpawnProcess {
                                tag: ProcessTag::Ignored,
                                command,
                                buf_len: 0,
                            });
                    }

                    ctx.editor.enter_mode(ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
                ReadLinePoll::Canceled => {
                    ctx.editor.enter_mode(ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
            }
        }

        ctx.editor.read_line.set_prompt("run-command:");
        ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }

    fn spawn_process(ctx: &mut EditorContext, client_handle: ClientHandle, pipe: bool) {
        let buffer_view_handle = match ctx.clients.get(client_handle).buffer_view_handle() {
            Some(handle) => handle,
            None => return,
        };
        let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
        let content = ctx.editor.buffers.get(buffer_view.buffer_handle).content();

        const NONE_POOLED_BUF: Option<PooledBuf> = None;
        let mut stdins = [NONE_POOLED_BUF; CursorCollection::capacity()];

        if pipe {
            for (i, cursor) in buffer_view.cursors[..].iter().enumerate() {
                let range = cursor.to_range();

                let mut buf = ctx.platform.buf_pool.acquire();
                let write = buf.write();
                for text in content.text_range(range) {
                    write.extend_from_slice(text.as_bytes());
                }

                stdins[i] = Some(buf);
            }
        }

        buffer_view.delete_text_in_cursor_ranges(
            &mut ctx.editor.buffers,
            &mut ctx.editor.word_database,
            &mut ctx.editor.events,
        );

        ctx.trigger_event_handlers();

        let command = ctx.editor.read_line.input();
        let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
        for (i, cursor) in buffer_view.cursors[..].iter().enumerate() {
            let command = match parse_process_command(command) {
                Some(command) => command,
                None => continue,
            };

            ctx.editor.buffers.spawn_insert_process(
                &mut ctx.platform,
                command,
                buffer_view.buffer_handle,
                cursor.position,
                stdins[i].take(),
            );
        }
    }
}

pub mod find_pattern {
    use super::*;

    use std::{path::Path, process::Stdio};

    use crate::{
        buffer::BufferProperties,
        buffer_position::BufferRange,
        platform::{PlatformRequest, ProcessTag},
    };

    pub fn enter_mode(ctx: &mut EditorContext, command: &str, prompt: &str) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => return Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => {
                    ctx.editor.enter_mode(ModeKind::default());
                    return Some(EditorControlFlow::Continue);
                }
            }

            let mut buffer_name = ctx.editor.string_pool.acquire();
            buffer_name.push_str(ctx.editor.read_line.input());
            buffer_name.push_str(".refs");
            let buffer_view_handle = ctx.editor.buffer_view_handle_from_path(
                client_handle,
                Path::new(&buffer_name),
                BufferProperties::scratch(),
                true,
            );
            ctx.editor.string_pool.release(buffer_name);

            let buffer_view_handle = match buffer_view_handle {
                Ok(handle) => handle,
                Err(error) => {
                    ctx.editor
                        .status_bar
                        .write(MessageKind::Error)
                        .fmt(format_args!("{}", error));
                    return Some(EditorControlFlow::Continue);
                }
            };

            let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
            let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle);

            buffer.properties = BufferProperties::scratch();
            let range = BufferRange::between(BufferPosition::zero(), buffer.content().end());
            buffer.delete_range(&mut ctx.editor.word_database, range, &mut ctx.editor.events);

            let state = &mut ctx.editor.mode.read_line_state;
            state.find_pattern_buffer_handle = Some(buffer.handle());
            state.find_pattern_residual_bytes = ResidualStrBytes::default();

            const REPLACE_PATTERN: &str = "{}";
            if let Some(i) = state.find_pattern_command.find(REPLACE_PATTERN) {
                state
                    .find_pattern_command
                    .replace_range(i..i + REPLACE_PATTERN.len(), ctx.editor.read_line.input());
            }

            let command = match parse_process_command(&state.find_pattern_command) {
                Some(mut command) => {
                    command.stdin(Stdio::null());
                    command.stdout(Stdio::piped());
                    command.stderr(Stdio::null());

                    command
                }
                None => {
                    ctx.editor
                        .status_bar
                        .write(MessageKind::Error)
                        .fmt(format_args!(
                            "invalid find pattern command '{}'",
                            &state.find_pattern_command
                        ));
                    return Some(EditorControlFlow::Continue);
                }
            };

            ctx.platform
                .requests
                .enqueue(PlatformRequest::SpawnProcess {
                    tag: ProcessTag::FindPattern,
                    command,
                    buf_len: 4 * 1024,
                });

            let client = ctx.clients.get_mut(client_handle);
            client.set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);

            {
                let mut cursors = ctx
                    .editor
                    .buffer_views
                    .get_mut(buffer_view_handle)
                    .cursors
                    .mut_guard();
                cursors.clear();
                cursors.add(Cursor {
                    anchor: BufferPosition::zero(),
                    position: BufferPosition::zero(),
                });
            }

            ctx.editor.enter_mode(ModeKind::default());
            Some(EditorControlFlow::Continue)
        }

        ctx.editor.read_line.set_prompt(prompt);
        let state = &mut ctx.editor.mode.read_line_state;
        state.on_client_keys = on_client_keys;
        state.find_pattern_command.clear();
        state.find_pattern_command.push_str(command);
        ctx.editor.enter_mode(ModeKind::ReadLine);
    }
}

fn save_current_position(ctx: &mut EditorContext, client_handle: ClientHandle) {
    let buffer_view_handle = match ctx.clients.get(client_handle).buffer_view_handle() {
        Some(handle) => handle,
        None => return,
    };
    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
    ctx.editor.mode.read_line_state.previous_position = buffer_view.cursors.main_cursor().position;
}

fn restore_saved_position(ctx: &mut EditorContext, client_handle: ClientHandle) {
    let buffer_view_handle = match ctx.clients.get(client_handle).buffer_view_handle() {
        Some(handle) => handle,
        None => return,
    };
    let position = ctx.editor.mode.read_line_state.previous_position;
    let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
    let mut cursors = buffer_view.cursors.mut_guard();
    cursors.clear();
    cursors.add(Cursor {
        anchor: position,
        position,
    });
}
