use pepper::{
    buffer::{BufferHandle, BufferProperties},
    command::{CommandError, CommandIO, CommandManager},
    cursor::Cursor,
    editor::{Editor, EditorContext},
    editor_utils::parse_process_command,
    platform::Platform,
    plugin::PluginHandle,
};

use crate::UnrealPlugin;

pub fn register_commands(commands: &mut CommandManager, plugin_handle: PluginHandle) {
    let mut r = |name, completions, command_fn| {
        commands.register(Some(plugin_handle), name, completions, command_fn);
    };

    r("unreal-path", &[], |ctx, io| {
        let path = io.args.next()?;
        io.args.assert_empty()?;

        let unreal = ctx.plugins.get_as::<UnrealPlugin>(io.plugin_handle());
        unreal.unreal_path.clear();
        unreal.unreal_path.push_str(path);

        Ok(())
    });
}

