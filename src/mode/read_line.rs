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
            ctx.read_line.reset("#");
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

pub mod script {
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
            let engine = engine.as_ref();

            let input = match poll {
                ReadLinePoll::Pending => return,
                ReadLinePoll::Submitted => match engine.create_string(read_line.input().as_bytes())
                {
                    Ok(input) => ScriptValue::String(input),
                    Err(error) => {
                        ctx.status_message.write_error(&error);
                        return;
                    }
                },
                ReadLinePoll::Canceled => ScriptValue::Nil,
            };

            match engine
                .take_from_registry::<ScriptFunction>(CALLBACK_REGISTRY_KEY)
                .and_then(|c| c.call(&mut ctx, input))
            {
                Ok(()) => (),
                Err(error) => {
                    ctx.status_message.write_error(&error);
                }
            }
        }

        engine.save_to_registry(CALLBACK_REGISTRY_KEY, ScriptValue::Function(callback))?;
        Ok(Mode::ReadLine(State { on_enter, on_event }))
    }
}
