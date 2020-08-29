use crate::{
    connection::TargetClient,
    editor::{EditorLoop, KeysIterator},
    editor_operation::EditorOperation,
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
            let mut editor_loop = EditorLoop::Continue;
            let context = ScriptContext {
                editor_loop: &mut editor_loop,
                target_client: ctx.target_client,
                operations: ctx.operations,

                config: ctx.config,
                keymaps: ctx.keymaps,
                buffers: ctx.buffers,
                buffer_views: ctx.buffer_views,
                current_buffer_view_handle: ctx.current_buffer_view_handle,
            };

            match ctx.scripts.eval(context, &ctx.input[..]) {
                Ok(()) => ModeOperation::EnterMode(from_mode.as_mode()),
                Err(e) => match editor_loop {
                    EditorLoop::Quit => ModeOperation::Quit,
                    EditorLoop::QuitAll => ModeOperation::QuitAll,
                    EditorLoop::Continue => {
                        use std::error::Error;
                        let mut message = e.to_string();
                        let mut error = e.source();
                        while let Some(e) = error {
                            message.push('\n');
                            let s = e.to_string();
                            message.push_str(&s);
                            error = e.source();
                        }
                        ctx.operations.serialize_error(&message);
                        ModeOperation::EnterMode(from_mode.as_mode())
                    }
                },
            }
        }
    }
}
