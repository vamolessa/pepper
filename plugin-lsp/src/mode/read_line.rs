use pepper::{
    editor::{EditorControlFlow, KeysIterator},
    editor_utils::ReadLinePoll,
    mode::{Mode, ModeContext, ModeKind},
};

use crate::{client::ClientOperation, LspPlugin};

pub fn enter_rename_mode(ctx: &mut ModeContext, placeholder: &str) -> ClientOperation {
    fn on_client_keys(
        ctx: &mut ModeContext,
        _: &mut KeysIterator,
        poll: ReadLinePoll,
    ) -> Option<EditorControlFlow> {
        match poll {
            ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
            ReadLinePoll::Submitted => {
                if let Some(handle) = ctx.editor.mode.read_line_state.plugin_handle {
                    let mut this = ctx.editor.plugins.acquire::<LspPlugin>(handle);
                    if let Some(client) = this
                        .read_line_client_handle
                        .take()
                        .and_then(|h| this.get_mut(h))
                    {
                        client.finish_rename(ctx.editor, ctx.platform);
                    }
                    ctx.editor.plugins.release(this);
                }
                Mode::change_to(ctx, ModeKind::default());
                Some(EditorControlFlow::Continue)
            }
            ReadLinePoll::Canceled => {
                if let Some(handle) = ctx.editor.mode.read_line_state.plugin_handle {
                    let mut this = ctx.editor.plugins.acquire::<LspPlugin>(handle);
                    if let Some(client) = this
                        .read_line_client_handle
                        .take()
                        .and_then(|h| this.get_mut(h))
                    {
                        client.cancel_current_request();
                    }
                    ctx.editor.plugins.release(this);
                }
                Mode::change_to(ctx, ModeKind::default());
                Some(EditorControlFlow::Continue)
            }
        }
    }

    // TODO: figure a way to set these values
    ctx.editor.read_line.set_prompt("rename:");
    /*
    let this = ctx.editor.plugins.acquire::<LspPlugin>(plugin_handle);
    this.read_line_client_handle = Some(client_handle);
    ctx.editor.plugins.release(this);
    */

    let state = &mut ctx.editor.mode.read_line_state;
    state.on_client_keys = on_client_keys;
    //state.plugin_handle = Some(plugin_handle);
    Mode::change_to(ctx, ModeKind::ReadLine);
    ctx.editor.read_line.input_mut().push_str(placeholder);

    ClientOperation::EnteredReadLineMode
}

