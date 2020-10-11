use crate::{
    editor::KeysIterator,
    mode::{poll_input, InputPollResult, Mode, ModeContext, ModeOperation, ModeState},
};

pub struct State {
    pub on_pick: fn(&mut ModeContext) -> ModeOperation,
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_pick: |_| ModeOperation::None,
        }
    }
}

impl ModeState for State {
    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.picker.reset();
    }

    fn on_event(&mut self, context: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        ModeOperation::None
    }
}
