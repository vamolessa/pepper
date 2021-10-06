use std::{
    any::Any,
    ops::{Deref, DerefMut},
    process::{Command, Stdio},
};

use crate::{
    client::ClientManager,
    editor::Editor,
    editor_utils::ResidualStrBytes,
    platform::{Platform, PlatformRequest, ProcessHandle, ProcessIndex, ProcessTag},
    ResourceFile,
};

pub struct PluginDefinition {
    pub create_fn: fn(&mut Editor, &mut Platform) -> Box<dyn Plugin>,
    pub help_pages: &'static [ResourceFile],
}

pub trait Plugin: 'static + AsAny {
    fn help_pages(&self) -> &'static [ResourceFile] {
        &[]
    }

    fn on_editor_events(
        &mut self,
        _editor: &mut Editor,
        _platform: &mut Platform,
        _clients: &mut ClientManager,
    ) {
    }

    fn on_process_spawned(
        &mut self,
        _editor: &mut Editor,
        _platform: &mut Platform,
        _clients: &mut ClientManager,
        _process_index: ProcessIndex,
        _process_handle: ProcessHandle,
    ) {
    }

    fn on_process_output(
        &mut self,
        _editor: &mut Editor,
        _platform: &mut Platform,
        _clients: &mut ClientManager,
        _process_index: ProcessIndex,
        _bytes: &[u8],
    ) {
    }

    fn on_process_exit(
        &mut self,
        _editor: &mut Editor,
        _platform: &mut Platform,
        _clients: &mut ClientManager,
        _process_index: ProcessIndex,
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
    pub output_residual_bytes: ResidualStrBytes,
}

#[derive(Default)]
pub struct PluginCollection {
    plugins: Vec<Box<dyn Plugin>>,
    processes: Vec<PluginProcess>,
}
impl PluginCollection {
    pub fn all(&self) -> impl Iterator<Item = &dyn Plugin> {
        self.plugins.iter().map(Deref::deref)
    }

    pub fn get<T>(&mut self, handle: PluginHandle) -> &mut T
    where
        T: Plugin,
    {
        let plugin = self.plugins[handle.0 as usize].deref_mut();
        plugin.as_any().downcast_mut::<T>().unwrap()
    }

    pub fn spawn_process(
        &mut self,
        platform: &mut Platform,
        mut command: Command,
        plugin_handle: PluginHandle,
        stdin: Stdio,
    ) -> ProcessIndex {
        let mut index = None;
        for (i, process) in self.processes.iter_mut().enumerate() {
            if !process.alive {
                process.alive = true;
                process.plugin_handle = plugin_handle;
                process.output_residual_bytes = ResidualStrBytes::default();
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
                    output_residual_bytes: ResidualStrBytes::default(),
                });
                index
            }
        };

        command.stdin(stdin);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::null());

        let index = ProcessIndex(index as _);
        platform.requests.enqueue(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Plugin(index),
            command,
            buf_len: 4 * 1024,
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
        process_index: ProcessIndex,
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
        process_index: ProcessIndex,
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
        process_index: ProcessIndex,
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

