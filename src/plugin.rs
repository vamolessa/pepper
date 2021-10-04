use crate::{
    client::ClientManager,
    editor::Editor,
    platform::{Platform, ProcessHandle},
};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(u32);

pub struct Plugin {
    //
}

#[derive(Default)]
pub struct PluginCollection {
    plugins: Vec<Plugin>,
}
impl PluginCollection {
    pub fn get(&self, handle: PluginHandle) -> &Plugin {
        &self.plugins[handle.0 as usize]
    }

    pub fn get_mut(&mut self, handle: PluginHandle) -> &mut Plugin {
        &mut self.plugins[handle.0 as usize]
    }

    pub fn on_process_spawned(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        handle: PluginHandle,
        process_id: u32,
        process_handle: ProcessHandle,
    ) {
        let plugin = editor.plugins.get(handle);
    }

    pub fn on_process_output(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        handle: PluginHandle,
        process_id: u32,
        bytes: &[u8],
    ) {
        let plugin = editor.plugins.get(handle);
    }

    pub fn on_process_exit(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        handle: PluginHandle,
        process_id: u32,
    ) {
        let plugin = editor.plugins.get(handle);
    }
}

