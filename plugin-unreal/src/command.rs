use pepper::{command::CommandManager, plugin::PluginHandle};

use crate::UnrealPlugin;

pub fn register_commands(commands: &mut CommandManager, plugin_handle: PluginHandle) {
    let mut r = |name, completions, command_fn| {
        commands.register(Some(plugin_handle), name, completions, command_fn);
    };

    r("unreal-editor-path", &[], |ctx, io| {
        let path = match io.args.try_next() {
            Some(path) => path,
            None => {
                todo!();
            }
        };
        io.args.assert_empty()?;

        let unreal = ctx.plugins.get_as::<UnrealPlugin>(io.plugin_handle());
        unreal.unreal_editor_path.clear();
        unreal.unreal_editor_path.push_str(path);

        Ok(())
    });

    r("unreal-source-path", &[], |ctx, io| {
        let path = io.args.next()?;
        io.args.assert_empty()?;

        let unreal = ctx.plugins.get_as::<UnrealPlugin>(io.plugin_handle());
        unreal.unreal_source_path.clear();
        unreal.unreal_source_path.push_str(path);

        Ok(())
    });

    r("unreal-compile-game", &[], |_ctx, _io| {
        //
        Ok(())
    });

    r("unreal-run-game", &[], |_ctx, _io| {
        //
        Ok(())
    });

    r("unreal-open-editor", &[], |_ctx, _io| {
        //
        Ok(())
    });

    r("unreal-find-file", &[], |_ctx, _io| {
        //
        Ok(())
    });

    r("unreal-find-pattern", &[], |_ctx, _io| {
        //
        Ok(())
    });
}
