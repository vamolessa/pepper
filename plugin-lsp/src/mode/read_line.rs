use pepper::{
    client::ClientHandle,
    editor::{Editor, EditorContext, EditorControlFlow, KeysIterator},
    editor_utils::ReadLinePoll,
    mode::ModeKind,
    plugin::PluginHandle,
};

use crate::{client::ClientOperation, LspPlugin};

pub fn enter_rename_mode(
    editor: &mut Editor,
    plugin_handle: PluginHandle,
    placeholder: &str,
) -> ClientOperation {
    fn on_client_keys(
        ctx: &mut EditorContext,
        _: ClientHandle,
        _: &mut KeysIterator,
        poll: ReadLinePoll,
    ) -> Option<EditorControlFlow> {
        match poll {
            ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
            ReadLinePoll::Submitted => {
                if let Some(handle) = ctx.editor.mode.read_line_state.plugin_handle {
                    let lsp = ctx.plugins.get::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .read_line_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        client.finish_rename(&mut ctx.editor, &mut ctx.platform);
                    }
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorControlFlow::Continue)
            }
            ReadLinePoll::Canceled => {
                if let Some(handle) = ctx.editor.mode.read_line_state.plugin_handle {
                    let lsp = ctx.plugins.get::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .read_line_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        client.cancel_current_request();
                    }
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
