use std::{
    any::Any,
    ops::{Deref, DerefMut},
    process::Command,
};

use crate::{
    client::ClientManager,
    editor::Editor,
    help,
    platform::{Platform, PlatformRequest, ProcessHandle, ProcessId, ProcessTag},
};

pub trait PluginDefinition {
    fn instantiate(&self, _: &mut Editor, _: &mut Platform, _: PluginHandle) -> Box<dyn Plugin>;
    fn help_pages(&self) -> &'static help::HelpPages;
}

pub trait Plugin: 'static + AsAny {
    fn on_editor_events(&mut self, _: &mut Editor, _: &mut Platform, _: &mut ClientManager) {}

    fn on_process_spawned(
        &mut self,
        _: &mut Editor,
        _: &mut Platform,
        _: &mut ClientManager,
        _: ProcessId,
        _: ProcessHandle,
    ) {
    }

    fn on_process_output(
        &mut self,
        _: &mut Editor,
        _: &mut Platform,
        _: &mut ClientManager,
        _: ProcessId,
        _: &[u8],
    ) {
    }

    fn on_process_exit(
        &mut self,
        _: &mut Editor,
        _: &mut Platform,
        _: &mut ClientManager,
        _: ProcessId,
    ) {
    }
}

pub trait AsAny: Any {
    fn as_any(&mut self) -> &mut dyn Any;
}
impl<T> AsAny for T
where
    T: Plugin,
{
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(u32);

pub struct PluginGuard<T> {
    handle: PluginHandle,
    plugin: Box<T>,
}
impl<T> Deref for PluginGuard<T>
where
    T: Plugin,
{
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.plugin
    }
}
impl<T> DerefMut for PluginGuard<T>
where
    T: Plugin,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.plugin
    }
}
impl<T> Drop for PluginGuard<T> {
    fn drop(&mut self) {
        panic!("forgot to call 'release' on PluginCollection");
    }
}

struct DummyPlugin;
impl DummyPlugin {
    pub fn new() -> Box<dyn Plugin> {
        Box::new(DummyPlugin)
    }
}
impl Plugin for DummyPlugin {}

struct PluginProcess {
    pub alive: bool,
    pub plugin_handle: PluginHandle,
}

#[derive(Default)]
pub struct PluginCollection {
    plugins: Vec<Box<dyn Plugin>>,
    processes: Vec<PluginProcess>,
}
impl PluginCollection {
    pub(crate) fn next_handle(&self) -> PluginHandle {
        PluginHandle(self.plugins.len() as _)
    }

    pub(crate) fn add(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    pub fn acquire<T>(&mut self, handle: PluginHandle) -> PluginGuard<T>
    where
        T: Plugin,
    {
        if !self.plugins[handle.0 as usize].as_any().is::<T>() {
            panic!(
                "plugin with handle {} was not of type '{}'",
                handle.0,
                std::any::type_name::<T>()
            );
        }

        let mut plugin = DummyPlugin::new();
        std::mem::swap(&mut plugin, &mut self.plugins[handle.0 as usize]);

        let plugin = unsafe {
            let raw = Box::into_raw(plugin);
            Box::from_raw(raw as *mut T)
        };

        PluginGuard { plugin, handle }
    }

    pub fn release<T>(&mut self, mut plugin: PluginGuard<T>)
    where
        T: Plugin,
    {
        let index = plugin.handle.0 as usize;
        let plugin = unsafe {
            let raw = plugin.plugin.deref_mut() as *mut dyn Plugin;
            std::mem::forget(plugin);
            Box::from_raw(raw)
        };
        self.plugins[index] = plugin;
    }

    pub fn spawn_process(
        &mut self,
        platform: &mut Platform,
        command: Command,
        plugin_handle: PluginHandle,
        buf_len: usize,
    ) -> ProcessId {
        let mut index = None;
        for (i, process) in self.processes.iter_mut().enumerate() {
            if !process.alive {
                process.alive = true;
                process.plugin_handle = plugin_handle;
                index = Some(i);
                break;
            }
        }
        let index = match index {
            Some(index) => index,
            None => {
                let index = self.processes.len();
                self.processes.push(PluginProcess {
                    alive: true,
                    plugin_handle,
                });
                index
            }
        };

        let index = ProcessId(index as _);
        platform.requests.enqueue(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Plugin(index),
            command,
            buf_len,
        });

        index
    }

    pub(crate) fn on_editor_events(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
    ) {
        let mut plugin = DummyPlugin::new();
        for i in 0..editor.plugins.plugins.len() {
            std::mem::swap(&mut plugin, &mut editor.plugins.plugins[i]);
            plugin.on_editor_events(editor, platform, clients);
            std::mem::swap(&mut plugin, &mut editor.plugins.plugins[i]);
        }
    }

    pub(crate) fn on_process_spawned(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        process_index: ProcessId,
        process_handle: ProcessHandle,
    ) {
        let index = editor.plugins.processes[process_index.0 as usize]
            .plugin_handle
            .0 as usize;
        let mut plugin = DummyPlugin::new();
        std::mem::swap(&mut plugin, &mut editor.plugins.plugins[index]);
        plugin.on_process_spawned(editor, platform, clients, process_index, process_handle);
        std::mem::swap(&mut plugin, &mut editor.plugins.plugins[index]);
    }

    pub(crate) fn on_process_output(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        process_index: ProcessId,
        bytes: &[u8],
    ) {
        let index = editor.plugins.processes[process_index.0 as usize]
            .plugin_handle
            .0 as usize;
        let mut plugin = DummyPlugin::new();
        std::mem::swap(&mut plugin, &mut editor.plugins.plugins[index]);
        plugin.on_process_output(editor, platform, clients, process_index, bytes);
        std::mem::swap(&mut plugin, &mut editor.plugins.plugins[index]);
    }

    pub(crate) fn on_process_exit(
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut ClientManager,
        process_index: ProcessId,
    ) {
        let index = editor.plugins.processes[process_index.0 as usize]
            .plugin_handle
            .0 as usize;
        let mut plugin = DummyPlugin::new();
        std::mem::swap(&mut plugin, &mut editor.plugins.plugins[index]);
        plugin.on_process_exit(editor, platform, clients, process_index);
        std::mem::swap(&mut plugin, &mut editor.plugins.plugins[index]);
    }
}

