use std::{
    io,
    ops::{Deref, DerefMut},
    path::PathBuf,
    process::{Command, Stdio},
};

use pepper::{
    editor::EditorContext,
    editor_utils::{hash_bytes, parse_process_command, MessageKind},
    events::{EditorEvent, EditorEventIter},
    glob::{Glob, InvalidGlobError},
    help::HelpPages,
    platform::{Platform, PlatformProcessHandle, PlatformRequest, ProcessTag},
    plugin::{CompletionContext, Plugin, PluginDefinition, PluginHandle},
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
    unreal_path: String,
}

fn on_process_spawned(
    handle: PluginHandle,
    ctx: &mut EditorContext,
    client_index: u32,
    process_handle: PlatformProcessHandle,
) {
}

fn on_process_output(
    plugin_handle: PluginHandle,
    ctx: &mut EditorContext,
    client_index: u32,
    bytes: &[u8],
) {
}

fn on_process_exit(handle: PluginHandle, ctx: &mut EditorContext, client_index: u32) {}

