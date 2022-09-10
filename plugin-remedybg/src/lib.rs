use std::{
    path::Path,
    process::{Command, Stdio},
};

use pepper::{
    buffer::{BufferHandle, BufferProperties},
    buffer_position::{BufferPosition, BufferPositionIndex},
    client::ClientManager,
    command::{CommandError, CommandManager, CompletionSource},
    cursor::Cursor,
    editor::{Editor, EditorContext},
    editor_utils::{to_absolute_path_string, LogKind},
    events::{EditorEvent, EditorEventIter},
    platform::{
        IpcReadMode, IpcTag, Platform, PlatformIpcHandle, PlatformProcessHandle, PlatformRequest,
        PooledBuf, ProcessTag,
    },
    plugin::{Plugin, PluginDefinition, PluginHandle},
    serialization::Serialize,
    ResourceFile,
};

mod protocol;

use protocol::{
    PendingCommandAction, PendingCommandContext, ProtocolError, RemedybgBool, RemedybgCommandKind,
    RemedybgCommandResult, RemedybgEvent, RemedybgId, RemedybgProtocolBreakpoint, RemedybgStr,
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

struct BreakpointPosition {
    pub buffer_handle: BufferHandle,
    pub line_index: u32,
}

const IPC_BUF_SIZE: usize = 8 * 1024;
const CONTROL_PIPE_ID: u32 = 0;
const EVENT_PIPE_ID: u32 = 1;

#[derive(Default)]
pub(crate) struct RemedybgPlugin {
    process_state: ProcessState,
    session_name: String,

    pending_command_contexts: Vec<PendingCommandContext>,
    control_ipc_handle: Option<PlatformIpcHandle>,
    new_breakpoint_positions: Vec<BreakpointPosition>,
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
        action: Option<PendingCommandAction>,
    ) -> Result<CommandSender, CommandError> {
        match self.control_ipc_handle {
            Some(ipc_handle) => {
                self.pending_command_contexts.push(PendingCommandContext {
                    command_kind,
                    action,
                });

                let mut buf = platform.buf_pool.acquire();
                let write = buf.write();
                command_kind.serialize(write);
                let sender = CommandSender { ipc_handle, buf };

                Ok(sender)
            }
            None => Err(CommandError::OtherStatic("remedybg is not running")),
        }
    }

    pub fn begin_sync_breakpoints(&mut self, platform: &mut Platform) -> Result<(), CommandError> {
        let sender =
            self.begin_send_command(
                platform,
                RemedybgCommandKind::GetBreakpoints,
                Some(PendingCommandAction::SyncBreakpoints)
            )?;
        sender.send(platform);
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
    action: Option<PendingCommandAction>,
) -> Result<CommandSender, CommandError> {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    remedybg.begin_send_command(&mut ctx.platform, command_kind, action)
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

            let mut sender = begin_send_command(
                ctx,
                io.plugin_handle(),
                RemedybgCommandKind::StartDebugging,
                None,
            )?;
            let write = sender.write();
            RemedybgBool(start_paused).serialize(write);
            sender.send(&mut ctx.platform);
            Ok(())
        },
    );

    r("remedybg-stop-debugging", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender = begin_send_command(
            ctx,
            io.plugin_handle(),
            RemedybgCommandKind::StopDebugging,
            None,
        )?;
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
                None,
            )?,
            Err(_) => begin_send_command(
                ctx,
                io.plugin_handle(),
                RemedybgCommandKind::AttachToProcessByName,
                None,
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
        let sender = begin_send_command(
            ctx,
            io.plugin_handle(),
            RemedybgCommandKind::StepIntoByLine,
            None,
        )?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-step-over", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender = begin_send_command(
            ctx,
            io.plugin_handle(),
            RemedybgCommandKind::StepOverByLine,
            None,
        )?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-step-out", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender =
            begin_send_command(ctx, io.plugin_handle(), RemedybgCommandKind::StepOut, None)?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-continue-execution", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender = begin_send_command(
            ctx,
            io.plugin_handle(),
            RemedybgCommandKind::ContinueExecution,
            None,
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
            None,
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
        let sender = begin_send_command(
            ctx,
            io.plugin_handle(),
            RemedybgCommandKind::BreakExecution,
            None,
        )?;
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
                let sender = remedybg.begin_send_command(
                    &mut ctx.platform,
                    RemedybgCommandKind::GetBreakpoints,
                    Some(PendingCommandAction::SendEditorBreakpoints),
                );
                if let Ok(sender) = sender {
                    sender.send(&mut ctx.platform);
                }
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
            buf_len: IPC_BUF_SIZE,
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
            buf_len: IPC_BUF_SIZE,
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
        let _ = remedybg.begin_sync_breakpoints(&mut ctx.platform);
    }

    let ipc_name = get_ipc_name(id);
    ctx.editor
        .logger
        .write(LogKind::Diagnostic)
        .fmt(format_args!("remedybg: connected to {} ipc", ipc_name));
}

fn on_control_response(
    remedybg: &mut RemedybgPlugin,
    editor: &mut Editor,
    platform: &mut Platform,
    clients: &mut ClientManager,
    command_context: PendingCommandContext,
    mut bytes: &[u8],
) -> Result<(), ProtocolError> {
    match RemedybgCommandResult::deserialize(&mut bytes) {
        Ok(RemedybgCommandResult::Ok) => (),
        Ok(result) => return Err(ProtocolError::RemedybgCommandResult(result)),
        Err(error) => return Err(error.into()),
    }

    editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
        "remedybg: on control response: {:?} bytes left: {}",
        std::mem::discriminant(&command_context.command_kind),
        bytes.len(),
    ));

    match command_context.command_kind {
        RemedybgCommandKind::GetBreakpoints => match command_context.action {
            Some(PendingCommandAction::SyncBreakpoints) => {
                remedybg.new_breakpoint_positions.clear();

                let breakpoint_count = u16::deserialize(&mut bytes)?;
                for _ in 0..breakpoint_count {
                    let _id = RemedybgId::deserialize(&mut bytes)?;
                    let _enabled = RemedybgBool::deserialize(&mut bytes)?;
                    let _module_name = RemedybgStr::deserialize(&mut bytes)?;
                    let _condition_expr = RemedybgStr::deserialize(&mut bytes)?;
                    let breakpoint = RemedybgProtocolBreakpoint::deserialize(&mut bytes)?;
                    if let RemedybgProtocolBreakpoint::FilenameLine { filename, line_num } = breakpoint {
                        let filename = filename.0;
                        let line_index = line_num.saturating_sub(1);

                        let result = editor
                            .buffer_handle_from_path(Path::new(filename), BufferProperties::text());
                        let events = editor.events.writer();
                        match result.read_error {
                            Some(_) => editor.buffers.defer_remove(result.buffer_handle, events),
                            None => remedybg.new_breakpoint_positions.push(BreakpointPosition {
                                buffer_handle: result.buffer_handle,
                                line_index,
                            }),
                        }
                    }
                }

                remedybg.new_breakpoint_positions.sort_unstable_by_key(|b| b.buffer_handle.0);

                let events = editor.events.writer();
                let mut breakpoint_positions = &remedybg.new_breakpoint_positions[..];
                while let Some(first) = breakpoint_positions.first() {
                    let buffer_handle = first.buffer_handle;

                    let buffer = editor.buffers.get_mut(buffer_handle);
                    let mut breakpoints = buffer.breakpoints_mut();

                    while let Some((position, rest)) = breakpoint_positions.split_first() {
                        breakpoint_positions = rest;
                        if position.buffer_handle != buffer_handle {
                            break;
                        }

                        breakpoints.add(position.line_index, events);
                    }
                }
            }
            Some(PendingCommandAction::SendEditorBreakpoints) => {
                let breakpoint_count = u16::deserialize(&mut bytes)?;
                for _ in 0..breakpoint_count {
                    let id = RemedybgId::deserialize(&mut bytes)?;
                    let _enabled = RemedybgBool::deserialize(&mut bytes)?;
                    let _module_name = RemedybgStr::deserialize(&mut bytes)?;
                    let _condition_expr = RemedybgStr::deserialize(&mut bytes)?;
                    let breakpoint = RemedybgProtocolBreakpoint::deserialize(&mut bytes)?;
                    if let RemedybgProtocolBreakpoint::FilenameLine { .. } = breakpoint {
                        let mut sender = remedybg.begin_send_command(
                            platform,
                            RemedybgCommandKind::DeleteBreakpoint,
                            Some(PendingCommandAction::GoToLocation),
                        )?;
                        let write = sender.write();
                        id.serialize(write);
                        sender.send(platform);
                    }
                }

                let current_directory = &editor.current_directory;
                let mut file_path = editor.string_pool.acquire();
                for buffer in editor.buffers.iter() {
                    file_path.clear();
                    if get_absolue_file_path(current_directory, &buffer.path, &mut file_path).is_ok() {
                        for breakpoint in buffer.breakpoints() {
                            let line_index = breakpoint.line_index as u32;
                            let mut sender = remedybg.begin_send_command(
                                platform,
                                RemedybgCommandKind::AddBreakpointAtFilenameLine,
                                None,
                            )?;
                            let write = sender.write();
                            RemedybgStr(&file_path).serialize(write);
                            line_index.serialize(write);
                            RemedybgStr("").serialize(write);
                            sender.send(platform);
                        }
                    }
                }
                editor.string_pool.release(file_path);
            }
            _ => (),
        }
        RemedybgCommandKind::GetBreakpointLocation => {
            let mut filename = "";
            let mut line_index = 0;
            let location_count = u16::deserialize(&mut bytes)?;
            for _ in 0..location_count {
                let _address = u64::deserialize(&mut bytes)?;
                let _module_name = RemedybgStr::deserialize(&mut bytes)?;
                filename = RemedybgStr::deserialize(&mut bytes)?.0;
                line_index = u32::deserialize(&mut bytes)?.saturating_sub(1) as BufferPositionIndex;
                break;
            }

            editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
                "remedybg: get breakpoint location: {:?} = {}:{}",
                std::mem::discriminant(&command_context.command_kind),
                filename,
                line_index,
            ));

            match command_context.action {
                Some(PendingCommandAction::GoToLocation) => {
                    if let Some(client_handle) = clients.focused_client() {
                        let buffer_view_handle = editor.buffer_view_handle_from_path(
                            client_handle,
                            Path::new(filename),
                            BufferProperties::text(),
                            false,
                        );
                        if let Ok(buffer_view_handle) = buffer_view_handle {
                            {
                                let position = BufferPosition::line_col(line_index, 0);
                                let buffer_view = editor.buffer_views.get_mut(buffer_view_handle);
                                let mut cursors = buffer_view.cursors.mut_guard();
                                cursors.clear();
                                cursors.add(Cursor {
                                    anchor: position,
                                    position,
                                })
                            }

                            {
                                let client = clients.get_mut(client_handle);
                                client.set_buffer_view_handle(
                                    Some(buffer_view_handle),
                                    &editor.buffer_views,
                                );
                            }
                        }
                    }
                }
                Some(PendingCommandAction::AddBreakpoint) => {
                    let buffer_handle = editor
                        .buffers
                        .find_with_path(&editor.current_directory, Path::new(filename));
                    if let Some(buffer_handle) = buffer_handle {
                        let buffer = editor.buffers.get_mut(buffer_handle);
                        let mut breakpoints = buffer.breakpoints_mut();
                        breakpoints.add(line_index, editor.events.writer());
                    }
                }
                Some(PendingCommandAction::RemoveBreakpoint) => {
                    let buffer_handle = editor
                        .buffers
                        .find_with_path(&editor.current_directory, Path::new(filename));
                    if let Some(buffer_handle) = buffer_handle {
                        let buffer = editor.buffers.get_mut(buffer_handle);
                        let breakpoint_index = buffer
                            .breakpoints()
                            .iter()
                            .position(|b| b.line_index == line_index);
                        if let Some(breakpoint_index) = breakpoint_index {
                            let mut breakpoints = buffer.breakpoints_mut();
                            breakpoints.remove_at(breakpoint_index, editor.events.writer());
                        }
                    }
                }
                _ => (),
            }
        }
        _ => (),
    }

    Ok(())
}

fn on_event(
    remedybg: &mut RemedybgPlugin,
    editor: &mut Editor,
    platform: &mut Platform,
    event: &RemedybgEvent,
    bytes: &[u8],
) -> Result<(), ProtocolError> {
    editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
        "remedybg: on event: {:?} bytes left: {}",
        std::mem::discriminant(event),
        bytes.len(),
    ));

    match event {
        &RemedybgEvent::BreakpointHit { breakpoint_id } => {
            let mut sender = remedybg.begin_send_command(
                platform,
                RemedybgCommandKind::GetBreakpointLocation,
                Some(PendingCommandAction::GoToLocation),
            )?;
            let write = sender.write();
            breakpoint_id.serialize(write);
            sender.send(platform);
        }
        &RemedybgEvent::BreakpointAdded { breakpoint_id } => {
            let mut sender = remedybg.begin_send_command(
                platform,
                RemedybgCommandKind::GetBreakpointLocation,
                Some(PendingCommandAction::AddBreakpoint),
            )?;
            let write = sender.write();
            breakpoint_id.serialize(write);
            sender.send(platform);
        }
        &RemedybgEvent::BreakpointRemoved { breakpoint_id } => {
            let mut sender = remedybg.begin_send_command(
                platform,
                RemedybgCommandKind::GetBreakpointLocation,
                Some(PendingCommandAction::RemoveBreakpoint),
            )?;
            let write = sender.write();
            breakpoint_id.serialize(write);
            sender.send(platform);
        }
        _ => (),
    }

    Ok(())
}

fn on_ipc_output(plugin_handle: PluginHandle, ctx: &mut EditorContext, id: u32, mut bytes: &[u8]) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);

    match id {
        CONTROL_PIPE_ID => match remedybg.pending_command_contexts.pop() {
            Some(command_context) => {
                let command_kind = command_context.command_kind;
                if let Err(error) = on_control_response(
                    remedybg,
                    &mut ctx.editor,
                    &mut ctx.platform,
                    &mut ctx.clients,
                    command_context,
                    bytes,
                ) {
                    ctx.editor.logger.write(LogKind::Error).fmt(format_args!(
                        "remedybg: error while deserializing command {}: {}",
                        command_kind as usize, error,
                    ));
                }
            }
            None => ctx.editor.logger.write(LogKind::Error).fmt(format_args!(
                "remedybg: received response with no pending command"
            )),
        },
        EVENT_PIPE_ID => match RemedybgEvent::deserialize(&mut bytes) {
            Ok(event) => {
                if let Err(error) = on_event(
                    remedybg,
                    &mut ctx.editor,
                    &mut ctx.platform,
                    &event,
                    bytes,
                ) {
                    ctx.editor.logger.write(LogKind::Error).fmt(format_args!(
                        "remedybg: error while deserializing event {}: {}",
                        event, error,
                    ));
                }
            }
            Err(_) => {
                ctx.editor
                    .logger
                    .write(LogKind::Error)
                    .fmt(format_args!("remedybg: could not deserialize debug event"));
                return;
            }
        },
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

