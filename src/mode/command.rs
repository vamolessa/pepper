use crate::{
    command::{CommandContext, CommandOperation},
    connection::TargetClient,
    editor::{EditorOperation, KeysIterator},
    mode::{poll_input, FromMode, InputPollResult, ModeContext, ModeOperation},
};

pub fn on_enter(ctx: &mut ModeContext) {
    ctx.input.clear();
    ctx.operations
        .send(TargetClient::All, EditorOperation::InputKeep(0));
}

pub fn on_event(
    mut ctx: &mut ModeContext,
    keys: &mut KeysIterator,
    from_mode: &FromMode,
) -> ModeOperation {
    match poll_input(&mut ctx, keys) {
        InputPollResult::Pending => ModeOperation::None,
        InputPollResult::Canceled => ModeOperation::EnterMode(from_mode.as_mode()),
        InputPollResult::Submited => {
            let command_name;
            let command_args;
            if let Some(index) = ctx.input.find(' ') {
                command_name = &ctx.input[..index];
                command_args = &ctx.input[(index + 1)..];
            } else {
                command_name = &ctx.input[..];
                command_args = "";
            }

            let command_context = CommandContext {
                target_client: ctx.target_client,
                operations: ctx.operations,

                buffers: ctx.buffers,
                buffer_views: ctx.buffer_views,
                current_buffer_view_handle: ctx.current_buffer_view_handle,
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
