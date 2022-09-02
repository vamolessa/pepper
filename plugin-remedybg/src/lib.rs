use std::{
    fmt,
    process::{Command, Stdio},
};

use pepper::{
    buffer::{BufferBreakpoint, BufferCollection, BufferHandle},
    command::{CommandError, CommandManager, CompletionSource},
    editor::EditorContext,
    editor_utils::{LogKind, to_absolute_path_string},
    events::{EditorEvent, EditorEventIter},
    platform::{Platform, PlatformProcessHandle, PlatformRequest, ProcessTag, IpcTag, IpcReadMode, PlatformIpcHandle},
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

            on_ipc_spawned,
            on_ipc_output,
            on_ipc_exit,

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

const CONTROL_PIPE_ID: u32 = 0;
const EVENT_PIPE_ID: u32 = 1;

#[derive(Default)]
pub(crate) struct RemedybgPlugin {
    breakpoints_changed: bool,
    process_state: ProcessState,
    session_name: String,
    pending_breakpoints: Vec<(BufferHandle, BufferBreakpoint)>,
}
impl RemedybgPlugin {
    pub fn spawn(
        &mut self,
        platform: &mut Platform,
        plugin_handle: PluginHandle,
        session_name: &str,
        session_file: Option<&str>,
    ) {
        if !matches!(self.process_state, ProcessState::NotRunning) {
            return;
        }

        self.process_state = ProcessState::Spawning;
        self.session_name.clear();
        self.session_name.push_str(session_name);

        let mut command = Command::new("remedybg");
        command.arg("--servername");
        command.arg(session_name);
        if let Some(session_file) = session_file {
            command.arg(session_file);
        }

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        platform.requests.enqueue(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Plugin {
                plugin_handle,
                id: 0,
            },
            command,
            buf_len: 128,
        });
    }

    pub fn begin_sync_breakpoints(
        &mut self,
        buffers: &BufferCollection,
        platform: &mut Platform,
        plugin_handle: PluginHandle,
    ) {
        if !matches!(self.process_state, ProcessState::Running(_)) {
            return;
        }

        self.pending_breakpoints.clear();
        for buffer in buffers.iter() {
            for &breakpoint in buffer.breakpoints() {
                self.pending_breakpoints.push((buffer.handle(), breakpoint));
            }
        }

        // TODO: finish this
    }
}

fn register_commands(commands: &mut CommandManager, plugin_handle: PluginHandle) {
    let mut r = |name, completions, command_fn| {
        commands.register_command(Some(plugin_handle), name, completions, command_fn);
    };

    r("remedybg-spawn", &[CompletionSource::Files], |ctx, io| {
        let session_file = io.args.try_next();
        io.args.assert_empty()?;

        let plugin_handle = io.plugin_handle();
        let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
        remedybg.spawn(&mut ctx.platform, plugin_handle, &ctx.editor.session_name, session_file);

        Ok(())
    });

    r(
        "remedybg-sync-breakpoints",
        &[CompletionSource::Files],
        |ctx, io| {
            io.args.assert_empty()?;

            let plugin_handle = io.plugin_handle();
            let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
            remedybg.begin_sync_breakpoints(&ctx.editor.buffers, &mut ctx.platform, plugin_handle);

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

        /*
        let mut command = Command::new("remedybg");
        for range in &arg_ranges.buf[..arg_ranges.len as usize] {
            let arg = &args_string[range.0 as usize..range.1 as usize];
            command.arg(arg);
        }
        */
        ctx.editor.string_pool.release(args_string);

        Ok(())
    });
}

fn on_editor_events(plugin_handle: PluginHandle, ctx: &mut EditorContext) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);

    let mut events = EditorEventIter::new();
    while let Some(event) = events.next(ctx.editor.events.reader()) {
        match event {
            // TODO: send breakpoints immediately
            EditorEvent::Idle => {
                if remedybg.breakpoints_changed {
                    remedybg.breakpoints_changed = false;
                    remedybg.begin_sync_breakpoints(
                        &ctx.editor.buffers,
                        &mut ctx.platform,
                        plugin_handle,
                    );
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
    _: u32,
    process_handle: PlatformProcessHandle,
) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    remedybg.process_state = ProcessState::Running(process_handle);
    remedybg.breakpoints_changed = true;

    // TODO: connect to ipc
    let mut control_path_buf = ctx.platform.buf_pool.acquire();
    let path_write = control_path_buf.write();
    path_write.extend_from_slice(remedybg.session_name.as_bytes());
    ctx.platform.requests.enqueue(PlatformRequest::ConnectToIpc {
        tag: IpcTag {
            plugin_handle,
            id: CONTROL_PIPE_ID,
        },
        path: control_path_buf,
        read: true,
        write: true,
        read_mode: IpcReadMode::MessageStream,
        buf_len: 1024,
    });

    let mut event_path_buf = ctx.platform.buf_pool.acquire();
    let path_write = event_path_buf.write();
    path_write.extend_from_slice(remedybg.session_name.as_bytes());
    path_write.extend_from_slice(b"-events");
    ctx.platform.requests.enqueue(PlatformRequest::ConnectToIpc {
        tag: IpcTag {
            plugin_handle,
            id: EVENT_PIPE_ID,
        },
        path: event_path_buf,
        read: true,
        write: false,
        read_mode: IpcReadMode::MessageStream,
        buf_len: 1024,
    });
}

fn on_process_exit(plugin_handle: PluginHandle, ctx: &mut EditorContext, _: u32) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    remedybg.process_state = ProcessState::NotRunning;
}

fn on_ipc_spawned(plugin_handle: PluginHandle, ctx: &mut EditorContext, id: u32, ipc_handle: PlatformIpcHandle) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    let _ = remedybg;
    let _ = ipc_handle;

    let ipc_name = match id {
        CONTROL_PIPE_ID => "control",
        EVENT_PIPE_ID => "event",
        _ => unreachable!(),
    };
    ctx.editor.logger.write(LogKind::Diagnostic).fmt(format_args!("remedybg: connected to {} ipc", ipc_name));
}

fn on_ipc_output(plugin_handle: PluginHandle, ctx: &mut EditorContext, id: u32, bytes: &[u8]) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    let _ = remedybg;

    let mut write = ctx.editor.logger.write(LogKind::Diagnostic);
    write.fmt(format_args!("remedybg: on ipc output {} with {} bytes", id, bytes.len()));
    if let Ok(text) = std::str::from_utf8(bytes) {
        write.str("\n");
        write.str(text);
    }
}

fn on_ipc_exit(plugin_handle: PluginHandle, ctx: &mut EditorContext, id: u32) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    let _ = remedybg;
    ctx.editor.logger.write(LogKind::Diagnostic).fmt(format_args!("remedybg: on ipc exit {}", id));
}


