use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
    process::{Command, Stdio},
};

use pepper::{
    buffer_position::BufferRange,
    editor::EditorContext,
    editor_utils::{hash_bytes, parse_process_command, LogKind, Logger},
    events::{EditorEvent, EditorEventIter},
    glob::{Glob, InvalidGlobError},
    platform::{Platform, PlatformProcessHandle, PlatformRequest, ProcessTag},
    plugin::{CompletionContext, Plugin, PluginDefinition, PluginHandle},
    ResourceFile,
};

mod capabilities;
mod client;
mod client_event_handler;
mod command;
mod json;
mod mode;
mod protocol;

use client::{util, Client, ClientHandle};
use json::{JsonObject, JsonValue};
use protocol::{ProtocolError, ResponseError, ServerEvent};

const SERVER_PROCESS_BUFFER_LEN: usize = 4 * 1024;

pub static DEFAULT_CONFIGS: ResourceFile = ResourceFile {
    name: "lsp_default_configs.pepper",
    content: include_str!("../rc/default_configs.pepper"),
};

pub static DEFINITION: PluginDefinition = PluginDefinition {
    instantiate: |handle, ctx| {
        command::register_commands(&mut ctx.editor.commands, handle);
        Some(Plugin {
            data: Box::new(LspPlugin::default()),

            on_editor_events,

            on_process_spawned,
            on_process_output,
            on_process_exit,

            on_completion,

            ..Default::default()
        })
    },
    help_pages: &[ResourceFile {
        name: "lsp_help.md",
        content: include_str!("../rc/help.md"),
    }],
};

struct ClientRecipe {
    glob_hash: u64,
    glob: Glob,
    command: String,
    root: PathBuf,
    running_client: Option<ClientHandle>,
}

enum ClientEntry {
    Occupied(Box<Client>),
    Reserved,
    Vacant,
}
impl ClientEntry {
    pub fn reserve_and_take(&mut self) -> Option<Box<Client>> {
        match self {
            Self::Occupied(_) => {
                let mut client = ClientEntry::Reserved;
                std::mem::swap(self, &mut client);
                match client {
                    Self::Occupied(client) => Some(client),
                    _ => unreachable!(),
                }
            }
            _ => None,
        }
    }
}

pub(crate) struct ClientGuard(Box<Client>);
impl Deref for ClientGuard {
    type Target = Client;
    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}
impl DerefMut for ClientGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
impl Drop for ClientGuard {
    fn drop(&mut self) {
        panic!("forgot to call 'release' on LspPlugin with ClientGuard");
    }
}

#[derive(Default)]
pub(crate) struct LspPlugin {
    entries: Vec<ClientEntry>,
    recipes: Vec<ClientRecipe>,
    current_client_handle: Option<ClientHandle>,
}

impl LspPlugin {
    pub fn add_recipe(
        &mut self,
        glob: &str,
        command: &str,
        root: Option<&str>,
    ) -> Result<(), InvalidGlobError> {
        let glob_hash = hash_bytes(glob.as_bytes());
        for recipe in &mut self.recipes {
            if recipe.glob_hash == glob_hash {
                recipe.command.clear();
                recipe.command.push_str(command);
                recipe.root.clear();
                if let Some(path) = root {
                    recipe.root.push(path);
                }
                recipe.running_client = None;
                return Ok(());
            }
        }

        let mut recipe_glob = Glob::default();
        recipe_glob.compile(glob)?;
        self.recipes.push(ClientRecipe {
            glob_hash,
            glob: recipe_glob,
            command: command.into(),
            root: root.unwrap_or("").into(),
            running_client: None,
        });
        Ok(())
    }

    pub fn start(
        &mut self,
        platform: &mut Platform,
        plugin_handle: PluginHandle,
        mut command: Command,
        root: PathBuf,
    ) -> ClientHandle {
        fn find_vacant_entry(lsp: &mut LspPlugin) -> ClientHandle {
            for (i, entry) in lsp.entries.iter_mut().enumerate() {
                if let ClientEntry::Vacant = entry {
                    return ClientHandle(i as _);
                }
            }
            let handle = ClientHandle(lsp.entries.len() as _);
            lsp.entries.push(ClientEntry::Vacant);
            handle
        }

        let handle = find_vacant_entry(self);

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        platform.requests.enqueue(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Plugin {
                plugin_handle,
                id: handle.0 as _,
            },
            command,
            buf_len: SERVER_PROCESS_BUFFER_LEN,
        });

        let client = Client::new(handle, root);
        self.entries[handle.0 as usize] = ClientEntry::Occupied(Box::new(client));
        handle
    }

    pub fn stop(
        &mut self,
        platform: &mut Platform,
        handle: ClientHandle,
        logger: &mut Logger,
    ) -> bool {
        match &mut self.entries[handle.0 as usize] {
            ClientEntry::Occupied(client) => {
                let _ = client.notify(platform, "exit", JsonObject::default(), logger);
                if let Some(process_handle) = client.protocol.process_handle() {
                    platform.requests.enqueue(PlatformRequest::KillProcess {
                        handle: process_handle,
                    });
                }

                self.entries[handle.0 as usize] = ClientEntry::Vacant;
                for recipe in &mut self.recipes {
                    if recipe.running_client == Some(handle) {
                        recipe.running_client = None;
                    }
                }

                true
            }
            _ => false,
        }
    }

    pub fn stop_all(&mut self, platform: &mut Platform, logger: &mut Logger) -> bool {
        let mut any_stopped = false;
        for i in 0..self.entries.len() {
            any_stopped = any_stopped || self.stop(platform, ClientHandle(i as _), logger);
        }

        any_stopped
    }

    pub(crate) fn get_mut(&mut self, handle: ClientHandle) -> Option<&mut Client> {
        match &mut self.entries[handle.0 as usize] {
            ClientEntry::Occupied(client) => Some(client.deref_mut()),
            _ => None,
        }
    }

    pub(crate) fn acquire(&mut self, handle: ClientHandle) -> Option<ClientGuard> {
        self.entries[handle.0 as usize]
            .reserve_and_take()
            .map(ClientGuard)
    }

    pub(crate) fn release(&mut self, mut guard: ClientGuard) {
        let index = guard.handle().0 as usize;
        let raw = guard.deref_mut() as *mut _;
        std::mem::forget(guard);
        let client = unsafe { Box::from_raw(raw) };
        self.entries[index] = ClientEntry::Occupied(client);
    }

    pub(crate) fn find_client<P>(&mut self, mut predicate: P) -> Option<ClientGuard>
    where
        P: FnMut(&Client) -> bool,
    {
        for entry in &mut self.entries {
            if let ClientEntry::Occupied(c) = entry {
                if predicate(c) {
                    let client = entry.reserve_and_take().unwrap();
                    return Some(ClientGuard(client));
                }
            }
        }

        None
    }
}

fn on_editor_events(plugin_handle: PluginHandle, ctx: &mut EditorContext) {
    let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);

    let mut events = EditorEventIter::new();
    while let Some(event) = events.next(ctx.editor.events.reader()) {
        if let EditorEvent::BufferRead { handle } = *event {
            let buffer_path = match ctx.editor.buffers.get(handle).path.to_str() {
                Some(path) => path,
                None => continue,
            };
            let (index, recipe) = match lsp
                .recipes
                .iter_mut()
                .enumerate()
                .find(|(_, r)| r.glob.matches(buffer_path))
            {
                Some(recipe) => recipe,
                None => continue,
            };
            if recipe.running_client.is_some() {
                continue;
            }
            let command = match parse_process_command(&recipe.command) {
                Some(command) => command,
                None => {
                    ctx.editor
                        .logger
                        .write(LogKind::Error)
                        .fmt(format_args!("invalid lsp command '{}'", &recipe.command));
                    continue;
                }
            };

            let root = if recipe.root.as_os_str().is_empty() {
                ctx.editor.current_directory.clone()
            } else {
                recipe.root.clone()
            };

            let client_handle = lsp.start(&mut ctx.platform, plugin_handle, command, root);
            lsp.recipes[index].running_client = Some(client_handle);
        }
    }

    for entry in &mut lsp.entries {
        let client = match entry {
            ClientEntry::Occupied(client) => client,
            _ => continue,
        };
        if !client.initialized {
            continue;
        }

        let mut events = EditorEventIter::new();
        while let Some(event) = events.next(ctx.editor.events.reader()) {
            client.json.clear();

            match *event {
                EditorEvent::Idle => {
                    util::send_pending_did_change(client, &mut ctx.editor, &mut ctx.platform);
                }
                EditorEvent::BufferTextInserts { handle, inserts } => {
                    let buffer = ctx.editor.buffers.get(handle);
                    if buffer.path.to_str() != ctx.editor.logger.log_file_path() {
                        for insert in inserts.as_slice(ctx.editor.events.reader()) {
                            let text = insert.text(ctx.editor.events.reader());
                            let range = BufferRange::between(insert.range.from, insert.range.from);
                            client.versioned_buffers.add_edit(handle, range, text);
                        }
                    }
                }
                EditorEvent::BufferRangeDeletes { handle, deletes } => {
                    let buffer = ctx.editor.buffers.get(handle);
                    if buffer.path.to_str() != ctx.editor.logger.log_file_path() {
                        for &range in deletes.as_slice(ctx.editor.events.reader()) {
                            client.versioned_buffers.add_edit(handle, range, "");
                        }
                    }
                }
                EditorEvent::BufferRead { handle } => {
                    let buffer = ctx.editor.buffers.get(handle);
                    if buffer.path.to_str() != ctx.editor.logger.log_file_path() {
                        client.versioned_buffers.dispose(handle);
                        util::send_did_open(
                            client,
                            &ctx.editor.buffers,
                            &mut ctx.platform,
                            handle,
                            &mut ctx.editor.logger,
                        );
                    }
                }
                EditorEvent::BufferWrite { handle, .. } => {
                    let buffer = ctx.editor.buffers.get(handle);
                    if buffer.path.to_str() != ctx.editor.logger.log_file_path() {
                        util::send_pending_did_change(client, &mut ctx.editor, &mut ctx.platform);
                        util::send_did_save(client, &mut ctx.editor, &mut ctx.platform, handle);
                    }
                }
                EditorEvent::BufferClose { handle } => {
                    let buffer = ctx.editor.buffers.get(handle);
                    if buffer.path.to_str() != ctx.editor.logger.log_file_path() {
                        client.versioned_buffers.dispose(handle);
                        client.diagnostics.on_close_buffer(handle);
                        util::send_pending_did_change(client, &mut ctx.editor, &mut ctx.platform);
                        util::send_did_close(client, &mut ctx.editor, &mut ctx.platform, handle);
                    }
                }
                EditorEvent::FixCursors { .. } => (),
                EditorEvent::BufferBreakpointsChanged { .. } => (),
            }
        }
    }
}

fn on_process_spawned(
    plugin_handle: PluginHandle,
    ctx: &mut EditorContext,
    client_index: u32,
    process_handle: PlatformProcessHandle,
) {
    if let ClientEntry::Occupied(client) =
        &mut ctx.plugins.get_as::<LspPlugin>(plugin_handle).entries[client_index as usize]
    {
        client.protocol.set_process_handle(process_handle);
        client.json.clear();
        client.initialize(&mut ctx.platform, &mut ctx.editor.logger);
    }
}

fn on_process_output(
    plugin_handle: PluginHandle,
    ctx: &mut EditorContext,
    client_index: u32,
    bytes: &[u8],
) {
    let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
    let mut client_guard = match lsp.acquire(ClientHandle(client_index as _)) {
        Some(client) => client,
        None => return,
    };
    let client = client_guard.deref_mut();
    client.json.clear();

    let mut events = client.protocol.parse_events(bytes);
    while let Some(event) = events.next(&mut client.protocol, &mut client.json) {
        match event {
            ServerEvent::ParseError => {
                {
                    let mut log_writer = ctx.editor.logger.write(LogKind::Diagnostic);
                    log_writer.str("lsp: ");
                    log_writer.str("send parse error\nrequest_id: ");
                    let _ = client.json.write(&mut log_writer, &JsonValue::Null);
                }

                client.respond(
                    &mut ctx.platform,
                    JsonValue::Null,
                    Err(ResponseError::parse_error()),
                    &mut ctx.editor.logger,
                );
            }
            ServerEvent::Request(request) => {
                let request_id = request.id.clone();
                match client_event_handler::on_request(client, ctx, request) {
                    Ok(value) => client.respond(
                        &mut ctx.platform,
                        request_id,
                        Ok(value),
                        &mut ctx.editor.logger,
                    ),
                    Err(ProtocolError::ParseError) => {
                        client.respond(
                            &mut ctx.platform,
                            request_id,
                            Err(ResponseError::parse_error()),
                            &mut ctx.editor.logger,
                        );
                    }
                    Err(ProtocolError::MethodNotFound) => {
                        client.respond(
                            &mut ctx.platform,
                            request_id,
                            Err(ResponseError::method_not_found()),
                            &mut ctx.editor.logger,
                        );
                    }
                }
            }
            ServerEvent::Notification(notification) => {
                let result =
                    client_event_handler::on_notification(client, ctx, plugin_handle, notification);
                if let Err(error) = result {
                    ctx.editor
                        .logger
                        .write(LogKind::Error)
                        .fmt(format_args!("lsp protocol error: {}", error));
                }
            }
            ServerEvent::Response(response) => {
                let result =
                    client_event_handler::on_response(client, ctx, plugin_handle, response);
                if let Err(error) = result {
                    ctx.editor
                        .logger
                        .write(LogKind::Error)
                        .fmt(format_args!("lsp protocol error: {}", error));
                }
            }
        }
    }
    events.finish(&mut client.protocol);

    let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
    lsp.release(client_guard);
}

fn on_process_exit(plugin_handle: PluginHandle, ctx: &mut EditorContext, client_index: u32) {
    for buffer in ctx.editor.buffers.iter_mut() {
        let mut lints = buffer.lints.mut_guard(plugin_handle);
        lints.clear();
    }

    let lsp = ctx.plugins.get_as::<LspPlugin>(plugin_handle);
    if let ClientEntry::Occupied(client) = &mut lsp.entries[client_index as usize] {
        {
            let mut log_writer = ctx.editor.logger.write(LogKind::Diagnostic);
            log_writer.str("lsp: ");
            log_writer.str("lsp server stopped");
        }

        let client_handle = client.handle();
        for recipe in &mut lsp.recipes {
            if recipe.running_client == Some(client_handle) {
                recipe.running_client = None;
            }
        }
    }
}

fn on_completion(
    handle: PluginHandle,
    ctx: &mut EditorContext,
    completion_ctx: &CompletionContext,
) -> bool {
    let lsp = ctx.plugins.get_as::<LspPlugin>(handle);
    for entry in &mut lsp.entries {
        let client = match entry {
            ClientEntry::Occupied(client) => client,
            _ => continue,
        };
        client.json.clear();

        let mut should_complete = completion_ctx.completion_requested;

        if !should_complete {
            if let Some(c) = ctx
                .editor
                .buffers
                .get(completion_ctx.buffer_handle)
                .content()
                .text_range(completion_ctx.word_range)
                .next()
                .and_then(|s| s.chars().next_back())
            {
                if client.signature_help_triggers().contains(c) {
                    client.signature_help(
                        &mut ctx.editor,
                        &mut ctx.platform,
                        completion_ctx.buffer_handle,
                        completion_ctx.cursor_position,
                    );
                    return false;
                }

                should_complete = client.completion_triggers().contains(c);
            }
        }

        if should_complete {
            client.completion(
                &mut ctx.editor,
                &mut ctx.platform,
                completion_ctx.client_handle,
                completion_ctx.buffer_handle,
                completion_ctx.cursor_position,
            );
            return true;
        }
    }

    false
}
