use std::{fmt::Write, fs, path::Path, process::Stdio};

use pepper::{
    buffer::BufferProperties,
    buffer_position::{BufferPosition, BufferRange},
    command::{CommandError, CommandIO, CommandManager},
    editor::EditorContext,
    editor_utils::{parse_process_command, MessageKind},
    platform::{PlatformRequest, ProcessTag},
    plugin::{PluginCollection, PluginHandle},
};

use crate::{UnrealPlugin, UNREAL_PROJECT_EXTENSION};

pub fn register_commands(commands: &mut CommandManager, plugin_handle: PluginHandle) {
    let mut r = |name, completions, command_fn| {
        commands.register(Some(plugin_handle), name, completions, command_fn);
    };

    r("unreal-project-path", &[], |ctx, io| {
        let plugin = ctx.plugins.get_as::<UnrealPlugin>(io.plugin_handle());
        plugin.unreal_project_path.clear();

        match io.args.try_next() {
            Some(path) => plugin.unreal_project_path.push_str(path),
            None => {
                let mut found = false;
                if let Ok(entries) = fs::read_dir(".") {
                    for entry in entries {
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(_) => continue,
                        };
                        match entry.file_type() {
                            Ok(t) if t.is_file() => (),
                            _ => continue,
                        }
                        let file_name = entry.file_name();
                        if let Some(path) = file_name.to_str() {
                            if path.ends_with(UNREAL_PROJECT_EXTENSION) {
                                plugin.unreal_project_path.push_str(path);
                                found = true;
                                break;
                            }
                        }
                    }
                }

                if !found {
                    return Err(CommandError::OtherStatic(
                        "could not find unreal project file",
                    ));
                }
            }
        }

        io.args.assert_empty()?;
        Ok(())
    });

    r("unreal-editor-path", &[], |ctx, io| {
        let path = match io.args.try_next() {
            Some(path) => path,
            None => {
                todo!();
            }
        };
        io.args.assert_empty()?;

        let plugin = ctx.plugins.get_as::<UnrealPlugin>(io.plugin_handle());
        plugin.unreal_editor_path.clear();
        plugin.unreal_editor_path.push_str(path);

        Ok(())
    });

    r("unreal-game-window-size", &[], |ctx, io| {
        let width = io
            .args
            .next()?
            .parse()
            .map_err(|_| CommandError::OtherStatic("could not parse game window width"))?;
        let height = io
            .args
            .next()?
            .parse()
            .map_err(|_| CommandError::OtherStatic("could not parse game window height"))?;
        io.args.assert_empty()?;

        let plugin = ctx.plugins.get_as::<UnrealPlugin>(io.plugin_handle());
        plugin.game_window_size = (width, height);

        Ok(())
    });

    r("unreal-open-editor", &[], |ctx, io| {
        spawn_process(ctx, io, None, |io, plugin, command| {
            let map = io.args.try_next().unwrap_or("");
            let _ = write!(
                command,
                "'{}/Engine/Binaries/Win64/UE4Editor.exe' '{}' '{}'",
                &plugin.unreal_editor_path, &plugin.unreal_project_path, map,
            );
        })?;
        ctx.editor
            .status_bar
            .write(MessageKind::Info)
            .str("unreal: opening editor...");
        Ok(())
    });

    r("unreal-compile-clean", &[], |ctx, io| {
        spawn_process(ctx, io, None, |_, plugin, command| {
            let _ = write!(
                command,
                "'{}/Engine/Build/BatchFiles/Clean.bat' '{}Editor' Win64 Development '{}'",
                &plugin.unreal_editor_path,
                plugin.unreal_project_name(),
                &plugin.unreal_project_path,
            );
        })?;
        ctx.editor
            .status_bar
            .write(MessageKind::Info)
            .str("unreal: cleaning...");
        Ok(())
    });

    r("unreal-compile-game", &[], |ctx, io| {
        spawn_process(ctx, io, Some("build.log"), |_, plugin, command| {
            let _ = write!(
                command,
                "'{}/Engine/Build/BatchFiles/Build.bat' '{}Editor' Win64 Development '{}' -waitmutex -NoHotReload",
                &plugin.unreal_editor_path,
                plugin.unreal_project_name(),
                &plugin.unreal_project_path,
            );
        })
    });

    r("unreal-run-game", &[], |ctx, io| {
        spawn_process(ctx, io, None, |io, plugin, command| {
            let map = io.args.try_next().unwrap_or("");
            let _ = write!(
                command,
                "'{}/Engine/Binaries/Win64/UE4Editor.exe' '{}' '{}' -game -log -windowed -resx={} -resy={}",
                &plugin.unreal_editor_path,
                plugin.unreal_project_path,
                map,
                plugin.game_window_size.0,
                plugin.game_window_size.1,
            );
        })?;
        ctx.editor
            .status_bar
            .write(MessageKind::Info)
            .str("unreal: running...");
        Ok(())
    });
}

fn get<'a>(
    plugins: &'a mut PluginCollection,
    io: &mut CommandIO,
) -> Result<&'a mut UnrealPlugin, CommandError> {
    let plugin = plugins.get_as::<UnrealPlugin>(io.plugin_handle());
    if plugin.unreal_project_path.is_empty() {
        return Err(CommandError::OtherStatic("unreal project path is not set"));
    }
    if plugin.unreal_editor_path.is_empty() {
        return Err(CommandError::OtherStatic("unreal editor path is not set"));
    }
    Ok(plugin)
}

fn spawn_process(
    ctx: &mut EditorContext,
    io: &mut CommandIO,
    buffer_name: Option<&str>,
    command_fn: fn(&mut CommandIO, &mut UnrealPlugin, &mut String),
) -> Result<(), CommandError> {
    let plugin = get(&mut ctx.plugins, io)?;
    let mut command_text = ctx.editor.string_pool.acquire();
    command_fn(io, plugin, &mut command_text);
    let command = parse_process_command(&command_text);
    let command = command.ok_or(CommandError::OtherOwned(format!(
        "could not parse unreal command: '{}'",
        &command_text
    )));
    ctx.editor.string_pool.release(command_text);
    io.args.assert_empty()?;

    let mut command = command?;
    command.stdin(Stdio::null());
    command.stderr(Stdio::null());

    match buffer_name {
        Some(buffer_name) => {
            command.stdout(Stdio::piped());

            let client_handle = io.client_handle()?;
            let buffer_view_handle = match ctx.editor.buffer_view_handle_from_path(
                client_handle,
                Path::new(buffer_name),
                BufferProperties::scratch(),
                true,
            ) {
                Ok(handle) => handle,
                Err(error) => {
                    ctx.editor
                        .status_bar
                        .write(MessageKind::Error)
                        .fmt(format_args!("{}", error));
                    return Ok(());
                }
            };

            let buffer_view = ctx.editor.buffer_views.get_mut(buffer_view_handle);
            buffer_view.cursors.mut_guard().clear();

            let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle);
            let range = BufferRange::between(BufferPosition::zero(), buffer.content().end());
            buffer.delete_range(&mut ctx.editor.word_database, range, &mut ctx.editor.events);

            let client = ctx.clients.get_mut(client_handle);
            client.set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);
        }
        None => {
            command.stdout(Stdio::null());
            let request = PlatformRequest::SpawnProcess {
                tag: ProcessTag::Ignored,
                command,
                buf_len: 0,
            };
            ctx.platform.requests.enqueue(request);
        }
    }

    Ok(())
}

