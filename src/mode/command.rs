use crate::{
    command::{CommandContext, CommandOperation},
    mode::{poll_input, FromMode, InputResult, ModeContext, ModeOperation},
};

pub fn on_enter(ctx: ModeContext) {
    ctx.input.clear();
}

pub fn on_event(mut ctx: ModeContext, from_mode: &FromMode) -> ModeOperation {
    match poll_input(&mut ctx) {
        InputResult::Canceled => ModeOperation::EnterMode(from_mode.as_mode()),
        InputResult::Submited => {
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
                CommandOperation::None => ModeOperation::EnterMode(from_mode.as_mode()),
                CommandOperation::Quit => ModeOperation::Quit,
                CommandOperation::Error(error) => ModeOperation::Error(error),
            }
        }
        InputResult::Pending => ModeOperation::None,
    }
}
