use crate::{
    client_event::Key,
    client::ClientCollection,
    editor::{Editor, EditorLoop, KeysIterator, ReadLinePoll, StatusMessageKind},
    mode::{Mode, ModeKind, ModeOperation, ModeState},
    script::{ScriptContext, ScriptEngine, ScriptResult, ScriptValue},
};

#[derive(Default)]
pub struct State {
    history_index: usize,
}

impl ModeState for State {
    fn on_enter(editor: &mut Editor, _: &mut ClientCollection) {
        editor.mode.script_state.history_index = editor.scripts.history_len();
        editor.read_line.set_prompt(":");
        editor.read_line.set_input("");
    }

    fn on_exit(editor: &mut Editor, _: &mut ClientCollection) {
        editor.read_line.set_input("");
    }

    fn on_client_keys(
        editor: &mut Editor,
        _: &mut ClientCollection,
        keys: &mut KeysIterator,
    ) -> ModeOperation {
        let this = &mut editor.mode.script_state;
        match editor.read_line.poll(keys) {
            ReadLinePoll::Pending => {
                keys.put_back();
                match keys.next() {
                    Key::Ctrl('n') | Key::Ctrl('j') => {
                        this.history_index = editor
                            .scripts
                            .history_len()
                            .saturating_sub(1)
                            .min(this.history_index + 1);
                        let entry = editor.scripts.history_entry(this.history_index);
                        editor.read_line.set_input(entry);
                    }
                    Key::Ctrl('p') | Key::Ctrl('k') => {
                        this.history_index = this.history_index.saturating_sub(1);
                        let entry = editor.scripts.history_entry(this.history_index);
                        editor.read_line.set_input(entry);
                    }
                    _ => (),
                }
            }
            ReadLinePoll::Canceled => Mode::change_to(editor, ModeKind::default()),
            ReadLinePoll::Submitted => {
                let input = editor.read_line.input();
                if !input.starts_with(' ') {
                    editor.scripts.add_to_history(input);
                }

                let previous_mode_kind = editor.mode.kind();

                /*
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
                */

                if editor.mode.kind() == previous_mode_kind {
                    Mode::change_to(editor, ModeKind::default());
                }
            }
        }

        ModeOperation::None
    }
}

fn eval<'a>(
    engine: &'a mut ScriptEngine,
    ctx: &mut ScriptContext<'a>,
    code: &str,
) -> ScriptResult<()> {
    engine.eval(ctx, code, |_, ctx, guard, value| {
        match value {
            ScriptValue::Nil => (),
            ScriptValue::Function(f) => f.call(&guard, ())?,
            value => ctx.status_message.write_fmt(
                StatusMessageKind::Info,
                format_args!("{}", value.display(&guard)),
            ),
        }
        Ok(())
    })
}
