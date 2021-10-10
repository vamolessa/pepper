use pepper::{
    editor::{Editor, EditorControlFlow, KeysIterator},
    editor_utils::ReadLinePoll,
    mode::{ModeContext, ModeKind},
    plugin::PluginHandle,
};

use crate::{client::ClientOperation, LspPlugin};

pub fn enter_rename_mode(
    editor: &mut Editor,
    plugin_handle: PluginHandle,
    placeholder: &str,
) -> ClientOperation {
    fn on_client_keys(
        ctx: &mut ModeContext,
        _: &mut KeysIterator,
        poll: ReadLinePoll,
    ) -> Option<EditorControlFlow> {
        match poll {
            ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
            ReadLinePoll::Submitted => {
                if let Some(handle) = ctx.editor.mode.read_line_state.plugin_handle {
                    let mut lsp = ctx.editor.plugins.acquire::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .read_line_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        client.finish_rename(ctx.editor, ctx.platform);
                    }
                    ctx.editor.plugins.release(lsp);
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorControlFlow::Continue)
            }
            ReadLinePoll::Canceled => {
                if let Some(handle) = ctx.editor.mode.read_line_state.plugin_handle {
                    let mut lsp = ctx.editor.plugins.acquire::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .read_line_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        client.cancel_current_request();
                    }
                    ctx.editor.plugins.release(lsp);
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorControlFlow::Continue)
            }
        }
    }

    editor.read_line.set_prompt("rename:");

    let state = &mut editor.mode.read_line_state;
    state.on_client_keys = on_client_keys;
    state.plugin_handle = Some(plugin_handle);
    editor.enter_mode(ModeKind::ReadLine);
    editor.read_line.input_mut().push_str(placeholder);

    ClientOperation::EnteredReadLineMode
}
