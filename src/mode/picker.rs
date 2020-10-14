use crate::{
    client_event::Key,
    editor::{KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    script::{ScriptFunction, ScriptString},
    word_database::WordDatabase,
};

pub const PROMPT_REGISTRY_KEY: &str = "picker_prompt";
pub const CALLBACK_REGISTRY_KEY: &str = "picker_callback";

pub struct State {
    on_event: fn(&mut ModeContext, &mut KeysIterator, ReadLinePoll),
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_event: |_, _, _| (),
        }
    }
}

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        match ctx
            .scripts
            .as_ref()
            .take_from_registry::<ScriptString>(PROMPT_REGISTRY_KEY)
        {
            Ok(prompt) => ctx.read_line.reset(prompt.to_str().unwrap_or(">")),
            Err(_) => ctx.read_line.reset(">"),
        }

        ctx.picker.filter(WordDatabase::empty(), "");
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.read_line.reset("");
        ctx.picker.reset();
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        let poll = ctx.read_line.poll(keys);
        (self.on_event)(ctx, keys, poll);

        let entry = match poll {
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

                return ModeOperation::None;
            }
            ReadLinePoll::Submitted => ctx
                .picker
                .current_entry_name(WordDatabase::empty())
                .map(|e| String::from(e)),
            ReadLinePoll::Canceled => None,
        };

        let (engine, _, mut ctx) = ctx.script_context();
        match engine
            .as_ref()
            .take_from_registry::<ScriptFunction>(CALLBACK_REGISTRY_KEY)
            .and_then(|c| c.call(&mut ctx, entry))
        {
            Ok(()) => (),
            Err(error) => {
                ctx.status_message.write_error(&error);
            }
        }

        ModeOperation::EnterMode(Mode::default())
    }
}
