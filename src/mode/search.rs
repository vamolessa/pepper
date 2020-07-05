use crate::mode::{poll_input, FromMode, InputResult, Mode, ModeContext, ModeOperation};

pub fn on_enter(ctx: ModeContext) {
    ctx.input.clear();
    update_search(ctx);
}

pub fn on_event(mut ctx: ModeContext, from_mode: &FromMode) -> ModeOperation {
    let operation = match poll_input(&mut ctx) {
        InputResult::Canceled => ModeOperation::EnterMode(from_mode.as_mode()),
        InputResult::Submited => ModeOperation::EnterMode(Mode::Normal),
        InputResult::Pending => ModeOperation::None,
    };

    update_search(ctx);
    operation
}

pub fn update_search(ctx: ModeContext) {
    for viewport in ctx.viewports.iter() {
        if let Some(handle) = viewport.current_buffer_view_handle() {
            let buffer_handle = ctx.buffer_views.get(handle).buffer_handle;
            if let Some(buffer) = ctx.buffers.get_mut(buffer_handle) {
                buffer.set_search(&ctx.input[..]);
            }
        };
    }
}
