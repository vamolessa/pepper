use crate::{
    editor::{EditorLoop, KeysIterator, StatusMessageKind},
    mode::{poll_input, InputPollResult, Mode, ModeContext, ModeOperation, ModeState},
    script::{ScriptContext, ScriptValue},
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        ctx.input.clear();
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.input.clear();
    }

    fn on_event(&mut self, mut ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match poll_input(&mut ctx, keys) {
            InputPollResult::Pending => ModeOperation::None,
            InputPollResult::Canceled => ModeOperation::EnterMode(Mode::default()),
            InputPollResult::Submited => {
                let mut context = ScriptContext {
                    target_client: ctx.target_client,
                    clients: ctx.clients,
                    editor_loop: EditorLoop::Continue,

                    config: ctx.config,

                    buffers: ctx.buffers,
                    buffer_views: ctx.buffer_views,
                    word_database: ctx.word_database,

                    picker: ctx.picker,

                    status_message_kind: ctx.status_message_kind,
                    status_message: ctx.status_message,

                    keymaps: ctx.keymaps,
                };

                match ctx.scripts.eval(&mut context, &ctx.input) {
                    Ok(value) => {
                        let mut kind = StatusMessageKind::Info;
                        let message = match value {
                            ScriptValue::Nil => None,
                            ScriptValue::Function(f) => match f.call(()) {
                                Ok(ScriptValue::Nil) => None,
                                Ok(value) => Some(value.to_string()),
                                Err(error) => match context.editor_loop {
                                    EditorLoop::Quit => return ModeOperation::Quit,
                                    EditorLoop::QuitAll => return ModeOperation::QuitAll,
                                    EditorLoop::Continue => {
                                        kind = StatusMessageKind::Error;
                                        Some(error.to_string())
                                    }
                                },
                            },
                            _ => Some(value.to_string()),
                        };

                        if let Some(message) = message {
                            *ctx.status_message_kind = kind;
                            ctx.status_message.clear();
                            ctx.status_message.push_str(&message);
                        }

                        ModeOperation::EnterMode(Mode::default())
                    }
                    Err(e) => match context.editor_loop {
                        EditorLoop::Quit => ModeOperation::Quit,
                        EditorLoop::QuitAll => ModeOperation::QuitAll,
                        EditorLoop::Continue => {
                            use std::error::Error;

                            let mut message = e.to_string();
                            let mut error = e.source();
                            while let Some(e) = error {
                                message.push('\n');
                                let s = e.to_string();
                                message.push_str(&s);
                                error = e.source();
                            }

                            *ctx.status_message_kind = StatusMessageKind::Error;
                            ctx.status_message.clear();
                            ctx.status_message.push_str(&message);

                            ModeOperation::EnterMode(Mode::default())
                        }
                    },
                }
            }
        }
    }
}
