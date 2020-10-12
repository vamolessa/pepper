use crate::{
    client_event::Key,
    editor::{KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
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
        ctx.read_line.reset(">");
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.read_line.reset("");
        ctx.picker.reset();
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match ctx.read_line.poll(keys) {
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

                ModeOperation::None
            }
            ReadLinePoll::Submited => {
                (self.on_pick)(ctx);
                ModeOperation::EnterMode(Mode::default())
            }
            ReadLinePoll::Canceled => ModeOperation::EnterMode(Mode::default()),
        }
    }
}
