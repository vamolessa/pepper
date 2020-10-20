use crate::{
    editor::{KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
};

pub struct State {
    on_enter: fn(&mut ModeContext),
    on_event: fn(&mut ModeContext, &mut KeysIterator, ReadLinePoll),
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_enter: |_| (),
            on_event: |_, _, _| (),
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
        (self.on_event)(ctx, keys, poll);
        match poll {
            ReadLinePoll::Pending => ModeOperation::None,
            _ => ModeOperation::EnterMode(Mode::default()),
        }
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

        fn on_event(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
            match poll {
                ReadLinePoll::Pending => update_search(ctx),
                ReadLinePoll::Submitted => ctx.search.set(ctx.read_line.input()),
                ReadLinePoll::Canceled => NavigationHistory::move_in_history(
                    ctx.clients,
                    ctx.buffer_views,
                    ctx.target_client,
                    NavigationDirection::Backward,
                ),
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

pub mod filter_cursors {
    use super::*;

    use crate::{
        buffer_position::BufferPosition,
        cursor::{Cursor, CursorCollectionMutGuard},
    };

    pub fn by_words_mode() -> Mode {
        fn on_enter(ctx: &mut ModeContext) {
            ctx.read_line.reset("filter:");
        }

        fn on_event(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
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

            if !matches!(poll, ReadLinePoll::Submitted) {
                return;
            }

            let pattern = ctx.read_line.input();
            let pattern = if pattern.is_empty() {
                ctx.search.as_str()
            } else {
                pattern
            };

            let handle = unwrap_or_return!(ctx.current_buffer_view_handle());
            let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
            let buffer =
                unwrap_or_return!(ctx.buffers.get_mut(buffer_view.buffer_handle)).content();

            let mut cursors = buffer_view.cursors.mut_guard();
            let main_cursor_position = cursors.main_cursor().position;
            let cursor_count = cursors[..].len();

            for i in 0..cursor_count {
                let range = cursors[i].as_range();

                if range.from.line_index == range.to.line_index {
                    let line = &buffer.line_at(range.from.line_index).as_str()
                        [range.from.column_byte_index..range.to.column_byte_index];
                    add_matches(&mut cursors, line, pattern, range.from);
                } else {
                    let line = &buffer.line_at(range.from.line_index).as_str()
                        [range.from.column_byte_index..];
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

                if cursors[i].position == range.from {
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

        Mode::ReadLine(State { on_enter, on_event })
    }

    pub fn by_separators_mode() -> Mode {
        fn on_enter(ctx: &mut ModeContext) {
            ctx.read_line.reset("filter:");
        }

        fn on_event(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
            fn add_matches(
                cursors: &mut CursorCollectionMutGuard,
                line: &str,
                pattern: &str,
                start_position: BufferPosition,
            ) {
                let mut index = start_position.column_byte_index;
                for (i, s) in line.match_indices(pattern) {
                    if i != index {
                        cursors.add(Cursor {
                            anchor: BufferPosition::line_col(start_position.line_index, index),
                            position: BufferPosition::line_col(start_position.line_index, i),
                        });
                    }

                    index = i + s.len();
                }
            }

            if !matches!(poll, ReadLinePoll::Submitted) {
                return;
            }

            let pattern = ctx.read_line.input();
            let pattern = if pattern.is_empty() {
                ctx.search.as_str()
            } else {
                pattern
            };

            let handle = unwrap_or_return!(ctx.current_buffer_view_handle());
            let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
            let buffer =
                unwrap_or_return!(ctx.buffers.get_mut(buffer_view.buffer_handle)).content();

            let mut cursors = buffer_view.cursors.mut_guard();
            let main_cursor_position = cursors.main_cursor().position;
            let cursor_count = cursors[..].len();

            for i in 0..cursor_count {
                let range = cursors[i].as_range();

                if range.from.line_index == range.to.line_index {
                    let line = &buffer.line_at(range.from.line_index).as_str()
                        [range.from.column_byte_index..range.to.column_byte_index];
                    add_matches(&mut cursors, line, pattern, range.from);
                } else {
                    let line = &buffer.line_at(range.from.line_index).as_str()
                        [range.from.column_byte_index..];
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

                if cursors[i].position == range.from {
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

        Mode::ReadLine(State { on_enter, on_event })
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

        fn on_event(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
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
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => NavigationHistory::move_in_history(
                    ctx.clients,
                    ctx.buffer_views,
                    ctx.target_client,
                    NavigationDirection::Backward,
                ),
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

        fn on_event(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
            let (engine, read_line, mut ctx) = ctx.script_context();
            let result = engine.as_ref_with_ctx(&mut ctx, |engine, _, mut guard| {
                let input = match poll {
                    ReadLinePoll::Pending => return Ok(()),
                    ReadLinePoll::Submitted => {
                        ScriptValue::String(engine.create_string(read_line.input().as_bytes())?)
                    }
                    ReadLinePoll::Canceled => ScriptValue::Nil,
                };

                engine
                    .take_from_registry::<ScriptFunction>(CALLBACK_REGISTRY_KEY)?
                    .call(&mut guard, input)?;
                Ok(())
            });

            if let Err(error) = result {
                ctx.status_message.write_error(&error);
            }
        }

        engine.save_to_registry(CALLBACK_REGISTRY_KEY, ScriptValue::Function(callback))?;
        Ok(Mode::ReadLine(State { on_enter, on_event }))
    }
}
