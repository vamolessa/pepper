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
        ctx.read_line.set_prompt(":");
        ctx.read_line.set_input("");
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.read_line.set_input("");
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

                let (engine, mut ctx) = ctx.into_script_context();

                let code = ctx.read_line.input();
                const BUF_CAPACITY: usize = 256;
                let result = if code.len() > BUF_CAPACITY {
                    let code = String::from(code);
                    eval(engine, &mut ctx, &code)
                } else {
                    let mut buf = [0; BUF_CAPACITY];
                    buf[..code.len()].copy_from_slice(code.as_bytes());
                    let code = unsafe { std::str::from_utf8_unchecked(&buf[..code.len()]) };
                    eval(engine, &mut ctx, code)
                };

                if let Err(error) = result {
                    match ctx.editor_loop {
                        EditorLoop::Quit => return ModeOperation::Quit,
                        EditorLoop::QuitAll => return ModeOperation::QuitAll,
                        EditorLoop::Continue => ctx.status_message.write_error(&error),
                    }
                }

                ModeOperation::EnterMode(ctx.next_mode)
            }
        }
    }
}

fn eval<'a>(
    engine: &'a mut ScriptEngine,
    ctx: &mut ScriptContext<'a>,
    code: &str,
) -> ScriptResult<()> {
    engine.eval(ctx, code, |_, ctx, guard, value| {
        let value = match value {
            ScriptValue::Function(f) => f.call(&guard, ()),
            value => Ok(value),
        }?;
        match value {
            ScriptValue::Nil => (),
            value => ctx.status_message.write_fmt(
                StatusMessageKind::Info,
                format_args!("{}", value.display(&guard)),
            ),
        }
        Ok(())
    })
}
