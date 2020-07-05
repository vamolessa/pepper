use crate::mode::{poll_input, InputResult, ModeContext, Operation};

pub fn on_enter(ctx: ModeContext) {
    ctx.input.clear();
}

pub fn on_event(mut ctx: ModeContext) -> Operation {
    match poll_input(&mut ctx) {
        InputResult::Canceled => Operation::LeaveMode,
        InputResult::Submited => {
            // handle command here
            Operation::LeaveMode
        }
        InputResult::Pending => Operation::None,
    }
}
