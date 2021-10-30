use pepper::{
    client::ClientHandle,
    editor::{EditorContext, EditorFlow, KeysIterator},
    editor_utils::ReadLinePoll,
    mode::ModeKind,
    plugin::PluginHandle,
};

use crate::{client::Client, LspPlugin};

pub fn enter_rename_mode(
    ctx: &mut EditorContext,
    plugin_handle: PluginHandle,
    placeholder: &str,
    client: &Client,
) {
    fn on_client_keys(
        ctx: &mut EditorContext,
        _: ClientHandle,
        _: &mut KeysIterator,
        poll: ReadLinePoll,
    ) -> Option<EditorFlow> {
        match poll {
            ReadLinePoll::Pending => Some(EditorFlow::Continue),
            ReadLinePoll::Submitted => {
                if let Some(handle) = ctx.editor.mode.plugin_handle {
                    let lsp = ctx.plugins.get_as::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .current_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        client.finish_rename(&mut ctx.editor, &mut ctx.platform);
                    }
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
            ReadLinePoll::Canceled => {
                if let Some(handle) = ctx.editor.mode.plugin_handle {
                    let lsp = ctx.plugins.get_as::<LspPlugin>(handle);
                    if let Some(client) = lsp
                        .current_client_handle
                        .take()
                        .and_then(|h| lsp.get_mut(h))
                    {
                        client.cancel_current_request();
                    }
                }

                ctx.editor.enter_mode(ModeKind::default());
                Some(EditorFlow::Continue)
            }
        }
    }

    ctx.editor.read_line.set_prompt("rename:");

    ctx.editor.mode.plugin_handle = Some(plugin_handle);
    ctx.editor.mode.read_line_state.on_client_keys = on_client_keys;
    ctx.editor.enter_mode(ModeKind::ReadLine);
    ctx.editor.read_line.input_mut().push_str(placeholder);

    let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
    lsp.current_client_handle = Some(client.handle());
}
