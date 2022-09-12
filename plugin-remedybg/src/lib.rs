use std::{
    collections::HashMap,
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

const IPC_BUF_SIZE: usize = 8 * 1024;
const CONTROL_PIPE_ID: u32 = 0;
const EVENT_PIPE_ID: u32 = 1;

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

struct BreakpointLocation {
    pub buffer_handle: BufferHandle,
    pub line_index: u32,
}

#[derive(Default)]
pub(crate) struct RemedybgPlugin {
    process_state: ProcessState,
    session_name: String,

    pending_command_contexts: Vec<PendingCommandContext>,
    control_ipc_handle: Option<PlatformIpcHandle>,

    breakpoints: HashMap<RemedybgId, BreakpointLocation>,
    new_breakpoint_locations: Vec<BreakpointLocation>,
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
        action: PendingCommandAction,
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
        let sender = self.begin_send_command(
            platform,
            RemedybgCommandKind::GetBreakpoints,
            PendingCommandAction::SyncBreakpoints,
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
        self.buf.write_no_clear()
    }

    pub fn send(self, platform: &mut Platform) {
        let len = self.buf.as_bytes().len();
        let bytes = self.buf.as_bytes().as_ptr();

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
    action: PendingCommandAction,
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

    r(
        "remedybg-sync-breakpoints",
        &[CompletionSource::Files],
        |ctx, io| {
            io.args.assert_empty()?;

            let plugin_handle = io.plugin_handle();
            let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
            remedybg.begin_sync_breakpoints(&mut ctx.platform)
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

            let mut sender = begin_send_command(
                ctx,
                io.plugin_handle(),
                RemedybgCommandKind::StartDebugging,
                PendingCommandAction::None,
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
            PendingCommandAction::None,
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
                PendingCommandAction::None,
            )?,
            Err(_) => begin_send_command(
                ctx,
                io.plugin_handle(),
                RemedybgCommandKind::AttachToProcessByName,
                PendingCommandAction::None,
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
            PendingCommandAction::None,
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
            PendingCommandAction::None,
        )?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-step-out", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender = begin_send_command(
            ctx,
            io.plugin_handle(),
            RemedybgCommandKind::StepOut,
            PendingCommandAction::None,
        )?;
        sender.send(&mut ctx.platform);
        Ok(())
    });

    r("remedybg-continue-execution", &[], |ctx, io| {
        io.args.assert_empty()?;
        let sender = begin_send_command(
            ctx,
            io.plugin_handle(),
            RemedybgCommandKind::ContinueExecution,
            PendingCommandAction::None,
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
            PendingCommandAction::None,
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
            PendingCommandAction::None,
        )?;
        sender.send(&mut ctx.platform);
        Ok(())
    });
}

fn on_editor_events(plugin_handle: PluginHandle, ctx: &mut EditorContext) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);

    let mut handled_breakpoints_changed = false;
    let mut events = EditorEventIter::new();
    while let Some(event) = events.next(ctx.editor.events.reader()) {
        match event {
            &EditorEvent::BufferClose { handle } => remedybg
                .breakpoints
                .retain(|_, b| b.buffer_handle != handle),
            EditorEvent::BufferBreakpointsChanged { .. } => {
                if !handled_breakpoints_changed {
                    handled_breakpoints_changed = true;

                    let sender = remedybg.begin_send_command(
                        &mut ctx.platform,
                        RemedybgCommandKind::GetBreakpoints,
                        PendingCommandAction::SendEditorBreakpoints,
                    );
                    if let Ok(sender) = sender {
                        sender.send(&mut ctx.platform);
                    }
                }
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
    let b = bytes.as_ptr();
    let l = bytes.len();

    match RemedybgCommandResult::deserialize(&mut bytes) {
        Ok(RemedybgCommandResult::Ok) => (),
        Ok(result) => return Err(ProtocolError::RemedybgCommandResult(result)),
        Err(error) => return Err(error.into()),
    }

    editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
        "remedybg: on control response: [kind: {:?} action: {:?}] bytes left: {}",
        std::mem::discriminant(&command_context.command_kind),
        std::mem::discriminant(&command_context.action),
        bytes.len(),
    ));

    match command_context.command_kind {
        RemedybgCommandKind::GetBreakpoints => match command_context.action {
            PendingCommandAction::SyncBreakpoints => {
                remedybg.breakpoints.clear();
                remedybg.new_breakpoint_locations.clear();

                let breakpoint_count = u16::deserialize(&mut bytes)?;
                for _ in 0..breakpoint_count {
                    let id = RemedybgId::deserialize(&mut bytes)?;
                    let _enabled = RemedybgBool::deserialize(&mut bytes)?;
                    let _module_name = RemedybgStr::deserialize(&mut bytes)?;
                    let _condition_expr = RemedybgStr::deserialize(&mut bytes)?;
                    let breakpoint = RemedybgProtocolBreakpoint::deserialize(&mut bytes)?;
                    if let RemedybgProtocolBreakpoint::FilenameLine { filename, line_num } =
                        breakpoint
                    {
                        let filename = filename.0;
                        let line_index = line_num.saturating_sub(1);

                        let result = editor
                            .buffer_handle_from_path(Path::new(filename), BufferProperties::text());
                        let events = editor.events.writer();
                        match result.read_error {
                            Some(_) => editor.buffers.defer_remove(result.buffer_handle, events),
                            None => {
                                remedybg.breakpoints.insert(
                                    id,
                                    BreakpointLocation {
                                        buffer_handle: result.buffer_handle,
                                        line_index,
                                    },
                                );
                                remedybg.new_breakpoint_locations.push(BreakpointLocation {
                                    buffer_handle: result.buffer_handle,
                                    line_index,
                                });
                            }
                        }
                    }
                }

                remedybg
                    .new_breakpoint_locations
                    .sort_unstable_by_key(|b| b.buffer_handle.0);

                let current_directory = &editor.current_directory;
                let events = editor.events.writer();

                let mut error = None;
                let mut file_path = editor.string_pool.acquire();
                let mut new_breakpoint_locations =
                    std::mem::take(&mut remedybg.new_breakpoint_locations);

                let mut new_breakpoint_locations_slice = &mut new_breakpoint_locations[..];
                while let Some(first) = new_breakpoint_locations_slice.first() {
                    let buffer_handle = first.buffer_handle;

                    let end_index = match new_breakpoint_locations_slice
                        .iter()
                        .position(|b| b.buffer_handle != buffer_handle)
                    {
                        Some(i) => i,
                        None => new_breakpoint_locations_slice.len(),
                    };
                    let (new_breakpoints, rest) =
                        new_breakpoint_locations_slice.split_at_mut(end_index);
                    new_breakpoint_locations_slice = rest;

                    let buffer = editor.buffers.get_mut(buffer_handle);
                    file_path.clear();
                    if get_absolue_file_path(current_directory, &buffer.path, &mut file_path)
                        .is_ok()
                    {
                        new_breakpoints.sort_unstable_by_key(|b| b.line_index);

                        let mut new_bp_index = 0;
                        'buffer_breakpoint_loop: for breakpoint in buffer.breakpoints() {
                            while let Some(new_bp) = new_breakpoints.get(new_bp_index) {
                                if new_bp.line_index < breakpoint.line_index {
                                    new_bp_index += 1;
                                } else if new_bp.line_index == breakpoint.line_index {
                                    continue 'buffer_breakpoint_loop;
                                } else {
                                    break;
                                }
                            }

                            let file_path = RemedybgStr(&file_path);
                            let line_num = (breakpoint.line_index + 1) as u32;
                            let condition_expr = RemedybgStr("");
                            match remedybg.begin_send_command(
                                platform,
                                RemedybgCommandKind::AddBreakpointAtFilenameLine,
                                PendingCommandAction::None,
                            ) {
                                Ok(mut sender) => {
                                    let write = sender.write();
                                    file_path.serialize(write);
                                    line_num.serialize(write);
                                    condition_expr.serialize(write);
                                    sender.send(platform);
                                }
                                Err(e) => error = Some(e),
                            }
                        }

                        let mut breakpoints = buffer.breakpoints_mut();
                        for breakpoint in new_breakpoints {
                            breakpoints.add(breakpoint.line_index, events);
                        }
                    }
                }

                editor.string_pool.release(file_path);
                remedybg.new_breakpoint_locations = new_breakpoint_locations;

                if let Some(error) = error {
                    return Err(error.into());
                }
            }
            PendingCommandAction::SendEditorBreakpoints => {
                remedybg.breakpoints.clear();

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
                            PendingCommandAction::None,
                        )?;
                        let write = sender.write();
                        id.serialize(write);
                        sender.send(platform);
                    }
                }

                let current_directory = &editor.current_directory;
                let mut error = None;
                let mut file_path = editor.string_pool.acquire();
                for buffer in editor.buffers.iter() {
                    file_path.clear();
                    if get_absolue_file_path(current_directory, &buffer.path, &mut file_path)
                        .is_ok()
                    {
                        for breakpoint in buffer.breakpoints() {
                            let file_path = RemedybgStr(&file_path);
                            let line_num = (breakpoint.line_index + 1) as u32;
                            let condition_expr = RemedybgStr("");
                            match remedybg.begin_send_command(
                                platform,
                                RemedybgCommandKind::AddBreakpointAtFilenameLine,
                                PendingCommandAction::None,
                            ) {
                                Ok(mut sender) => {
                                    let write = sender.write();
                                    file_path.serialize(write);
                                    line_num.serialize(write);
                                    condition_expr.serialize(write);
                                    sender.send(platform);
                                }
                                Err(e) => error = Some(e),
                            }
                        }
                    }
                }
                editor.string_pool.release(file_path);
                if let Some(error) = error {
                    return Err(error.into());
                }
            }
            _ => (),
        },
        RemedybgCommandKind::GetBreakpointLocations => {
            // NOTE: insufficient data here
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
                "remedybg: get breakpoint locations [kind: {:?} action: {:?}] (loc count: {}): {}:{}",
                std::mem::discriminant(&command_context.command_kind),
                std::mem::discriminant(&command_context.action),
                location_count,
                filename,
                line_index,
            ));

            match command_context.action {
                PendingCommandAction::GoToLocation(breakpoint_id) => {
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
                                });

                                remedybg.breakpoints.insert(
                                    breakpoint_id,
                                    BreakpointLocation {
                                        buffer_handle: buffer_view.buffer_handle,
                                        line_index,
                                    },
                                );
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
                PendingCommandAction::UpdateBreakpoint(breakpoint_id) => {
                    if let Some(breakpoint) = remedybg.breakpoints.get(&breakpoint_id) {
                        let buffer = editor.buffers.get_mut(breakpoint.buffer_handle);
                        let breakpoint_index = buffer
                            .breakpoints()
                            .binary_search_by_key(&breakpoint.line_index, |b| b.line_index);
                        if let Ok(breakpoint_index) = breakpoint_index {
                            let mut breakpoints = buffer.breakpoints_mut();
                            breakpoints.remove_at(breakpoint_index, editor.events.writer());
                        }
                    }

                    let buffer_handle = editor
                        .buffers
                        .find_with_path(&editor.current_directory, Path::new(filename));
                    if let Some(buffer_handle) = buffer_handle {
                        remedybg.breakpoints.insert(
                            breakpoint_id,
                            BreakpointLocation {
                                buffer_handle,
                                line_index,
                            },
                        );

                        let buffer = editor.buffers.get_mut(buffer_handle);
                        let has_breakpoint = buffer
                            .breakpoints()
                            .binary_search_by_key(&line_index, |b| b.line_index)
                            .is_ok();
                        if !has_breakpoint {
                            let mut breakpoints = buffer.breakpoints_mut();
                            breakpoints.add(line_index, editor.events.writer());
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
                RemedybgCommandKind::GetBreakpointLocations,
                PendingCommandAction::GoToLocation(breakpoint_id),
            )?;
            let write = sender.write();
            breakpoint_id.serialize(write);
            sender.send(platform);
        }
        &RemedybgEvent::BreakpointResolved { breakpoint_id } => {
            let mut sender = remedybg.begin_send_command(
                platform,
                RemedybgCommandKind::GetBreakpointLocations,
                PendingCommandAction::UpdateBreakpoint(breakpoint_id),
            )?;
            let write = sender.write();
            breakpoint_id.serialize(write);
            sender.send(platform);
        }
        &RemedybgEvent::BreakpointAdded { breakpoint_id } => {
            if !remedybg.breakpoints.contains_key(&breakpoint_id) {
                let mut sender = remedybg.begin_send_command(
                    platform,
                    RemedybgCommandKind::GetBreakpointLocations,
                    PendingCommandAction::UpdateBreakpoint(breakpoint_id),
                )?;
                let write = sender.write();
                breakpoint_id.serialize(write);
                sender.send(platform);
            }
        }
        &RemedybgEvent::BreakpointRemoved { breakpoint_id } => {
            if let Some(breakpoint) = remedybg.breakpoints.remove(&breakpoint_id) {
                let buffer = editor.buffers.get_mut(breakpoint.buffer_handle);
                let breakpoint_index = buffer
                    .breakpoints()
                    .binary_search_by_key(&breakpoint.line_index, |b| b.line_index);
                if let Ok(breakpoint_index) = breakpoint_index {
                    let mut breakpoints = buffer.breakpoints_mut();
                    breakpoints.remove_at(breakpoint_index, editor.events.writer());
                }
            }
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
                        "remedybg: error while deserializing command result {}: {}",
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
                if let Err(error) =
                    on_event(remedybg, &mut ctx.editor, &mut ctx.platform, &event, bytes)
                {
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

fn on_ipc_close(plugin_handle: PluginHandle, ctx: &mut EditorContext, id: u32) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    if id == CONTROL_PIPE_ID {
        remedybg.control_ipc_handle = None;
    }

    let ipc_name = get_ipc_name(id);
    ctx.editor
        .logger
        .write(LogKind::Diagnostic)
        .fmt(format_args!("remedybg: {} ipc closed", ipc_name));
}
