use crate::{
    editor::{EditorLoop, KeysIterator, StatusMessageKind},
    mode::{poll_input, InputPollResult, Mode, ModeContext, ModeOperation, ModeState},
    script::{ScriptContext, ScriptValue},
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        ctx.prompt.clear();
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.prompt.clear();
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

                    status_message: ctx.status_message,

                    keymaps: ctx.keymaps,
                };

                match ctx.scripts.eval(&mut context, &ctx.prompt) {
                    Ok(value) => {
                        match value {
                            ScriptValue::Nil => (),
                            ScriptValue::Function(f) => match f.call(()) {
                                Ok(ScriptValue::Nil) => (),
                                Ok(value) => ctx
                                    .status_message
                                    .write_str(StatusMessageKind::Info, &value.to_string()),
                                Err(error) => match context.editor_loop {
                                    EditorLoop::Quit => return ModeOperation::Quit,
                                    EditorLoop::QuitAll => return ModeOperation::QuitAll,
                                    EditorLoop::Continue => {
                                        ctx.status_message.write_str(
                                            StatusMessageKind::Error,
                                            &error.to_string(),
                                        );
                                    }
                                },
                            },
                            _ => ctx
                                .status_message
                                .write_str(StatusMessageKind::Info, &value.to_string()),
                        }

                        ModeOperation::EnterMode(Mode::default())
                    }
                    Err(e) => match context.editor_loop {
                        EditorLoop::Quit => ModeOperation::Quit,
                        EditorLoop::QuitAll => ModeOperation::QuitAll,
                        EditorLoop::Continue => {
                            ctx.status_message.write_error(&e);
                            ModeOperation::EnterMode(Mode::default())
                        }
                    },
                }
            }
        }
    }
}
