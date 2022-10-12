use std::{
    collections::{HashMap, VecDeque},
    hash::{Hash, Hasher},
    path::Path,
    process::{Command, Stdio},
};

use pepper::{
    buffer::{BufferBreakpointId, BufferHandle, BufferProperties},
    buffer_position::BufferPosition,
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
    serialization::{DeserializeError, Serialize},
    ResourceFile,
};

mod protocol;

use protocol::{
    deserialize_remedybg_bytes, ProtocolError, RemedybgBool, RemedybgCommandKind,
    RemedybgCommandResult, RemedybgEvent, RemedybgId, RemedybgProtocolBreakpoint,
    RemedybgSourceLocationChangedReason, RemedybgStr,
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

const SESSION_PREFIX: &str = "remedybg-";
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

#[derive(PartialEq, Eq)]
struct EditorToRemedybgBreakpointMapKey {
    pub buffer_handle: BufferHandle,
    pub breakpoint_id: BufferBreakpointId,
}
impl Hash for EditorToRemedybgBreakpointMapKey {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        state.write_u32(self.buffer_handle.0);
        state.write_u32(self.breakpoint_id.0);
    }
}

struct EditorToRemedybgBreakpointMapValue {
    pub breakpoint_id: RemedybgId,
    pub marked_for_deletion: bool,
}

struct BreakpointLocation {
    pub buffer_handle: BufferHandle,
    pub line_index: u32,
}

struct NewBreakpoint {
    pub remedybg_id: RemedybgId,
    pub buffer_handle: BufferHandle,
    pub line_index: u32,
}

#[derive(Default)]
pub(crate) struct RemedybgPlugin {
    process_state: ProcessState,
    session_name: String,

    pending_commands: VecDeque<RemedybgCommandKind>,
    control_ipc_handle: Option<PlatformIpcHandle>,

    editor_to_remedybg_breakpoint_map:
        HashMap<EditorToRemedybgBreakpointMapKey, EditorToRemedybgBreakpointMapValue>,
    breakpoints: HashMap<RemedybgId, BreakpointLocation>,
    new_breakpoints: Vec<NewBreakpoint>,
    new_serialized_breakpoints: Vec<SerializedBreakpoint>,
}
impl RemedybgPlugin {
    pub fn spawn(
        &mut self,
        platform: &mut Platform,
        plugin_handle: PluginHandle,
        editor_session_name: &str,
        session_file: Option<&str>,
    ) {
        if !matches!(self.process_state, ProcessState::NotRunning) {
            return;
        }

        self.process_state = ProcessState::Spawning;
        self.session_name.clear();
        self.session_name.push_str(SESSION_PREFIX);
        self.session_name.push_str(editor_session_name);

        let mut command = Command::new("remedybg");
        command.arg("--servername");
        command.arg(&self.session_name);
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

    fn control_ipc_handle(&self) -> Result<PlatformIpcHandle, CommandError> {
        match self.control_ipc_handle {
            Some(handle) => Ok(handle),
            None => Err(CommandError::OtherStatic("remedybg is not running")),
        }
    }

    fn begin_send_command(
        &mut self,
        platform: &mut Platform,
        command_kind: RemedybgCommandKind,
    ) -> Result<CommandSender, CommandError> {
        let ipc_handle = self.control_ipc_handle()?;
        let sender = begin_send_command_raw(
            platform,
            ipc_handle,
            command_kind,
            &mut self.pending_commands,
        );
        Ok(sender)
    }

    pub fn begin_sync_breakpoints(&mut self, platform: &mut Platform) -> Result<(), CommandError> {
        let sender = self.begin_send_command(platform, RemedybgCommandKind::GetBreakpoints)?;
        sender.send(platform);
        Ok(())
    }

    fn send_buffer_breakpoint_changes(
        &mut self,
        editor: &mut Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
    ) {
        let ipc_handle = match self.control_ipc_handle() {
            Ok(handle) => handle,
            Err(_) => return,
        };

        for (key, value) in self.editor_to_remedybg_breakpoint_map.iter_mut() {
            if key.buffer_handle == buffer_handle {
                value.marked_for_deletion = true;
            }
        }

        let buffer = editor.buffers.get(buffer_handle);
        let current_directory = &editor.current_directory;
        let mut file_path = editor.string_pool.acquire();

        for breakpoint in buffer.breakpoints() {
            let key = EditorToRemedybgBreakpointMapKey {
                buffer_handle,
                breakpoint_id: breakpoint.id,
            };

            let breakpoint_map_value = self.editor_to_remedybg_breakpoint_map.get_mut(&key);
            let remedybg_breakpoint_location = breakpoint_map_value
                .as_ref()
                .and_then(|v| self.breakpoints.get(&v.breakpoint_id));
            match (breakpoint_map_value, remedybg_breakpoint_location) {
                (Some(breakpoint_map_value), Some(remedybg_breakpoint_location)) => {
                    breakpoint_map_value.marked_for_deletion = false;
                    if remedybg_breakpoint_location.line_index != breakpoint.line_index {
                        let line_num = (breakpoint.line_index + 1) as u32;

                        let mut sender = begin_send_command_raw(
                            platform,
                            ipc_handle,
                            RemedybgCommandKind::UpdateBreakpointLine,
                            &mut self.pending_commands,
                        );
                        let write = sender.write();
                        breakpoint_map_value.breakpoint_id.serialize(write);
                        line_num.serialize(write);
                        sender.send(platform);
                    }
                }
                _ => {
                    file_path.clear();
                    match get_absolue_file_path(current_directory, &buffer.path, &mut file_path) {
                        Ok(()) => {
                            let file_path = RemedybgStr(file_path.as_bytes());
                            let line_num = (breakpoint.line_index + 1) as u32;
                            let condition_expr = RemedybgStr(b"");

                            let mut sender = begin_send_command_raw(
                                platform,
                                ipc_handle,
                                RemedybgCommandKind::AddBreakpointAtFilenameLine,
                                &mut self.pending_commands,
                            );
                            let write = sender.write();
                            file_path.serialize(write);
                            line_num.serialize(write);
                            condition_expr.serialize(write);
                            sender.send(platform);
                        }
                        Err(error) => editor.logger.write(LogKind::Error).fmt(format_args!(
                            "remedybg: error while sending editor breakpoints: {}",
                            error
                        )),
                    }
                }
            }
        }
        editor.string_pool.release(file_path);

        for value in self.editor_to_remedybg_breakpoint_map.values() {
            if value.marked_for_deletion {
                self.breakpoints.remove(&value.breakpoint_id);

                let mut sender = begin_send_command_raw(
                    platform,
                    ipc_handle,
                    RemedybgCommandKind::DeleteBreakpoint,
                    &mut self.pending_commands,
                );
                let write = sender.write();
                value.breakpoint_id.serialize(write);
                sender.send(platform);
            }
        }
        self.editor_to_remedybg_breakpoint_map
            .retain(|_, v| !v.marked_for_deletion);
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
fn begin_send_command_raw(
    platform: &mut Platform,
    ipc_handle: PlatformIpcHandle,
    command_kind: RemedybgCommandKind,
    pending_commands: &mut VecDeque<RemedybgCommandKind>,
) -> CommandSender {
    pending_commands.push_back(command_kind);

    let mut buf = platform.buf_pool.acquire();
    let write = buf.write();
    command_kind.serialize(write);
    CommandSender { ipc_handle, buf }
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
            begin_send_command(ctx, io.plugin_handle(), RemedybgCommandKind::StopDebugging)?;
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
        RemedybgStr(file_path.as_bytes()).serialize(write);
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

    let mut handled_breakpoints_changed = false;
    let mut events = EditorEventIter::new();
    while let Some(event) = events.next(ctx.editor.events.reader()) {
        match event {
            &EditorEvent::BufferClose { handle } => remedybg
                .breakpoints
                .retain(|_, b| b.buffer_handle != handle),
            &EditorEvent::BufferBreakpointsChanged { handle } => {
                if !handled_breakpoints_changed {
                    handled_breakpoints_changed = true;
                    remedybg.send_buffer_breakpoint_changes(
                        &mut ctx.editor,
                        &mut ctx.platform,
                        handle,
                    );
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
    remedybg.breakpoints.clear();
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

struct SerializedBreakpoint {
    id: RemedybgId,
    filename_range: (u32, u32),
    line_index: u32,
}
impl SerializedBreakpoint {
    pub fn filename<'de>(&self, bytes: &'de [u8]) -> Result<&'de str, &'de [u8]> {
        let (from, to) = self.filename_range;
        let bytes = &bytes[from as usize..to as usize];
        match std::str::from_utf8(bytes) {
            Ok(str) => Ok(str),
            Err(_) => Err(bytes),
        }
    }
}

fn get_all_breakpoints<'bytes>(
    mut bytes: &'bytes [u8],
    breakpoints: &mut Vec<SerializedBreakpoint>,
) -> Result<&'bytes [u8], DeserializeError> {
    breakpoints.clear();
    let breakpoint_bytes = bytes;
    let bytes_ptr = breakpoint_bytes.as_ptr() as usize;
    let breakpoint_count = u16::deserialize(&mut bytes)?;
    for _ in 0..breakpoint_count {
        let id = RemedybgId::deserialize(&mut bytes)?;
        let _enabled = RemedybgBool::deserialize(&mut bytes)?;
        let _module_name = deserialize_remedybg_bytes(&mut bytes)?;
        let _condition_expr = deserialize_remedybg_bytes(&mut bytes)?;
        let breakpoint = RemedybgProtocolBreakpoint::deserialize(&mut bytes)?;
        if let RemedybgProtocolBreakpoint::FilenameLine { filename, line_num } = breakpoint {
            let filename_from = filename.0.as_ptr() as usize - bytes_ptr;
            let filename_to = filename_from + filename.0.len();
            let line_index = line_num.saturating_sub(1);
            breakpoints.push(SerializedBreakpoint {
                id,
                filename_range: (filename_from as _, filename_to as _),
                line_index,
            });
        }
    }
    Ok(breakpoint_bytes)
}

fn log_filename_invaid_utf8(editor: &mut Editor, bytes: &[u8]) {
    editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
        "remedybg: serialized breakpoint has invalid utf-8 filename: {}",
        String::from_utf8_lossy(bytes),
    ));
}

fn on_control_response(
    remedybg: &mut RemedybgPlugin,
    editor: &mut Editor,
    platform: &mut Platform,
    command_kind: RemedybgCommandKind,
    mut bytes: &[u8],
) -> Result<(), ProtocolError> {
    let ipc_handle = remedybg.control_ipc_handle()?;

    match RemedybgCommandResult::deserialize(&mut bytes) {
        Ok(RemedybgCommandResult::Ok) => (),
        Ok(result) => return Err(ProtocolError::RemedybgCommandResult(result)),
        Err(error) => return Err(error.into()),
    }

    editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
        "remedybg: on control response: [{:?}] bytes left: {}",
        std::mem::discriminant(&command_kind),
        bytes.len(),
    ));

    match command_kind {
        RemedybgCommandKind::GetBreakpoints => {
            let current_directory = &editor.current_directory;

            let mut file_path = editor.string_pool.acquire();
            for buffer in editor.buffers.iter_mut() {
                let buffer_handle = buffer.handle();
                let len = buffer.breakpoints().len();
                if len == 0 {
                    continue;
                }

                file_path.clear();
                let has_file_path = match get_absolue_file_path(
                    current_directory,
                    &buffer.path,
                    &mut file_path,
                ) {
                    Ok(()) => true,
                    Err(error) => {
                        editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
                                "remedybg: error when trying to get buffer ({:?} {:?}) absolute file path: {}",
                                current_directory,
                                &buffer.path,
                                error,
                            ));
                        false
                    }
                };

                let events = editor.events.writer();
                let mut breakpoints = buffer.breakpoints_mut();
                for i in 0..len {
                    let breakpoint = &breakpoints.as_slice()[i];
                    let key = EditorToRemedybgBreakpointMapKey {
                        buffer_handle,
                        breakpoint_id: breakpoint.id,
                    };

                    if remedybg
                        .editor_to_remedybg_breakpoint_map
                        .contains_key(&key)
                        || !has_file_path
                    {
                        breakpoints.remove_at(i, events);
                    } else {
                        let mut sender = begin_send_command_raw(
                            platform,
                            ipc_handle,
                            RemedybgCommandKind::AddBreakpointAtFilenameLine,
                            &mut remedybg.pending_commands,
                        );
                        let file_path = RemedybgStr(file_path.as_bytes());
                        let line_num = (breakpoint.line_index + 1) as u32;
                        let condition_expr = RemedybgStr(b"");

                        let write = sender.write();
                        file_path.serialize(write);
                        line_num.serialize(write);
                        condition_expr.serialize(write);
                        sender.send(platform);
                    }
                }
            }
            editor.string_pool.release(file_path);

            remedybg.breakpoints.clear();
            remedybg.editor_to_remedybg_breakpoint_map.clear();
            remedybg.new_breakpoints.clear();

            let breakpoint_bytes =
                get_all_breakpoints(bytes, &mut remedybg.new_serialized_breakpoints)?;
            for breakpoint in &remedybg.new_serialized_breakpoints {
                let filename = match breakpoint.filename(breakpoint_bytes) {
                    Ok(filename) => filename,
                    Err(bytes) => {
                        log_filename_invaid_utf8(editor, bytes);
                        continue;
                    }
                };

                let result =
                    editor.buffer_handle_from_path(Path::new(filename), BufferProperties::text());
                match result.read_error {
                    Some(error) => {
                        let buffer = editor.buffers.get(result.buffer_handle);
                        editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
                            "remedybg: could not open buffer {:?}: {}",
                            &buffer.path, error,
                        ));
                        let events = editor.events.writer();
                        editor.buffers.defer_remove(result.buffer_handle, events);
                    }
                    None => {
                        remedybg.breakpoints.insert(
                            breakpoint.id,
                            BreakpointLocation {
                                buffer_handle: result.buffer_handle,
                                line_index: breakpoint.line_index,
                            },
                        );
                        remedybg.new_breakpoints.push(NewBreakpoint {
                            remedybg_id: breakpoint.id,
                            buffer_handle: result.buffer_handle,
                            line_index: breakpoint.line_index,
                        });
                    }
                }
            }

            remedybg
                .new_breakpoints
                .sort_unstable_by_key(|b| b.buffer_handle.0);

            let mut new_breakpoints = &remedybg.new_breakpoints[..];
            while let Some(first) = new_breakpoints.first() {
                let buffer_handle = first.buffer_handle;

                let end_index = match new_breakpoints
                    .iter()
                    .position(|b| b.buffer_handle != buffer_handle)
                {
                    Some(i) => i,
                    None => new_breakpoints.len(),
                };
                let (new_buffer_breakpoints, rest) = new_breakpoints.split_at(end_index);
                new_breakpoints = rest;

                let buffer = editor.buffers.get_mut(buffer_handle);
                let mut breakpoints = buffer.breakpoints_mut();
                let events = editor.events.writer();
                for new_buffer_breakpoint in new_buffer_breakpoints {
                    let breakpoint = breakpoints.add(new_buffer_breakpoint.line_index as _, events);
                    let key = EditorToRemedybgBreakpointMapKey {
                        buffer_handle,
                        breakpoint_id: breakpoint.id,
                    };
                    remedybg.editor_to_remedybg_breakpoint_map.insert(
                        key,
                        EditorToRemedybgBreakpointMapValue {
                            breakpoint_id: new_buffer_breakpoint.remedybg_id,
                            marked_for_deletion: false,
                        },
                    );
                }
            }
        }
        RemedybgCommandKind::GetBreakpoint => {
            let id = RemedybgId::deserialize(&mut bytes)?;
            let _enabled = RemedybgBool::deserialize(&mut bytes)?;
            let _module_name = deserialize_remedybg_bytes(&mut bytes)?;
            let _condition_expr = deserialize_remedybg_bytes(&mut bytes)?;
            let breakpoint = RemedybgProtocolBreakpoint::deserialize(&mut bytes)?;
            if let RemedybgProtocolBreakpoint::FilenameLine { filename, line_num } = breakpoint {
                let line_index = line_num.saturating_sub(1);

                if let Some(breakpoint) = remedybg.breakpoints.remove(&id) {
                    let buffer_handle = breakpoint.buffer_handle;
                    let buffer = editor.buffers.get_mut(buffer_handle);
                    let breakpoint_index = buffer
                        .breakpoints()
                        .binary_search_by_key(&breakpoint.line_index, |b| b.line_index);
                    if let Ok(breakpoint_index) = breakpoint_index {
                        let mut breakpoints = buffer.breakpoints_mut();
                        let breakpoint =
                            breakpoints.remove_at(breakpoint_index, editor.events.writer());
                        let key = EditorToRemedybgBreakpointMapKey {
                            buffer_handle,
                            breakpoint_id: breakpoint.id,
                        };
                        remedybg.editor_to_remedybg_breakpoint_map.remove(&key);
                    }
                }

                match std::str::from_utf8(filename.0) {
                    Ok(filename) => {
                        editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
                            "remedybg: update breakpoint: {} to {}:{}",
                            id.0, filename, line_index,
                        ));

                        let buffer_handle = editor
                            .buffers
                            .find_with_path(&editor.current_directory, Path::new(filename));
                        if let Some(buffer_handle) = buffer_handle {
                            let buffer = editor.buffers.get_mut(buffer_handle);
                            let breakpoint_index = buffer
                                .breakpoints()
                                .binary_search_by_key(&line_index, |b| b.line_index);
                            let breakpoint = match breakpoint_index {
                                Ok(i) => buffer.breakpoints()[i],
                                Err(_) => buffer
                                    .breakpoints_mut()
                                    .add(line_index, editor.events.writer()),
                            };

                            remedybg.breakpoints.insert(
                                id,
                                BreakpointLocation {
                                    buffer_handle,
                                    line_index,
                                },
                            );
                            let key = EditorToRemedybgBreakpointMapKey {
                                buffer_handle,
                                breakpoint_id: breakpoint.id,
                            };
                            remedybg.editor_to_remedybg_breakpoint_map.insert(
                                key,
                                EditorToRemedybgBreakpointMapValue {
                                    breakpoint_id: id,
                                    marked_for_deletion: false,
                                },
                            );
                        }
                    }
                    Err(_) => log_filename_invaid_utf8(editor, bytes),
                };
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
    clients: &mut ClientManager,
    event: &RemedybgEvent,
    bytes: &[u8],
) -> Result<(), ProtocolError> {
    editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
        "remedybg: on event: {:?} bytes left: {}",
        std::mem::discriminant(event),
        bytes.len(),
    ));

    match event {
        &RemedybgEvent::SourceLocationChanged {
            filename,
            line_num,
            reason,
        } => {
            let should_focus = matches!(
                reason,
                RemedybgSourceLocationChangedReason::BreakpointHit
                    | RemedybgSourceLocationChangedReason::ExceptionHit
                    | RemedybgSourceLocationChangedReason::StepOver
                    | RemedybgSourceLocationChangedReason::StepIn
                    | RemedybgSourceLocationChangedReason::StepOut
                    | RemedybgSourceLocationChangedReason::NonUserBreakpoint
                    | RemedybgSourceLocationChangedReason::DebugBreak
            );
            if should_focus {
                if let Some(client_handle) = clients.focused_client() {
                    let line_index = line_num.saturating_sub(1);
                    match std::str::from_utf8(filename.0) {
                        Ok(filename) => {
                            let buffer_view_handle = editor.buffer_view_handle_from_path(
                                client_handle,
                                Path::new(filename),
                                BufferProperties::text(),
                                false,
                            );
                            if let Ok(buffer_view_handle) = buffer_view_handle {
                                {
                                    let position = BufferPosition::line_col(line_index, 0);
                                    let buffer_view =
                                        editor.buffer_views.get_mut(buffer_view_handle);
                                    let mut cursors = buffer_view.cursors.mut_guard();
                                    cursors.clear();
                                    cursors.add(Cursor {
                                        anchor: position,
                                        position,
                                    });
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
                        Err(_) => log_filename_invaid_utf8(editor, bytes),
                    };
                }
            }
        }
        &RemedybgEvent::BreakpointAdded { breakpoint_id } => {
            editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
                "remedybg: breakpoint added: {}",
                breakpoint_id.0
            ));

            let mut sender =
                remedybg.begin_send_command(platform, RemedybgCommandKind::GetBreakpoint)?;
            let write = sender.write();
            breakpoint_id.serialize(write);
            sender.send(platform);
        }
        &RemedybgEvent::BreakpointModified { breakpoint_id, .. } => {
            editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
                "remedybg: breakpoint modified: {}",
                breakpoint_id.0
            ));

            let mut sender =
                remedybg.begin_send_command(platform, RemedybgCommandKind::GetBreakpoint)?;
            let write = sender.write();
            breakpoint_id.serialize(write);
            sender.send(platform);
        }
        &RemedybgEvent::BreakpointRemoved { breakpoint_id } => {
            editor.logger.write(LogKind::Diagnostic).fmt(format_args!(
                "remedybg: breakpoint removed: {}",
                breakpoint_id.0
            ));

            if let Some(breakpoint) = remedybg.breakpoints.remove(&breakpoint_id) {
                let buffer = editor.buffers.get_mut(breakpoint.buffer_handle);
                let breakpoint_index = buffer
                    .breakpoints()
                    .binary_search_by_key(&breakpoint.line_index, |b| b.line_index);
                if let Ok(breakpoint_index) = breakpoint_index {
                    let mut breakpoints = buffer.breakpoints_mut();
                    breakpoints.remove_at(breakpoint_index, editor.events.writer());
                }

                remedybg
                    .editor_to_remedybg_breakpoint_map
                    .retain(|_, v| v.breakpoint_id.0 != breakpoint_id.0);
            }
        }
        _ => (),
    }

    Ok(())
}

fn on_ipc_output(plugin_handle: PluginHandle, ctx: &mut EditorContext, id: u32, mut bytes: &[u8]) {
    let remedybg = ctx.plugins.get_as::<RemedybgPlugin>(plugin_handle);
    let message_bytes = bytes;

    match id {
        CONTROL_PIPE_ID => match remedybg.pending_commands.pop_front() {
            Some(command_kind) => {
                if let Err(error) = on_control_response(
                    remedybg,
                    &mut ctx.editor,
                    &mut ctx.platform,
                    command_kind,
                    bytes,
                ) {
                    ctx.editor.logger.write(LogKind::Error).fmt(format_args!(
                        "remedybg: error while deserializing command result {}: {}\nmessage:\n{:?}",
                        command_kind as usize, error, message_bytes
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
                    &mut ctx.clients,
                    &event,
                    bytes,
                ) {
                    ctx.editor.logger.write(LogKind::Error).fmt(format_args!(
                        "remedybg: error while deserializing event {}: {}\nmessage:\n{:?}",
                        event, error, message_bytes
                    ));
                }
            }
            Err(_) => {
                let first_u16 = match message_bytes {
                    [b0, b1, ..] => u16::from_le_bytes([*b0, *b1]),
                    _ => 0,
                };
                ctx.editor.logger.write(LogKind::Error).fmt(format_args!(
                    "remedybg: could not deserialize debug event\nmessage:\n{:?}\nfirst u16: {}",
                    message_bytes, first_u16,
                ));
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
