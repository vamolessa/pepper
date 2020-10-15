use crate::{
    client_event::Key,
    editor::{EditorLoop, KeysIterator, ReadLinePoll, StatusMessageKind},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    picker::CustomPickerEntry,
    script::ScriptValue,
    word_database::WordDatabase,
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        ctx.picker.reset();

        if ctx.scripts.history().count() > 0 {
            ctx.picker.add_custom_entry(CustomPickerEntry {
                name: String::new(),
                description: String::new(),
            });
        }

        for entry in ctx.scripts.history() {
            ctx.picker.add_custom_entry(CustomPickerEntry {
                name: entry.into(),
                description: String::new(),
            });
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
                        if let Some(entry) = ctx.picker.current_entry_name(WordDatabase::empty()) {
                            ctx.read_line.set_input(entry);
                        }
                    }
                    Key::Ctrl('p') | Key::Ctrl('k') => {
                        ctx.picker.move_cursor(-1);
                        if let Some(entry) = ctx.picker.current_entry_name(WordDatabase::empty()) {
                            ctx.read_line.set_input(entry);
                        }
                    }
                    _ => (),
                }

                ModeOperation::None
            }
            ReadLinePoll::Canceled => ModeOperation::EnterMode(Mode::default()),
            ReadLinePoll::Submitted => {
                ctx.scripts.add_to_history(ctx.read_line.input());
                let (scripts, read_line, mut context) = ctx.script_context();
                match scripts.eval(&mut context, read_line.input()) {
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
