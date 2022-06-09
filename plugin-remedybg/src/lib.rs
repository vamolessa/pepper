use std::{
    fmt,
    path::Path,
    process::{Command, Stdio},
};

use pepper::{
    buffer::{BufferBreakpoint, BufferHandle},
    command::{CommandError, CommandManager, CompletionSource},
    editor::{Editor, EditorContext},
    editor_utils::to_absolute_path_string,
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
const COMMAND_PROCESS_ID: u32 = 1;

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
            .stdin(Stdio::null())
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
            .stdin(Stdio::null())
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
            use fmt::Write;
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
            .stdin(Stdio::null())
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
        let session_file = io.args.try_next();
        io.args.assert_empty()?;

        let mut command = Command::new("remedybg");
        if let Some(session_file) = session_file {
            command.arg(session_file);
        }

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

    static COMMAND_COMPLETIONS: &[CompletionSource] = &[CompletionSource::Custom(&[
        "start",
        "start-paused",
        "stop",
        "attach",
        "continue",
        "run-to-cursor",
    ])];
    r("remedybg-command", COMMAND_COMPLETIONS, |ctx, io| {
        fn write_arg(args: &mut String, arg: &str) -> (u32, u32) {
            let start = args.len() as _;
            args.push_str(arg);
            let end = args.len() as _;
            (start, end)
        }

        #[derive(Default)]
        struct ArgRanges {
            pub buf: [(u32, u32); 3],
            pub len: u8,
        }
        impl ArgRanges {
            pub fn push(&mut self, range: (u32, u32)) {
                self.buf[self.len as usize] = range;
                self.len += 1;
            }
        }

        let mut args_string = ctx.editor.string_pool.acquire();
        let mut arg_ranges = ArgRanges::default();

        match io.args.next()? {
            "start" => arg_ranges.push(write_arg(&mut args_string, "start-debugging")),
            "start-paused" => {
                arg_ranges.push(write_arg(&mut args_string, "start-debugging"));
                arg_ranges.push(write_arg(&mut args_string, "1"));
            }
            "stop" => arg_ranges.push(write_arg(&mut args_string, "stop-debugging")),
            "attach" => {
                let process_id = io.args.next()?;
                arg_ranges.push(write_arg(&mut args_string, "attach-to-process-by-id"));
                arg_ranges.push(write_arg(&mut args_string, process_id));
            }
            "continue" => arg_ranges.push(write_arg(&mut args_string, "continue-execution")),
            "run-to-cursor" => {
                use fmt::Write;

                let buffer_view_handle = io.current_buffer_view_handle(ctx)?;
                let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
                let buffer_path = match ctx
                    .editor
                    .buffers
                    .get(buffer_view.buffer_handle)
                    .path
                    .to_str()
                {
                    Some(path) => path,
                    None => {
                        ctx.editor.string_pool.release(args_string);
                        return Err(CommandError::OtherStatic("buffer path is not utf-8"));
                    }
                };

                let current_directory = match ctx.editor.current_directory.to_str() {
                    Some(path) => path,
                    None => {
                        ctx.editor.string_pool.release(args_string);
                        return Err(CommandError::OtherStatic("current directory is not utf-8"));
                    }
                };

                arg_ranges.push(write_arg(&mut args_string, "run-to-cursor"));

                let start = args_string.len() as _;
                to_absolute_path_string(current_directory, buffer_path, &mut args_string);
                let end = args_string.len() as _;
                arg_ranges.push((start, end));

                let line_number = buffer_view.cursors.main_cursor().position.line_index + 1;
                let start = args_string.len() as _;
                write!(args_string, "{}", line_number).unwrap();
                let end = args_string.len() as _;
                arg_ranges.push((start, end));
            }
            _ => {
                ctx.editor.string_pool.release(args_string);
                return Err(CommandError::OtherStatic(
                    "invalid remedybg-debub operation",
                ));
            }
        }
        io.args.assert_empty()?;

        let plugin_handle = io.plugin_handle();
        let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);

        if !matches!(remedybg.process_state, ProcessState::Running(_)) {
            ctx.editor.string_pool.release(args_string);
            return Ok(());
        }

        let mut command = Command::new("remedybg");
        for range in &arg_ranges.buf[..arg_ranges.len as usize] {
            let arg = &args_string[range.0 as usize..range.1 as usize];
            command.arg(arg);
        }
        ctx.editor.string_pool.release(args_string);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        ctx.platform
            .requests
            .enqueue(PlatformRequest::SpawnProcess {
                tag: ProcessTag::Plugin {
                    plugin_handle,
                    id: COMMAND_PROCESS_ID,
                },
                command,
                buf_len: 128,
            });

        Ok(())
    });
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
