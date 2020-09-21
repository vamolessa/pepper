use crate::{
    editor::KeysIterator,
    mode::{poll_input, InputPollResult, Mode, ModeContext, ModeOperation},
};

pub fn on_enter(ctx: &mut ModeContext) {
    ctx.input.clear();
    update_search(ctx);
}

pub fn on_exit(ctx: &mut ModeContext) {
    ctx.input.clear();
}

pub fn on_event(mut ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    match poll_input(&mut ctx, keys) {
        InputPollResult::Pending => {
            update_search(ctx);
            ModeOperation::None
        }
        InputPollResult::Submited | InputPollResult::Canceled => {
            ModeOperation::EnterMode(Mode::default())
        }
    }
}

pub fn update_search(ctx: &mut ModeContext) {
    let view_handle = ctx.current_buffer_view_handle();
    let buffer_views = &ctx.buffer_views;
    let buffers = &mut ctx.buffers;

    if let Some(buffer) = view_handle
        .and_then(|h| buffer_views.get(h))
        .and_then(|v| buffers.get_mut(v.buffer_handle))
    {
        buffer.set_search(&ctx.input);
    }
}
