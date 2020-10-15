use crate::{
    client_event::Key,
    editor::{KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    word_database::WordDatabase,
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
        ctx.picker.filter(WordDatabase::empty(), "");
        (self.on_enter)(ctx);
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.read_line.reset("");
        ctx.picker.reset();
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match ctx.read_line.poll(keys) {
            ReadLinePoll::Pending => {
                keys.put_back();
                match keys.next() {
                    Key::Ctrl('n') | Key::Ctrl('j') => {
                        ctx.picker.move_cursor(1);
                    }
                    Key::Ctrl('p') | Key::Ctrl('k') => {
                        ctx.picker.move_cursor(-1);
                    }
                    _ => ctx
                        .picker
                        .filter(WordDatabase::empty(), ctx.read_line.input()),
                }

                (self.on_event)(ctx, keys, ReadLinePoll::Pending);
                ModeOperation::None
            }
            poll => {
                (self.on_event)(ctx, keys, poll);
                ModeOperation::EnterMode(Mode::default())
            }
        }
    }
}

pub mod buffer {
    use super::*;

    use std::path::Path;

    use crate::{
        editor::StatusMessageKind, navigation_history::NavigationHistory, picker::CustomPickerEntry,
    };

    pub fn mode(ctx: &mut ModeContext) -> Mode {
        fn on_enter(ctx: &mut ModeContext) {
            ctx.read_line.reset(">");
        }

        fn on_event(ctx: &mut ModeContext, _: &mut KeysIterator, poll: ReadLinePoll) {
            if !matches!(poll, ReadLinePoll::Submitted) {
                return;
            }

            let path = ctx
                .picker
                .current_entry_name(WordDatabase::empty())
                .unwrap_or(ctx.read_line.input());

            NavigationHistory::save_client_snapshot(
                ctx.clients,
                ctx.buffer_views,
                ctx.target_client,
            );

            match ctx.buffer_views.buffer_view_handle_from_path(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                ctx.target_client,
                Path::new(path),
            ) {
                Ok(handle) => ctx.set_current_buffer_view_handle(Some(handle)),
                Err(error) => ctx
                    .status_message
                    .write_str(StatusMessageKind::Error, &error),
            }
        }

        ctx.picker.reset();
        for buffer in ctx.buffers.iter() {
            if let Some(path) = buffer.path().and_then(|p| p.to_str()) {
                ctx.picker.add_custom_entry(CustomPickerEntry {
                    name: path.into(),
                    description: String::new(),
                });
            }
        }

        Mode::Picker(State { on_enter, on_event })
    }
}

pub mod custom {
    use super::*;

    use crate::script::{ScriptEngineRef, ScriptFunction, ScriptResult, ScriptString, ScriptValue};

    const PROMPT_REGISTRY_KEY: &str = "picker_prompt";
    const CALLBACK_REGISTRY_KEY: &str = "picker_callback";

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
            let (engine, _, mut ctx) = ctx.script_context();
            let engine = engine.as_ref();

            let entry = match poll {
                ReadLinePoll::Pending => return,
                ReadLinePoll::Submitted => {
                    match ctx.picker.current_entry_name(WordDatabase::empty()) {
                        Some(entry) => match engine.create_string(entry.as_bytes()) {
                            Ok(entry) => ScriptValue::String(entry),
                            Err(error) => {
                                ctx.status_message.write_error(&error);
                                return;
                            }
                        },
                        None => ScriptValue::Nil,
                    }
                }
                ReadLinePoll::Canceled => ScriptValue::Nil,
            };

            match engine
                .take_from_registry::<ScriptFunction>(CALLBACK_REGISTRY_KEY)
                .and_then(|c| c.call(&mut ctx, entry))
            {
                Ok(()) => (),
                Err(error) => {
                    ctx.status_message.write_error(&error);
                }
            }
        }

        engine.save_to_registry(CALLBACK_REGISTRY_KEY, ScriptValue::Function(callback))?;
        Ok(Mode::Picker(State { on_enter, on_event }))
    }
}
