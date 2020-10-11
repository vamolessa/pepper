use crate::{
    client_event::Key,
    editor::KeysIterator,
    mode::{poll_input, InputPollResult, Mode, ModeContext, ModeOperation, ModeState},
    word_database::WordDatabase,
};

pub struct State {
    pub on_pick: fn(&mut ModeContext),
}

impl Default for State {
    fn default() -> Self {
        Self { on_pick: |_| () }
    }
}

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        ctx.prompt.clear();
        ctx.picker.clear_filtered();
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.prompt.clear();
        ctx.picker.reset();
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match poll_input(&mut ctx.prompt, keys) {
            InputPollResult::Pending => {
                ctx.picker.filter(WordDatabase::empty(), &ctx.prompt);

                keys.put_back();
                match keys.next() {
                    Key::Ctrl('n') | Key::Ctrl('j') => {
                        ctx.picker.move_cursor(1);
                    }
                    Key::Ctrl('p') | Key::Ctrl('k') => {
                        ctx.picker.move_cursor(-1);
                    }
                    _ => (),
                }

                ModeOperation::None
            }
            InputPollResult::Submited => {
                (self.on_pick)(ctx);
                ModeOperation::EnterMode(Mode::default())
            }
            InputPollResult::Canceled => ModeOperation::EnterMode(Mode::default()),
        }
    }
}
