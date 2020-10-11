use crate::{
    editor::{EditorLoop, KeysIterator, StatusMessageKind},
    mode::{poll_input, InputPollResult, Mode, ModeContext, ModeOperation, ModeState},
    script::ScriptValue,
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

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match poll_input(&mut ctx.prompt, keys) {
            InputPollResult::Pending => ModeOperation::None,
            InputPollResult::Canceled => ModeOperation::EnterMode(Mode::default()),
            InputPollResult::Submited => {
                let (scripts, prompt, mut context) = ctx.script_context();
                match scripts.eval(&mut context, prompt) {
                    Ok(value) => {
                        match value {
                            ScriptValue::Nil => (),
                            ScriptValue::Function(f) => match f.call(&mut context, ()) {
                                Ok(ScriptValue::Nil) => (),
                                Ok(value) => context
                                    .status_message
                                    .write_str(StatusMessageKind::Info, &value.to_string()),
                                Err(error) => match context.editor_loop {
                                    EditorLoop::Quit => return ModeOperation::Quit,
                                    EditorLoop::QuitAll => return ModeOperation::QuitAll,
                                    EditorLoop::Continue => {
                                        context.status_message.write_str(
                                            StatusMessageKind::Error,
                                            &error.to_string(),
                                        );
                                    }
                                },
                            },
                            _ => context
                                .status_message
                                .write_str(StatusMessageKind::Info, &value.to_string()),
                        }

                        ModeOperation::EnterMode(context.next_mode)
                    }
                    Err(e) => match context.editor_loop {
                        EditorLoop::Quit => ModeOperation::Quit,
                        EditorLoop::QuitAll => ModeOperation::QuitAll,
                        EditorLoop::Continue => {
                            context.status_message.write_error(&e);
                            ModeOperation::EnterMode(Mode::default())
                        }
                    },
                }
            }
        }
    }
}
