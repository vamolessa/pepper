use crate::{
    client_event::Key,
    editor::{EditorLoop, KeysIterator, ReadLinePoll, StatusMessageKind},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    script::{ScriptContext, ScriptEngine, ScriptResult, ScriptValue},
    word_database::WordDatabase,
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        ctx.picker.reset();
        for entry in ctx.scripts.history() {
            ctx.picker.add_custom_entry(entry, "");
        }

        ctx.picker.filter(WordDatabase::empty(), "");
        ctx.read_line.reset(":");
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.picker.reset();
        ctx.read_line.reset("");
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match ctx.read_line.poll(keys) {
            ReadLinePoll::Pending => {
                keys.put_back();
                match keys.next() {
                    Key::Ctrl('n') | Key::Ctrl('j') => {
                        ctx.picker.move_cursor(1);
                        if let Some(entry) = ctx.picker.current_entry(WordDatabase::empty()) {
                            ctx.read_line.set_input(entry.name);
                        }
                    }
                    Key::Ctrl('p') | Key::Ctrl('k') => {
                        ctx.picker.move_cursor(-1);
                        if let Some(entry) = ctx.picker.current_entry(WordDatabase::empty()) {
                            ctx.read_line.set_input(entry.name);
                        }
                    }
                    _ => (),
                }

                ModeOperation::None
            }
            ReadLinePoll::Canceled => ModeOperation::EnterMode(Mode::default()),
            ReadLinePoll::Submitted => {
                ctx.scripts.add_to_history(ctx.read_line.input());
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
