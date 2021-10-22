use std::path::Path;

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
            data: Box::new(UnrealPlugin::new()),
            on_process_spawned,
            on_process_output,
            on_process_exit,
            ..Default::default()
        })
    },
    help_pages: &HELP_PAGES,
};

pub(crate) struct UnrealPlugin {
    pub(crate) unreal_project_path: String,
    pub(crate) unreal_editor_path: String,
    pub(crate) game_window_size: (u16, u16),
}

impl UnrealPlugin {
    pub(crate) fn new() -> Self {
        Self {
            unreal_project_path: String::new(),
            unreal_editor_path: String::new(),
            game_window_size: (960, 540),
        }
    }

    pub(crate) fn unreal_project_name(&self) -> &str {
        let name = match Path::new(&self.unreal_project_path).file_name() {
            Some(name) => name.to_str().unwrap(),
            None => return "",
        };
        name.trim_end_matches(".uproject")
    }
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
