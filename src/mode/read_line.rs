use crate::{
    editor::{SEARCH_REGISTER, KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
};

pub struct State {
    on_enter: fn(&mut ModeContext),
    on_event: fn(&mut ModeContext, &mut KeysIterator, ReadLinePoll) -> ModeOperation,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_enter: |_| (),
            on_event: |_, _, _| ModeOperation::EnterMode(Mode::default()),
        }
    }
}

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        (self.on_enter)(ctx);
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.read_line.reset("");
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        let poll = ctx.read_line.poll(keys);
        (self.on_event)(ctx, keys, poll)
    }
}

pub mod search {
    use super::*;

    use crate::navigation_history::{NavigationDirection, NavigationHistory};

    pub fn mode() -> Mode {
        fn on_enter(ctx: &mut ModeContext) {
            NavigationHistory::save_client_snapshot(
                ctx.clients,
                ctx.buffer_views,
                ctx.target_client,
            );
            ctx.read_line.reset("search:");
            update_search(ctx);
        }

        fn on_event(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> ModeOperation {
            match poll {
                ReadLinePoll::Pending => {
                    update_search(ctx);
                    ModeOperation::None
                }
                ReadLinePoll::Submitted => {
                    ctx.registers.set(SEARCH_REGISTER, ctx.read_line.input());
                    ModeOperation::EnterMode(Mode::default())
                }
                ReadLinePoll::Canceled => {
                    NavigationHistory::move_in_history(
                        ctx.clients,
                        ctx.buffer_views,
                        ctx.target_client,
                        NavigationDirection::Backward,
                    );
                    ModeOperation::EnterMode(Mode::default())
                }
            }
        }

        Mode::ReadLine(State { on_enter, on_event })
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
    ($poll:expr => $value:expr) => {
        match $poll {
            ReadLinePoll::Pending => ModeOperation::None,
            ReadLinePoll::Submitted => {
                $value;
                ModeOperation::EnterMode(Mode::default())
            }
            ReadLinePoll::Canceled => ModeOperation::EnterMode(Mode::default()),
        }
    };
}

pub mod filter_cursors {
    use super::*;

    use crate::{buffer::BufferContent, buffer_position::BufferRange, cursor::Cursor};

    pub fn filter_mode() -> Mode {
        fn on_enter(ctx: &mut ModeContext) {
            ctx.read_line.reset("filter:");
        }

        Mode::ReadLine(State {
            on_enter,
            on_event: |ctx, _, poll| on_submitted!(poll => on_event_impl(ctx, true)),
        })
    }

    pub fn except_mode() -> Mode {
        fn on_enter(ctx: &mut ModeContext) {
            ctx.read_line.reset("except:");
        }

        Mode::ReadLine(State {
            on_enter,
            on_event: |ctx, _, poll| on_submitted!(poll => on_event_impl(ctx, false)),
        })
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
            ctx.registers.get(SEARCH_REGISTER).unwrap_or("")
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

    pub fn by_pattern_mode() -> Mode {
        fn on_enter(ctx: &mut ModeContext) {
            ctx.read_line.reset("split-by:");
        }

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

        Mode::ReadLine(State {
            on_enter,
            on_event: |ctx, _, poll| on_submitted!(poll => on_event_impl(ctx, add_matches)),
        })
    }

    pub fn by_separators_mode() -> Mode {
        fn on_enter(ctx: &mut ModeContext) {
            ctx.read_line.reset("split-on:");
        }

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

        Mode::ReadLine(State {
            on_enter,
            on_event: |ctx, _, poll| on_submitted!(poll => on_event_impl(ctx, add_matches)),
        })
    }

    fn on_event_impl(
        ctx: &mut ModeContext,
        add_matches: fn(&mut CursorCollectionMutGuard, &str, &str, BufferPosition),
    ) {
        let pattern = ctx.read_line.input();
        let pattern = if pattern.is_empty() {
            ctx.registers.get(SEARCH_REGISTER).unwrap_or("")
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

    pub fn mode() -> Mode {
        fn on_enter(ctx: &mut ModeContext) {
            NavigationHistory::save_client_snapshot(
                ctx.clients,
                ctx.buffer_views,
                ctx.target_client,
            );
            ctx.read_line.reset("goto-line:");
        }

        fn on_event(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> ModeOperation {
            match poll {
                ReadLinePoll::Pending => {
                    let line_number: usize = match ctx.read_line.input().parse() {
                        Ok(number) => number,
                        Err(_) => return ModeOperation::None,
                    };
                    let line_index = line_number.saturating_sub(1);

                    let handle = unwrap_or_none!(ctx.current_buffer_view_handle());
                    let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

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

                    ModeOperation::None
                }
                ReadLinePoll::Submitted => ModeOperation::EnterMode(Mode::default()),
                ReadLinePoll::Canceled => {
                    NavigationHistory::move_in_history(
                        ctx.clients,
                        ctx.buffer_views,
                        ctx.target_client,
                        NavigationDirection::Backward,
                    );
                    ModeOperation::EnterMode(Mode::default())
                }
            }
        }

        Mode::ReadLine(State { on_enter, on_event })
    }
}

pub mod custom {
    use super::*;

    use crate::script::{ScriptEngineRef, ScriptFunction, ScriptResult, ScriptString, ScriptValue};

    const PROMPT_REGISTRY_KEY: &str = "read_line_prompt";
    const CALLBACK_REGISTRY_KEY: &str = "read_line_callback";

    pub fn prompt(engine: ScriptEngineRef, prompt: ScriptString) -> ScriptResult<()> {
        engine.save_to_registry(PROMPT_REGISTRY_KEY, ScriptValue::String(prompt))
    }

    pub fn mode(engine: ScriptEngineRef, callback: ScriptFunction) -> ScriptResult<Mode> {
        fn on_enter(ctx: &mut ModeContext) {
            match ctx
                .scripts
                .as_ref()
                .take_from_registry::<ScriptString>(PROMPT_REGISTRY_KEY)
            {
                Ok(prompt) => ctx.read_line.reset(prompt.to_str().unwrap_or(">")),
                Err(_) => ctx.read_line.reset(">"),
            }
        }

        fn on_event(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> ModeOperation {
            let (engine, read_line, mut ctx) = ctx.script_context();
            let operation = engine.as_ref_with_ctx(&mut ctx, |engine, ctx, mut guard| {
                let input = match poll {
                    ReadLinePoll::Pending => return Ok(ModeOperation::None),
                    ReadLinePoll::Submitted => {
                        ScriptValue::String(engine.create_string(read_line.input().as_bytes())?)
                    }
                    ReadLinePoll::Canceled => ScriptValue::Nil,
                };

                engine
                    .take_from_registry::<ScriptFunction>(CALLBACK_REGISTRY_KEY)?
                    .call(&mut guard, input)?;

                let mut mode = Mode::default();
                std::mem::swap(&mut mode, &mut ctx.next_mode);
                Ok(ModeOperation::EnterMode(mode))
            });

            match operation {
                Ok(operation) => operation,
                Err(error) => {
                    ctx.status_message.write_error(&error);
                    ModeOperation::EnterMode(Mode::default())
                }
            }
        }

        engine.save_to_registry(CALLBACK_REGISTRY_KEY, ScriptValue::Function(callback))?;
        Ok(Mode::ReadLine(State { on_enter, on_event }))
    }
}
