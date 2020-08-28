use crate::{
    buffer::{Buffer, BufferCollection, BufferContent, TextRef},
    buffer_view::{BufferView, BufferViewCollection, BufferViewHandle},
    config::{Config, ParseConfigError},
    connection::{ConnectionWithClientHandle, TargetClient},
    editor::ClientTargetMap,
    editor_operation::{EditorOperation, EditorOperationSerializer, StatusMessageKind},
    keymap::{KeyMapCollection, ParseKeyMapError},
    mode::Mode,
    pattern::Pattern,
    script::{ScriptContext, ScriptEngine, ScriptResult},
    syntax::TokenKind,
    theme::ParseThemeError,
};

pub fn bind_all(scripts: &mut ScriptEngine) -> ScriptResult<()> {
    scripts.register_ctx_function("print", bindings::print)?;
    Ok(())
}

mod bindings {
    use super::*;

    pub fn print(ctx: &mut ScriptContext, message: String) -> ScriptResult<()> {
        println!("printing: {}", &message);
        ctx.operations.serialize(
            TargetClient::All,
            &EditorOperation::StatusMessage(StatusMessageKind::Info, &message),
        );
        Ok(())
    }
}
