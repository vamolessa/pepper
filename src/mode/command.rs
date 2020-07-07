use crate::{
    command::{CommandContext, CommandOperation},
    editor::KeysIterator,
    mode::{poll_input, FromMode, InputPollResult, ModeContext, ModeOperation},
};

pub fn on_enter(ctx: &mut ModeContext) {
    ctx.input.clear();
}

pub fn on_event(
    mut ctx: &mut ModeContext,
    keys: &mut KeysIterator,
    from_mode: &FromMode,
) -> ModeOperation {
    match poll_input(&mut ctx, keys) {
        InputPollResult::NoMatch => ModeOperation::NoMatch,
        InputPollResult::Pending => ModeOperation::None,
        InputPollResult::Canceled => ModeOperation::EnterMode(from_mode.as_mode()),
        InputPollResult::Submited => {
            let command_name;
            let command_args;
            if let Some(index) = ctx.input.find(' ') {
                command_name = &ctx.input[..index];
                command_args = &ctx.input[index..];
            } else {
                command_name = &ctx.input[..];
                command_args = "";
            }

            let command_context = CommandContext {
                buffers: ctx.buffers,
                buffer_views: ctx.buffer_views,
                viewports: ctx.viewports,
            };

            match ctx
                .commands
                .execute(command_name, command_context, command_args)
            {
                CommandOperation::Complete => ModeOperation::EnterMode(from_mode.as_mode()),
                CommandOperation::Quit => ModeOperation::Quit,
                CommandOperation::Error(error) => ModeOperation::Error(error),
            }
        }
    }
}
