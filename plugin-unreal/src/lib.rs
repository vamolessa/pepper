use pepper::{
    editor::EditorContext,
    help::HelpPages,
    platform::PlatformProcessHandle,
    plugin::{Plugin, PluginDefinition, PluginHandle},
    ResourceFile,
};

mod command;

pub static DEFAULT_BINDINGS_CONFIG: ResourceFile = ResourceFile {
    name: "unreal_default_bindings.pepper",
    content: include_str!("../rc/default_bindings.pepper"),
};

static HELP_PAGES: HelpPages = HelpPages::new(&[]);
pub static DEFINITION: PluginDefinition = PluginDefinition {
    instantiate: |handle, ctx| {
        command::register_commands(&mut ctx.editor.commands, handle);
        Some(Plugin {
            data: Box::new(UnrealPlugin::default()),
            on_process_spawned,
            on_process_output,
            on_process_exit,
            ..Default::default()
        })
    },
    help_pages: &HELP_PAGES,
};

#[derive(Default)]
pub(crate) struct UnrealPlugin {
    unreal_editor_path: String,
    unreal_source_path: String,
}

fn on_process_spawned(
    _handle: PluginHandle,
    _ctx: &mut EditorContext,
    _client_index: u32,
    _process_handle: PlatformProcessHandle,
) {
}

fn on_process_output(
    _plugin_handle: PluginHandle,
    _ctx: &mut EditorContext,
    _client_index: u32,
    _bytes: &[u8],
) {
}

fn on_process_exit(_handle: PluginHandle, _ctx: &mut EditorContext, _client_index: u32) {}
