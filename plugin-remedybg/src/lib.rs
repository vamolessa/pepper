use std::{
    fmt,
    path::Path,
    process::{Command, Stdio},
};

use pepper::{
    buffer::{BufferBreakpoint, BufferCollection, BufferHandle},
    command::{CommandError, CommandManager, CompletionSource},
    editor::{Editor, EditorContext},
    editor_utils::{StringPool, to_absolute_path_string, LogKind},
    events::{EditorEvent, EditorEventIter},
    platform::{
        IpcReadMode, IpcTag, Platform, PlatformIpcHandle, PlatformProcessHandle, PlatformRequest,
        PooledBuf, ProcessTag,
    },
    plugin::{Plugin, PluginDefinition, PluginHandle},
    serialization::{Serialize, DeserializeError, },
    ResourceFile,
};

mod protocol;

use protocol::{
    RemedybgBool, RemedybgId, RemedybgCommandKind, RemedybgCommandResult, RemedybgEvent, RemedybgStr, RemedybgBreakpoint,
};

pub static DEFINITION: PluginDefinition = PluginDefinition {
    instantiate: |handle, ctx| {
        register_commands(&mut ctx.editor.commands, handle);
        Some(Plugin {
            data: Box::new(RemedybgPlugin::default()),

            on_editor_events,

            on_process_spawned,
            on_process_exit,

            on_ipc_connected,
            on_ipc_output,
            on_ipc_close,

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

enum BreakpointSyncState {
    Synced,
    WaitingForRemedybgBreakpoints,
}
impl Default for BreakpointSyncState {
    fn default() -> Self {
        Self::Synced
    }
}

const CONTROL_PIPE_ID: u32 = 0;
const EVENT_PIPE_ID: u32 = 1;

#[derive(Default)]
pub(crate) struct RemedybgPlugin {
    process_state: ProcessState,
    session_name: String,

    breakpoint_sync_state: BreakpointSyncState,
    pending_commands: Vec<RemedybgCommandKind>,
    control_ipc_handle: Option<PlatformIpcHandle>,
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

    fn begin_send_command(
        &mut self,
        platform: &mut Platform,
        command_kind: RemedybgCommandKind,
    ) -> Result<CommandSender, CommandError> {
        match self.control_ipc_handle {
            Some(ipc_handle) => {
                self.pending_commands.push(command_kind);

                let mut buf = platform.buf_pool.acquire();
                let write = buf.write();
                command_kind.serialize(write);
                let sender = CommandSender { ipc_handle, buf };
                Ok(sender)
            }
            None => Err(CommandError::OtherStatic("remedybg is not running")),
        }
    }

    pub fn begin_sync_breakpoints(
        &mut self,
        editor: &mut Editor,
        platform: &mut Platform,
    ) -> Result<(), CommandError> {
        self.breakpoint_sync_state = BreakpointSyncState::WaitingForRemedybgBreakpoints;
        let sender = self.begin_send_command(platform, RemedybgCommandKind::GetBreakpoints)?;
        sender.send(platform);

        /*
        {
            let sender =
                self.begin_send_command(platform, RemedybgCommandKind::DeleteAllBreakpoints)?;
            sender.send(platform);
        }

        let mut file_path_buf = editor.string_pool.acquire();
        for buffer in editor.buffers.iter() {
            let current_directory = &editor.current_directory;
            let buffer_path = &buffer.path;

            file_path_buf.clear();
            if get_absolue_file_path(current_directory, buffer_path, &mut file_path_buf).is_err() {
                continue;
            }

            for &breakpoint in buffer.breakpoints() {
                let mut sender = self.begin_send_command(
                    platform,
                    RemedybgCommandKind::AddBreakpointAtFilenameLine,
                )?;
                let write = sender.write();
                RemedybgStr(&file_path_buf).serialize(write);
                let line = (breakpoint.line_index + 1) as u32;
                line.serialize(write);
                RemedybgStr("").serialize(write);
                sender.send(platform);
            }
        }

        editor.string_pool.release(file_path_buf);
        */
        Ok(())
    }
}

fn get_absolue_file_path(
    current_directory: &Path,
    buffer_path: &Path,
    file_path: &mut String,
) -> Result<(), CommandError> {
    let current_directory = match current_directory.to_str() {
        Some(path) => path,
        None => return Err(CommandError::OtherStatic("current directory is not utf-8")),
    };
    let buffer_path = match buffer_path.to_str() {
        Some(path) => path,
        None => return Err(CommandError::OtherStatic("buffer path is not utf-8")),
    };

    to_absolute_path_string(current_directory, buffer_path, file_path);
    Ok(())
}

struct CommandSender {
    ipc_handle: PlatformIpcHandle,
    buf: PooledBuf,
}
impl CommandSender {
    pub fn write(&mut self) -> &mut Vec<u8> {
        self.buf.write()
    }

    pub fn send(self, platform: &mut Platform) {
        platform.requests.enqueue(PlatformRequest::WriteToIpc {
            handle: self.ipc_handle,
            buf: self.buf,
        })
    }
}
fn begin_send_command(
    ctx: &mut EditorContext,
    plugin_handle: PluginHandle,
    command_kind: RemedybgCommandKind,
) -> Result<CommandSender, CommandError> {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    remedybg.begin_send_command(&mut ctx.platform, command_kind)
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
        remedybg.spawn(
            &mut ctx.platform,
            plugin_handle,
            &ctx.editor.session_name,
            session_file,
        );

        Ok(())
    });

    r(
        "remedybg-sync-breakpoints",
        &[CompletionSource::Files],
        |ctx, io| {
            io.args.assert_empty()?;
            let plugin_handle = io.plugin_handle();
            let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
            remedybg.begin_sync_breakpoints(&mut ctx.editor, &mut ctx.platform)
        },
    );

    static START_DEBUGGING_COMPLETIONS: &[CompletionSource] =
        &[CompletionSource::Custom(&["paused"])];
    r(
        "remedybg-start-debugging",
        START_DEBUGGING_COMPLETIONS,
        |ctx, io| {
            let start_paused = match io.args.try_next() {
                Some("paused") => true,
                Some(_) => return Err(CommandError::OtherStatic("invalid arg")),
                None => false,
            };
            io.args.assert_empty()?;

            let mut sender =
                begin_send_command(ctx, io.plugin_handle(), RemedybgCommandKind::StartDebugging)?;
            let write = sender.write();
            RemedybgBool(start_paused).serialize(write);
            sender.send(&mut ctx.platform);
            Ok(())
        },
    );

    r("remedybg-stop-debugging", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender =
            begin_send_command(ctx, io.plugin_handle(), RemedybgCommandKind::StartDebugging)?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-attach-to-process", &[], |ctx, io| {
        let process_arg = io.args.next()?;
        io.args.assert_empty()?;

        let process = process_arg.parse::<u32>();
        let mut sender = match process {
            Ok(_) => begin_send_command(
                ctx,
                io.plugin_handle(),
                RemedybgCommandKind::AttachToProcessByPid,
            )?,
            Err(_) => begin_send_command(
                ctx,
                io.plugin_handle(),
                RemedybgCommandKind::AttachToProcessByName,
            )?,
        };
        let write = sender.write();
        match process {
            Ok(pid) => pid.serialize(write),
            Err(_) => process_arg.serialize(write),
        }
        RemedybgBool(false).serialize(write);
        protocol::RDBG_IF_DEBUGGING_TARGET_STOP_DEBUGGING.serialize(write);
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-step-into", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender =
            begin_send_command(ctx, io.plugin_handle(), RemedybgCommandKind::StepIntoByLine)?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-step-over", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender =
            begin_send_command(ctx, io.plugin_handle(), RemedybgCommandKind::StepOverByLine)?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-step-out", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender = begin_send_command(ctx, io.plugin_handle(), RemedybgCommandKind::StepOut)?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-continue-execution", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender = begin_send_command(
            ctx,
            io.plugin_handle(),
            RemedybgCommandKind::ContinueExecution,
        )?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-run-to-cursor", &[], |ctx, io| {
        io.args.assert_empty()?;

        let buffer_view_handle = io.current_buffer_view_handle(ctx)?;
        let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
        let buffer_path = &ctx.editor.buffers.get(buffer_view.buffer_handle).path;
        let current_directory = &ctx.editor.current_directory;

        let mut file_path = ctx.editor.string_pool.acquire();
        if let Err(error) = get_absolue_file_path(current_directory, buffer_path, &mut file_path) {
            ctx.editor.string_pool.release(file_path);
            return Err(error);
        }

        let line_number = buffer_view.cursors.main_cursor().position.line_index + 1;
        let line_number = line_number as u32;

        let mut sender = match begin_send_command(
            ctx,
            io.plugin_handle(),
            RemedybgCommandKind::RunToFileAtLine,
        ) {
            Ok(sender) => sender,
            Err(error) => {
                ctx.editor.string_pool.release(file_path);
                return Err(error);
            }
        };
        let write = sender.write();
        RemedybgStr(&file_path).serialize(write);
        line_number.serialize(write);
        sender.send(&mut ctx.platform);

        ctx.editor.string_pool.release(file_path);
        Ok(())
    });

    r("remedybg-break-execution", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender =
            begin_send_command(ctx, io.plugin_handle(), RemedybgCommandKind::BreakExecution)?;
        sender.send(&mut ctx.platform);
        Ok(())
    });
}

fn on_editor_events(plugin_handle: PluginHandle, ctx: &mut EditorContext) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);

    let mut events = EditorEventIter::new();
    while let Some(event) = events.next(ctx.editor.events.reader()) {
        match event {
            EditorEvent::BufferBreakpointsChanged { .. } => {
                // TODO: send only breakpoints that changed
                let _ = remedybg.begin_sync_breakpoints(&mut ctx.editor, &mut ctx.platform);
                break;
            }
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

    let mut control_path_buf = ctx.platform.buf_pool.acquire();
    let path_write = control_path_buf.write();
    path_write.extend_from_slice(remedybg.session_name.as_bytes());
    ctx.platform
        .requests
        .enqueue(PlatformRequest::ConnectToIpc {
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
    ctx.platform
        .requests
        .enqueue(PlatformRequest::ConnectToIpc {
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

fn get_ipc_name(ipc_id: u32) -> &'static str {
    match ipc_id {
        CONTROL_PIPE_ID => "control",
        EVENT_PIPE_ID => "event",
        _ => "unknown",
    }
}

fn on_ipc_connected(
    plugin_handle: PluginHandle,
    ctx: &mut EditorContext,
    id: u32,
    ipc_handle: PlatformIpcHandle,
) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    if id == CONTROL_PIPE_ID {
        remedybg.control_ipc_handle = Some(ipc_handle);
        let _ = remedybg.begin_sync_breakpoints(&mut ctx.editor, &mut ctx.platform);
    }

    let ipc_name = get_ipc_name(id);
    ctx.editor
        .logger
        .write(LogKind::Diagnostic)
        .fmt(format_args!("remedybg: connected to {} ipc", ipc_name));
}

enum ControlResponseError {
    DeserializeError(DeserializeError),
    RemedybgCommandResult(RemedybgCommandResult),
    CommandError(CommandError),
}
impl From<DeserializeError> for ControlResponseError {
    fn from(other: DeserializeError) -> Self {
        Self::DeserializeError(other)
    }
}
impl From<CommandError> for ControlResponseError {
    fn from(other: CommandError) -> Self {
        Self::CommandError(other)
    }
}
impl fmt::Display for ControlResponseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::DeserializeError(DeserializeError::InvalidData) => f.write_str("deserialize error: invalid data"),
            Self::DeserializeError(DeserializeError::InsufficientData) => f.write_str("deserialize error: insufficient data"),
            Self::RemedybgCommandResult(result) => f.write_fmt(format_args!("result: {}", result)),
            Self::CommandError(error) => error.fmt(f),
        }
    }
}

fn on_control_response(
    remedybg: &mut RemedybgPlugin,
    string_pool: &mut StringPool,
    platform: &mut Platform,
    command_kind: RemedybgCommandKind,
    mut bytes: &[u8]
) -> Result<(), ControlResponseError> {
    match RemedybgCommandResult::deserialize(&mut bytes) {
        Ok(RemedybgCommandResult::Ok) => (),
        Ok(result) => return Err(ControlResponseError::RemedybgCommandResult(result)),
        Err(error) => return Err(error.into()),
    }

    match command_kind {
        RemedybgCommandKind::GetBreakpoints => {
            if let BreakpointSyncState::WaitingForRemedybgBreakpoints = remedybg.breakpoint_sync_state {
                remedybg.breakpoint_sync_state = BreakpointSyncState::Synced;

                let breakpoint_count = u16::deserialize(&mut bytes)?;
                for _ in 0..breakpoint_count {
                    let id = RemedybgId::deserialize(&mut bytes)?;
                    let _enabled = RemedybgBool::deserialize(&mut bytes)?;
                    let _module_name = RemedybgStr::deserialize(&mut bytes)?;
                    let _condition_expr = RemedybgStr::deserialize(&mut bytes)?;
                    let breakpoint = RemedybgBreakpoint::deserialize(&mut bytes)?;
                    if let RemedybgBreakpoint::FilenameLine { filename, line_num } = breakpoint {
                        let mut sender = remedybg.begin_send_command(platform, RemedybgCommandKind::DeleteBreakpoint)?;
                        let writer = sender.write();
                        id.serialize(writer);
                        sender.send(platform);
                    }
                }

                // send editor breakpoints
            }
        }
        _ => (),
    }

    Ok(())
}

fn on_ipc_output(plugin_handle: PluginHandle, ctx: &mut EditorContext, id: u32, mut bytes: &[u8]) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);

    match id {
        CONTROL_PIPE_ID => {
            match remedybg.pending_commands.pop() {
                Some(command_kind) => {
                    if let Err(error) = on_control_response(
                        remedybg,
                        &mut ctx.editor.string_pool,
                        &mut ctx.platform,
                        command_kind,
                        bytes,
                    ) {
                        ctx.editor.logger.write(LogKind::Error).fmt(format_args!(
                            "remedybg: error while deserializing command {}: {}", command_kind as usize, error,
                        ));
                    }
                },
                None => ctx.editor.logger.write(LogKind::Error).fmt(format_args!(
                    "remedybg: received response with no pending command"
                )),
            }
        }
        EVENT_PIPE_ID => {
            let event = match RemedybgEvent::deserialize(&mut bytes) {
                Ok(event) => event,
                Err(_) => {
                    ctx.editor
                        .logger
                        .write(LogKind::Error)
                        .fmt(format_args!("remedybg: could not deserialize debug event"));
                    return;
                }
            };

            match event {
                RemedybgEvent::BreakpointHit { breakpoint_id } => {
                    //
                }
                RemedybgEvent::BreakpointResolved { breakpoint_id } => {
                    //
                }
                RemedybgEvent::BreakpointAdded { breakpoint_id } => {
                    //
                }
                RemedybgEvent::BreakpointRemoved { breakpoint_id } => {
                    //
                }
                _ => (),
            }

            let mut write = ctx.editor.logger.write(LogKind::Diagnostic);
            write.fmt(format_args!(
                "remedybg: event {:?} :",
                std::mem::discriminant(&event)
            ));
            if let Ok(text) = std::str::from_utf8(bytes) {
                write.str("\n");
                write.str(text);
            }
        }
        _ => unreachable!(),
    }
}

fn on_ipc_close(_: PluginHandle, ctx: &mut EditorContext, id: u32) {
    let ipc_name = get_ipc_name(id);
    ctx.editor
        .logger
        .write(LogKind::Diagnostic)
        .fmt(format_args!("remedybg: {} ipc closed", ipc_name));
}
