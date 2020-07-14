use crate::{
    editor::KeysIterator,
    mode::{poll_input, FromMode, InputPollResult, Mode, ModeContext, ModeOperation},
};

pub fn on_enter(ctx: &mut ModeContext) {
    ctx.input.clear();
    update_search(ctx);
}

pub fn on_event(
    mut ctx: &mut ModeContext,
    keys: &mut KeysIterator,
    from_mode: &FromMode,
) -> ModeOperation {
    let operation = match poll_input(&mut ctx, keys) {
        InputPollResult::Pending => ModeOperation::None,
        InputPollResult::Canceled => ModeOperation::EnterMode(from_mode.as_mode()),
        InputPollResult::Submited => ModeOperation::EnterMode(Mode::Normal),
    };

    update_search(ctx);
    operation
}

pub fn update_search(ctx: &mut ModeContext) {
    if let Some(handle) = ctx.current_buffer_view_handle {
        let buffer_handle = ctx.buffer_views.get(handle).buffer_handle;
        if let Some(buffer) = ctx.buffers.get_mut(buffer_handle) {
            buffer.set_search(&ctx.input[..]);
        }
    };
}
