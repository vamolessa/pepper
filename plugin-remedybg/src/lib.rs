use std::{
    fmt::Write,
    path::Path,
    process::{Command, Stdio},
};

use pepper::{
    buffer::{BufferBreakpoint, BufferHandle},
    command::{CommandManager, CompletionSource},
    editor::{Editor, EditorContext},
    events::{EditorEvent, EditorEventIter},
    platform::{Platform, PlatformProcessHandle, PlatformRequest, ProcessTag},
    plugin::{Plugin, PluginDefinition, PluginHandle},
    ResourceFile,
};

pub static DEFINITION: PluginDefinition = PluginDefinition {
    instantiate: |handle, ctx| {
        register_commands(&mut ctx.editor.commands, handle);
        Some(Plugin {
            data: Box::new(RemedybgPlugin::default()),

            on_editor_events,

            on_process_spawned,
            on_process_exit,

            ..Default::default()
        })
    },
    help_pages: &[ResourceFile {
        name: "remedybg_help.md",
        content: include_str!("../rc/help.md"),
    }],
};

enum ProcessState {
    NotRunning,
    Spawning,
    Running(PlatformProcessHandle),
}
impl Default for ProcessState {
    fn default() -> Self {
        Self::NotRunning
    }
}

const MAIN_PROCESS_ID: u32 = 0;
const CLEAR_BREAKPOINTS_PROCESS_ID: u32 = 1;
const ADD_BREAKPOINT_PROCESS_ID: u32 = 2;

#[derive(Default)]
pub(crate) struct RemedybgPlugin {
    breakpoints_changed: bool,
    process_state: ProcessState,
    pending_breakpoints: Vec<(BufferHandle, BufferBreakpoint)>,
}
impl RemedybgPlugin {
    pub fn spawn(
        &mut self,
        platform: &mut Platform,
        plugin_handle: PluginHandle,
        mut command: Command,
    ) {
        if !matches!(self.process_state, ProcessState::NotRunning) {
            return;
        }

        self.process_state = ProcessState::Spawning;

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        platform.requests.enqueue(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Plugin {
                plugin_handle,
                id: MAIN_PROCESS_ID,
            },
            command,
            buf_len: 128,
        });
    }

    pub fn begin_sync_breakpoints(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        plugin_handle: PluginHandle,
    ) {
        if !matches!(self.process_state, ProcessState::Running(_)) {
            return;
        }

        self.pending_breakpoints.clear();
        for buffer in editor.buffers.iter() {
            for &breakpoint in buffer.breakpoints() {
                self.pending_breakpoints.push((buffer.handle(), breakpoint));
            }
        }

        let mut command = Command::new("remedybg");
        command.arg("remove-all-breakpoints");

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        platform.requests.enqueue(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Plugin {
                plugin_handle,
                id: CLEAR_BREAKPOINTS_PROCESS_ID,
            },
            command,
            buf_len: 128,
        });
    }

    fn add_next_breakpoint(
        &mut self,
        editor: &mut Editor,
        platform: &mut Platform,
        plugin_handle: PluginHandle,
    ) {
        if !matches!(self.process_state, ProcessState::Running(_)) {
            return;
        }

        let (buffer_handle, breakpoint) = match self.pending_breakpoints.pop() {
            Some(buffer_handle_breakpoint_pair) => buffer_handle_breakpoint_pair,
            None => return,
        };

        let mut command = Command::new("remedybg");
        command.arg("add-breakpoint-at-file");

        {
            let mut arg = editor.string_pool.acquire();

            let current_directory = editor.current_directory.to_str();
            let buffer = editor.buffers.get(buffer_handle);
            let path = buffer.path.to_str();
            if let (Some(current_directory), Some(path)) = (current_directory, path) {
                if Path::new(path).is_relative() {
                    arg.push_str(current_directory);
                    if let Some(false) = current_directory
                        .chars()
                        .next_back()
                        .map(std::path::is_separator)
                    {
                        arg.push(std::path::MAIN_SEPARATOR);
                    }
                }
                arg.push_str(path);
            }
            command.arg(&arg);

            arg.clear();
            let _ = write!(arg, "{}", breakpoint.line_index + 1);
            command.arg(&arg);

            editor.string_pool.release(arg);
        }

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        platform.requests.enqueue(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Plugin {
                plugin_handle,
                id: ADD_BREAKPOINT_PROCESS_ID,
            },
            command,
            buf_len: 128,
        });
    }
}

fn register_commands(commands: &mut CommandManager, plugin_handle: PluginHandle) {
    let mut r = |name, completions, command_fn| {
        commands.register_command(Some(plugin_handle), name, completions, command_fn);
    };

    r("remedybg-spawn", &[CompletionSource::Files], |ctx, io| {
        let session_file = match io.args.try_next() {
            Some(file) => file,
            None => "session.rdbg",
        };
        io.args.assert_empty()?;

        let mut command = Command::new("remedybg");
        command.arg(session_file);

        let plugin_handle = io.plugin_handle();
        let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
        remedybg.spawn(&mut ctx.platform, plugin_handle, command);

        Ok(())
    });

    r(
        "remedybg-sync-breakpoints",
        &[CompletionSource::Files],
        |ctx, io| {
            io.args.assert_empty()?;

            let plugin_handle = io.plugin_handle();
            let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
            remedybg.begin_sync_breakpoints(&ctx.editor, &mut ctx.platform, plugin_handle);

            Ok(())
        },
    );
}

fn on_editor_events(plugin_handle: PluginHandle, ctx: &mut EditorContext) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);

    let mut events = EditorEventIter::new();
    while let Some(event) = events.next(&ctx.editor.events) {
        match event {
            EditorEvent::Idle => {
                if remedybg.breakpoints_changed {
                    remedybg.breakpoints_changed = false;
                    remedybg.begin_sync_breakpoints(&ctx.editor, &mut ctx.platform, plugin_handle);
                }
            }
            EditorEvent::BufferBreakpointsChanged { .. } => remedybg.breakpoints_changed = true,
            _ => (),
        }
    }
}

fn on_process_spawned(
    plugin_handle: PluginHandle,
    ctx: &mut EditorContext,
    id: u32,
    process_handle: PlatformProcessHandle,
) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    match id {
        MAIN_PROCESS_ID => remedybg.process_state = ProcessState::Running(process_handle),
        _ => (),
    }
}

fn on_process_exit(plugin_handle: PluginHandle, ctx: &mut EditorContext, id: u32) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    match id {
        MAIN_PROCESS_ID => remedybg.process_state = ProcessState::NotRunning,
        CLEAR_BREAKPOINTS_PROCESS_ID => {
            remedybg.add_next_breakpoint(&mut ctx.editor, &mut ctx.platform, plugin_handle)
        }
        ADD_BREAKPOINT_PROCESS_ID => {
            remedybg.add_next_breakpoint(&mut ctx.editor, &mut ctx.platform, plugin_handle)
        }
        _ => (),
    }
}

