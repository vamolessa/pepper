use std::{
    any::Any,
    ops::{Deref, DerefMut},
};

use crate::{
    buffer::BufferHandle, buffer_position::BufferPosition, buffer_view::BufferViewHandle,
    editor::EditorContext, help, platform::PlatformProcessHandle,
};

pub trait PluginDefinition {
    fn instantiate(&self, ctx: &mut EditorContext, plugin_handle: PluginHandle) -> Box<dyn Plugin>;
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
    fn on_editor_events(&mut self, _: &mut EditorContext, _: PluginHandle) {}

    fn on_process_spawned(
        &mut self,
        _: &mut EditorContext,
        _: PluginHandle,
        _: u32,
        _: PlatformProcessHandle,
    ) {
    }

    fn on_process_output(&mut self, _: &mut EditorContext, _: PluginHandle, _: u32, _: &[u8]) {}

    fn on_process_exit(&mut self, _: &mut EditorContext, _: PluginHandle, _: u32) {}

    fn on_completion_flow(
        &mut self,
        _: &mut EditorContext,
        _: PluginHandle,
        _: &CompletionContext,
    ) -> Option<CompletionFlow> {
        None
    }

    fn on_completion(
        &mut self,
        _: &mut EditorContext,
        _: PluginHandle,
        _: &CompletionContext,
    ) -> bool {
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

pub struct PluginGuard<T> {
    plugin: Box<T>,
    handle: PluginHandle,
}
impl<T> Deref for PluginGuard<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.plugin
    }
}
impl<T> DerefMut for PluginGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.plugin
    }
}
impl<T> Drop for PluginGuard<T> {
    fn drop(&mut self) {
        panic!("forgot to call 'release' on PluginCollection");
    }
}

// TODO: make PluginIterGuard or something

struct DummyPlugin;
impl DummyPlugin {
    pub fn new() -> Box<dyn Plugin> {
        Box::new(Self)
    }
}
impl Plugin for DummyPlugin {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(u32);

#[derive(Default)]
pub struct PluginCollection {
    plugins: Vec<Box<dyn Plugin>>,
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
        let slot = &mut self.plugins[handle.0 as usize];
        if !slot.as_any().is::<T>() {
            panic!(
                "plugin with handle {} was not of type '{}'",
                handle.0,
                std::any::type_name::<T>()
            );
        }

        let mut plugin = DummyPlugin::new();
        std::mem::swap(&mut plugin, slot);

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
        let raw = plugin.plugin.deref_mut() as *mut dyn Plugin;
        std::mem::forget(plugin);
        let plugin = unsafe { Box::from_raw(raw) };
        self.plugins[index] = plugin;
    }

    pub(crate) fn on_editor_events(ctx: &mut EditorContext) {
        let mut plugin = DummyPlugin::new();
        for i in 0..ctx.plugins.plugins.len() {
            let plugin_handle = PluginHandle(i as _);
            std::mem::swap(&mut plugin, &mut ctx.plugins.plugins[i]);
            plugin.on_editor_events(ctx, plugin_handle);
            std::mem::swap(&mut plugin, &mut ctx.plugins.plugins[i]);
        }
    }

    pub(crate) fn on_process_spawned(
        ctx: &mut EditorContext,
        plugin_handle: PluginHandle,
        process_id: u32,
        process_handle: PlatformProcessHandle,
    ) {
        let mut plugin = DummyPlugin::new();
        std::mem::swap(
            &mut plugin,
            &mut ctx.plugins.plugins[plugin_handle.0 as usize],
        );
        plugin.on_process_spawned(ctx, plugin_handle, process_id, process_handle);
        std::mem::swap(
            &mut plugin,
            &mut ctx.plugins.plugins[plugin_handle.0 as usize],
        );
    }

    pub(crate) fn on_process_output(
        ctx: &mut EditorContext,
        plugin_handle: PluginHandle,
        process_id: u32,
        bytes: &[u8],
    ) {
        let mut plugin = DummyPlugin::new();
        std::mem::swap(
            &mut plugin,
            &mut ctx.plugins.plugins[plugin_handle.0 as usize],
        );
        plugin.on_process_output(ctx, plugin_handle, process_id, bytes);
        std::mem::swap(
            &mut plugin,
            &mut ctx.plugins.plugins[plugin_handle.0 as usize],
        );
    }

    pub(crate) fn on_process_exit(
        ctx: &mut EditorContext,
        plugin_handle: PluginHandle,
        process_id: u32,
    ) {
        let mut plugin = DummyPlugin::new();
        std::mem::swap(
            &mut plugin,
            &mut ctx.plugins.plugins[plugin_handle.0 as usize],
        );
        plugin.on_process_exit(ctx, plugin_handle, process_id);
        std::mem::swap(
            &mut plugin,
            &mut ctx.plugins.plugins[plugin_handle.0 as usize],
        );
    }
}

