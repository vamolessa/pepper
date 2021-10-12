use std::any::Any;

use crate::{
    buffer::BufferHandle,
    buffer_position::{BufferPosition, BufferRange},
    editor::EditorContext,
    help,
    platform::PlatformProcessHandle,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(u32);

#[derive(Clone, Copy)]
pub struct PluginDefinition {
    pub instantiate: fn(PluginHandle, &mut EditorContext) -> Plugin,
    pub help_pages: &'static help::HelpPages,
}

pub struct Plugin {
    pub data: Box<dyn Any>,
    pub on_editor_events: fn(PluginHandle, &mut EditorContext),
    pub on_process_spawned: fn(PluginHandle, &mut EditorContext, u32, PlatformProcessHandle),
    pub on_process_output: fn(PluginHandle, &mut EditorContext, u32, &[u8]),
    pub on_process_exit: fn(PluginHandle, &mut EditorContext, u32),
    pub on_completion:
        fn(PluginHandle, &mut EditorContext, &CompletionContext) -> Option<CompletionFlow>,
}
impl Default for Plugin {
    fn default() -> Self {
        Self {
            data: Box::new(()),
            on_editor_events: |_, _| (),
            on_process_spawned: |_, _, _, _| (),
            on_process_output: |_, _, _, _| (),
            on_process_exit: |_, _, _| (),
            on_completion: |_, _, _| None,
        }
    }
}

pub enum CompletionFlow {
    Completing,
    Cancel,
}

pub struct CompletionContext {
    pub buffer_handle: BufferHandle,
    pub word_range: BufferRange,
    pub cursor_position: BufferPosition,
    pub completion_requested: bool,
}

#[derive(Default)]
pub struct PluginCollection {
    plugins: Vec<Plugin>,
}
impl PluginCollection {
    pub(crate) fn add(ctx: &mut EditorContext, definition: PluginDefinition) {
        help::add_help_pages(definition.help_pages);

        let handle = PluginHandle(ctx.plugins.plugins.len() as _);
        let plugin = (definition.instantiate)(handle, ctx);
        ctx.plugins.plugins.push(plugin);
    }

    pub fn get_as<T>(&mut self, handle: PluginHandle) -> &mut T
    where
        T: Any,
    {
        match self.plugins[handle.0 as usize].data.downcast_mut::<T>() {
            Some(plugin) => plugin,
            None => panic!(
                "plugin with handle {} was not of type '{}'",
                handle.0,
                std::any::type_name::<T>()
            ),
        }
    }

    pub(crate) fn get(&self, handle: PluginHandle) -> &Plugin {
        &self.plugins[handle.0 as usize]
    }

    pub(crate) fn handles(&self) -> impl Iterator<Item = PluginHandle> {
        std::iter::repeat(())
            .take(self.plugins.len())
            .enumerate()
            .map(|(i, _)| PluginHandle(i as _))
    }

    pub(crate) fn on_editor_events(ctx: &mut EditorContext) {
        for i in 0..ctx.plugins.plugins.len() {
            let handle = PluginHandle(i as _);
            let f = ctx.plugins.plugins[i].on_editor_events;
            f(handle, ctx);
        }
    }

    pub(crate) fn on_process_spawned(
        ctx: &mut EditorContext,
        plugin_handle: PluginHandle,
        process_id: u32,
        process_handle: PlatformProcessHandle,
    ) {
        let f = ctx.plugins.plugins[plugin_handle.0 as usize].on_process_spawned;
        f(plugin_handle, ctx, process_id, process_handle);
    }

    pub(crate) fn on_process_output(
        ctx: &mut EditorContext,
        plugin_handle: PluginHandle,
        process_id: u32,
        bytes: &[u8],
    ) {
        let f = ctx.plugins.plugins[plugin_handle.0 as usize].on_process_output;
        f(plugin_handle, ctx, process_id, bytes);
    }

    pub(crate) fn on_process_exit(
        ctx: &mut EditorContext,
        plugin_handle: PluginHandle,
        process_id: u32,
    ) {
        let f = ctx.plugins.plugins[plugin_handle.0 as usize].on_process_exit;
        f(plugin_handle, ctx, process_id);
    }
}

