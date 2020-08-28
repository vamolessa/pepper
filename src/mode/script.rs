use crate::{
    connection::TargetClient,
    editor::KeysIterator,
    editor_operation::{EditorOperation, StatusMessageKind},
    mode::{poll_input, FromMode, InputPollResult, ModeContext, ModeOperation},
    script::ScriptContext,
};

pub fn on_enter(ctx: &mut ModeContext) {
    ctx.input.clear();
    ctx.operations
        .serialize(TargetClient::All, &EditorOperation::InputKeep(0));
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
            let mut quit = false;
            let context = ScriptContext {
                quit: &mut quit,
                target_client: ctx.target_client,
                client_target_map: ctx.client_target_map,
                operations: ctx.operations,

                config: ctx.config,
                keymaps: ctx.keymaps,
                buffers: ctx.buffers,
                buffer_views: ctx.buffer_views,
                current_buffer_view_handle: ctx.current_buffer_view_handle,
            };

            match ctx.scripts.eval(context, &ctx.input[..]) {
                Ok(()) => ModeOperation::EnterMode(from_mode.as_mode()),
                Err(e) => match quit {
                    true => ModeOperation::Quit,
                    false => {
                        let message = e.to_string();
                        let op = EditorOperation::StatusMessage(StatusMessageKind::Error, &message);
                        ctx.operations.serialize(TargetClient::All, &op);
                        ModeOperation::EnterMode(from_mode.as_mode())
                    }
                },
            }
        }
    }
}
