use crate::{
    editor::{KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    script::{ScriptFunction, ScriptString},
};

pub const PROMPT_REGISTRY_KEY: &str = "read_line_prompt";
pub const CALLBACK_REGISTRY_KEY: &str = "read_line_callback";

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        match ctx
            .scripts
            .as_ref()
            .take_from_registry::<ScriptString>(PROMPT_REGISTRY_KEY)
        {
            Ok(prompt) => ctx.read_line.reset(prompt.to_str().unwrap_or("")),
            Err(_) => ctx.read_line.reset(""),
        }
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.read_line.reset("");
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        let input = match ctx.read_line.poll(keys) {
            ReadLinePoll::Pending => return ModeOperation::None,
            ReadLinePoll::Submitted => Some(String::from(ctx.read_line.input())),
            ReadLinePoll::Canceled => None,
        };

        let (engine, _, mut ctx) = ctx.script_context();

        match engine
            .as_ref()
            .take_from_registry::<ScriptFunction>(CALLBACK_REGISTRY_KEY)
            .and_then(|c| c.call(&mut ctx, input))
        {
            Ok(()) => (),
            Err(error) => {
                ctx.status_message.write_error(&error);
            }
        }

        ModeOperation::EnterMode(Mode::default())
    }
}
