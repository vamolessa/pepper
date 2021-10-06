use pepper::{
    editor::{EditorControlFlow, KeysIterator},
    editor_utils::ReadLinePoll,
    mode::{Mode, ModeContext, ModeKind},
    plugin::PluginHandle,
};

use crate::{ClientHandle, LspPlugin};

pub mod rename {
    use super::*;

    pub fn enter_mode(
        ctx: &mut ModeContext,
        plugin_handle: PluginHandle,
        client_handle: ClientHandle,
        placeholder: &str,
    ) {
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
                        let platform = &mut *ctx.platform;
                        LspPlugin::access(ctx.editor, handle, |e, c| {
                            c.finish_rename(e, platform);
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
                ReadLinePoll::Canceled => {
                    if let Some(handle) = ctx.editor.mode.read_line_state.lsp_client_handle {
                        LspPlugin::access(ctx.editor, handle, |_, c| {
                            c.cancel_current_request();
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
            }
        }

        ctx.editor.read_line.set_prompt("rename:");
        let state = &mut ctx.editor.mode.read_line_state;
        state.on_client_keys = on_client_keys;
        state.lsp_client_handle = Some(client_handle);
        Mode::change_to(ctx, ModeKind::ReadLine);
        ctx.editor.read_line.input_mut().push_str(placeholder);
    }
}

