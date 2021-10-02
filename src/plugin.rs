use crate::command::CommandContext;

pub mod api;
mod api_impl;

pub type PluginInitFn = extern "C" fn(api: &api::PluginApi, ctx: &mut CommandContext) -> Plugin;
pub type PluginDeinitFn = extern "C" fn(userdata: api::PluginUserData);

pub fn api() -> &'static api::PluginApi {
    use api_impl::*;
    static PLUGIN_API: api::PluginApi = api::PluginApi {
        register_command,
        write_to_statusbar,
    };
    &PLUGIN_API
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(u32);

pub struct Plugin {
    pub userdata: api::PluginUserData,
    pub deinit_fn: Option<PluginDeinitFn>,
}

#[derive(Default)]
pub struct PluginCollection {
    plugins: Vec<Plugin>,
}
impl PluginCollection {
    pub fn load(ctx: &mut CommandContext, init_fn: PluginInitFn) {
        let handle = PluginHandle(ctx.editor.plugins.plugins.len() as _);
        ctx.plugin_handle = handle;
        let plugin = init_fn(api(), ctx);
        ctx.editor.plugins.plugins.push(plugin);
    }

    pub fn get_userdata(&self, handle: PluginHandle) -> api::PluginUserData {
        self.plugins[handle.0 as usize].userdata
    }
}

