use crate::{
    client_event::Key,
    editor::{EditorLoop, KeysIterator, ReadLinePoll, StatusMessageKind},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    script::{ScriptContext, ScriptEngine, ScriptResult, ScriptValue},
};

#[derive(Default)]
pub struct State {
    history_index: usize,
}

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        self.history_index = ctx.scripts.history_len();
        ctx.read_line.reset(":");
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.read_line.reset("");
    }

    fn on_client_keys(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match ctx.read_line.poll(keys) {
            ReadLinePoll::Pending => {
                keys.put_back();
                match keys.next() {
                    Key::Ctrl('n') | Key::Ctrl('j') => {
                        self.history_index = ctx
                            .scripts
                            .history_len()
                            .saturating_sub(1)
                            .min(self.history_index + 1);
                        let entry = ctx.scripts.history_entry(self.history_index);
                        ctx.read_line.set_input(entry);
                    }
                    Key::Ctrl('p') | Key::Ctrl('k') => {
                        self.history_index = self.history_index.saturating_sub(1);
                        let entry = ctx.scripts.history_entry(self.history_index);
                        ctx.read_line.set_input(entry);
                    }
                    _ => (),
                }

                ModeOperation::None
            }
            ReadLinePoll::Canceled => ModeOperation::EnterMode(Mode::default()),
            ReadLinePoll::Submitted => {
                let input = ctx.read_line.input();
                if !input.starts_with(' ') {
                    ctx.scripts.add_to_history(input);
                }

                let (engine, read_line, mut context) = ctx.script_context();

                if let Err(error) = eval(engine, &mut context, read_line.input()) {
                    match context.editor_loop {
                        EditorLoop::Quit => return ModeOperation::Quit,
                        EditorLoop::QuitAll => return ModeOperation::QuitAll,
                        EditorLoop::Continue => {
                            context
                                .status_message
                                .write_str(StatusMessageKind::Error, &error.to_string());
                        }
                    }
                }

                ModeOperation::EnterMode(context.next_mode)
            }
        }
    }
}

fn eval<'a>(
    engine: &'a mut ScriptEngine,
    ctx: &mut ScriptContext<'a>,
    code: &str,
) -> ScriptResult<()> {
    let value = engine.eval(ctx, code, |_, _, mut guard, value| match value {
        ScriptValue::Function(f) => f.call(&mut guard, ()),
        value => Ok(value),
    })?;

    match value {
        ScriptValue::Nil => (),
        value => ctx
            .status_message
            .write_str(StatusMessageKind::Info, &value.to_string()),
    }

    Ok(())
}
