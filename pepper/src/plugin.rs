use std::{any::Any, ops::DerefMut, process::Command};

use crate::{
    buffer::BufferHandle,
    buffer_position::BufferPosition,
    buffer_view::BufferViewHandle,
    client::ClientManager,
    editor::{Editor, EditorContext},
    help,
    platform::{Platform, PlatformProcessHandle, PlatformRequest, ProcessId, ProcessTag},
};

pub struct PluginContext<'a> {
    pub editor: &'a mut Editor,
    pub platform: &'a mut Platform,
    pub clients: &'a mut ClientManager,
    pub plugin_handle: PluginHandle,
}

pub trait PluginDefinition {
    fn instantiate(&self, _: &mut PluginContext) -> Box<dyn Plugin>;
    fn help_pages(&self) -> &'static help::HelpPages;
}

pub enum CompletionFlow {
    ForceCompletion,
    Cancel,
}

pub struct CompletionContext {
    pub buffer_handle: BufferHandle,
    pub buffer_view_handle: BufferViewHandle,
    pub position: BufferPosition,
    pub last_char: char,
}

pub trait Plugin: 'static + AsAny {
    fn on_editor_events(&mut self, _: &mut PluginContext) {}

    fn on_process_spawned(
        &mut self,
        _: &mut PluginContext,
        _: ProcessId,
        _: PlatformProcessHandle,
    ) {
    }

    fn on_process_output(&mut self, _: &mut PluginContext, _: ProcessId, _: &[u8]) {}

    fn on_process_exit(&mut self, _: &mut PluginContext, _: ProcessId) {}

    fn on_completion_flow(
        &mut self,
        _: &mut PluginContext,
        _: &CompletionContext,
    ) -> Option<CompletionFlow> {
        None
    }

    fn on_completion(&mut self, _: &mut PluginContext, _: &CompletionContext) -> bool {
        false
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(u32);

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

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut dyn Plugin> {
        self.plugins.iter_mut().map(DerefMut::deref_mut)
    }

    fn get_mut(&mut self, handle: PluginHandle) -> &mut dyn Plugin {
        self.plugins[handle.0 as usize].deref_mut()
    }

    pub fn get_mut_as<T>(&mut self, handle: PluginHandle) -> &mut T
    where
        T: Plugin,
    {
        match self.plugins[handle.0 as usize].as_any().downcast_mut::<T>() {
            Some(plugin) => plugin,
            None => panic!(
                "plugin with handle {} was not of type '{}'",
                handle.0,
                std::any::type_name::<T>()
            ),
        }
    }

    /*
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
    */

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

        let id = ProcessId(index as _);
        platform.requests.enqueue(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Plugin(id),
            command,
            buf_len,
        });

        id
    }

    pub(crate) fn on_editor_events(ctx: &mut EditorContext) {
        let (plugins, mut ctx) = get_plugins_and_ctx(ctx);
        for (i, plugin) in plugins.iter_mut().enumerate() {
            ctx.plugin_handle = PluginHandle(i as _);
            plugin.on_editor_events(&mut ctx);
        }
    }

    fn plugin_handle_form_process(&mut self, process_id: ProcessId) -> PluginHandle {
        self.processes[process_id.0 as usize].plugin_handle
    }

    pub(crate) fn on_process_spawned(
        ctx: &mut EditorContext,
        process_id: ProcessId,
        process_handle: PlatformProcessHandle,
    ) {
        let (plugins, mut ctx) = get_plugins_and_ctx(ctx);
        ctx.plugin_handle = plugins.plugin_handle_form_process(process_id);
        let plugin = plugins.get_mut(ctx.plugin_handle);
        plugin.on_process_spawned(&mut ctx, process_id, process_handle);
    }

    pub(crate) fn on_process_output(ctx: &mut EditorContext, process_id: ProcessId, bytes: &[u8]) {
        let (plugins, mut ctx) = get_plugins_and_ctx(ctx);
        ctx.plugin_handle = plugins.plugin_handle_form_process(process_id);
        let plugin = plugins.get_mut(ctx.plugin_handle);
        plugin.on_process_output(&mut ctx, process_id, bytes);
    }

    pub(crate) fn on_process_exit(ctx: &mut EditorContext, process_id: ProcessId) {
        let (plugins, mut ctx) = get_plugins_and_ctx(ctx);
        ctx.plugin_handle = plugins.plugin_handle_form_process(process_id);
        let plugin = plugins.get_mut(ctx.plugin_handle);
        plugin.on_process_exit(&mut ctx, process_id);
    }
}

fn get_plugins_and_ctx(ctx: &mut EditorContext) -> (&mut PluginCollection, PluginContext) {
    let plugins = &mut ctx.plugins;
    let ctx = PluginContext {
        editor: &mut ctx.editor,
        platform: &mut ctx.platform,
        clients: &mut ctx.clients,
        plugin_handle: PluginHandle(0),
    };
    (plugins, ctx)
}
