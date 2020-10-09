use crate::{
    editor::KeysIterator,
    mode::{poll_input, InputPollResult, Mode, ModeContext, ModeOperation, ModeState},
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        ctx.prompt.clear();
        ctx.prompt.push_str(&ctx.search);
        ctx.search.clear();
        update_search(ctx);
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.prompt.clear();
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match poll_input(&mut ctx.search, keys) {
            InputPollResult::Pending => {
                update_search(ctx);
                ModeOperation::None
            }
            InputPollResult::Submited => ModeOperation::EnterMode(Mode::default()),
            InputPollResult::Canceled => {
                ctx.search.clear();
                ctx.search.push_str(&ctx.prompt);
                ModeOperation::EnterMode(Mode::default())
            }
        }
    }
}

fn update_search(ctx: &mut ModeContext) {
    for buffer in ctx.buffers.iter_mut() {
        buffer.set_search("");
    }

    let handle = unwrap_or_return!(ctx.current_buffer_view_handle());
    let buffer_view = unwrap_or_return!(ctx.buffer_views.get(handle));
    let buffer = unwrap_or_return!(ctx.buffers.get_mut(buffer_view.buffer_handle));
    buffer.set_search(&ctx.search);
}
