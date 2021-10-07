use pepper::{
    editor::{Editor, EditorControlFlow, KeysIterator},
    editor_utils::ReadLinePoll,
    mode::{Mode, ModeContext, ModeKind},
};

use crate::{client::ClientHandle, LspPlugin};

pub mod rename {
    use super::*;

    pub fn enter_mode(editor: &mut Editor, client_handle: ClientHandle, placeholder: &str) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => {
                    if let Some(handle) = ctx.editor.mode.read_line_state.plugin_handle {
                        let this = ctx.editor.plugins.acquire::<LspPlugin>(handle);
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
                        let this = ctx.editor.plugins.acquire::<LspPlugin>(handle);
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
        editor.read_line.set_prompt("rename:");
        /*
        let this = editor.plugins.acquire::<LspPlugin>(plugin_handle);
        this.read_line_client_handle = Some(client_handle);
        editor.plugins.release(this);
        */

        let state = &mut editor.mode.read_line_state;
        state.on_client_keys = on_client_keys;
        //state.plugin_handle = Some(plugin_handle);
        Mode::change_to(ctx, ModeKind::ReadLine);
        editor.read_line.input_mut().push_str(placeholder);
    }
}

