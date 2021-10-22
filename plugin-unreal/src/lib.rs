use std::path::Path;

use pepper::{
    help::HelpPages,
    plugin::{Plugin, PluginDefinition},
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
            ..Default::default()
        })
    },
    help_pages: &HELP_PAGES,
};

pub(crate) const UNREAL_PROJECT_EXTENSION: &str = ".uproject";

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
        name.trim_end_matches(UNREAL_PROJECT_EXTENSION)
    }
}

