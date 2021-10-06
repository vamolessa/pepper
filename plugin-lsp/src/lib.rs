use std::{
    io,
    path::PathBuf,
    process::{Command, Stdio},
};

use pepper::{
    editor::Editor,
    editor_utils::{hash_bytes, parse_process_command, MessageKind},
    events::{EditorEvent, EditorEventIter},
    glob::{Glob, InvalidGlobError},
    platform::{Platform, PlatformRequest, ProcessTag},
    plugin::{Plugin, PluginHandle},
};

mod capabilities;
mod client;
mod json;
mod protocol;

use client::{Client, ClientHandle};
use json::JsonValue;
use protocol::{ResponseError, ServerEvent};

const SERVER_PROCESS_BUFFER_LEN: usize = 4 * 1024;

enum ClientEntry {
    Vacant,
    Reserved,
    Occupied(Box<Client>),
}
impl ClientEntry {
    pub fn reserve_and_take(&mut self) -> Option<Box<Client>> {
        let mut entry = Self::Reserved;
        std::mem::swap(self, &mut entry);
        match entry {
            Self::Vacant => {
                *self = Self::Vacant;
                None
            }
            Self::Reserved => None,
            Self::Occupied(client) => Some(client),
        }
    }
}

struct ClientRecipe {
    glob_hash: u64,
    glob: Glob,
    command: String,
    root: PathBuf,
    log_file_path: String,
    running_client: Option<ClientHandle>,
}

pub struct LspPlugin {
    entries: Vec<ClientEntry>,
    recipes: Vec<ClientRecipe>,
}

impl LspPlugin {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            recipes: Vec::new(),
        }
    }

    pub fn add_recipe(
        &mut self,
        glob: &str,
        command: &str,
        root: Option<&str>,
        log_file_path: Option<&str>,
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
                recipe.log_file_path.clear();
                if let Some(name) = log_file_path {
                    recipe.log_file_path.push_str(name);
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
            log_file_path: log_file_path.unwrap_or("").into(),
            running_client: None,
        });
        Ok(())
    }

    pub fn start(
        &mut self,
        editor: &mut Editor,
        platform: &mut Platform,
        plugin_handle: PluginHandle,
        mut command: Command,
        root: PathBuf,
        log_file_path: Option<String>,
    ) -> ClientHandle {
        fn find_vacant_entry(this: &mut LspPlugin) -> ClientHandle {
            for (i, slot) in this.entries.iter_mut().enumerate() {
                if let ClientEntry::Vacant = slot {
                    *slot = ClientEntry::Reserved;
                    return ClientHandle(i as _);
                }
            }
            let handle = ClientHandle(this.entries.len() as _);
            this.entries.push(ClientEntry::Reserved);
            handle
        }

        let handle = find_vacant_entry(self);

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let process_index = editor.plugins.spawn_process(
            platform,
            command,
            plugin_handle,
            SERVER_PROCESS_BUFFER_LEN,
        );

        let client = Client::new(handle, root, log_file_path);
        self.entries[handle.0 as usize] = ClientEntry::Occupied(Box::new(client));
        handle
    }

    pub fn stop(&mut self, platform: &mut Platform, handle: ClientHandle) {
        if let ClientEntry::Occupied(client) = &mut self.entries[handle.0 as usize] {
            let _ = client.notify(platform, "exit", JsonObject::default());
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
        }
    }

    pub fn stop_all(&mut self, platform: &mut Platform) {
        for i in 0..self.entries.len() {
            self.stop(platform, ClientHandle(i as _));
        }
    }

    pub fn get(&self, handle: ClientHandle) -> Option<&Client> {
        match self.entries[handle.0 as usize] {
            ClientEntry::Occupied(ref client) => Some(client),
            _ => None,
        }
    }

    pub fn access<A, R>(editor: &mut Editor, handle: ClientHandle, accessor: A) -> Option<R>
    where
        A: FnOnce(&mut Editor, &mut Client) -> R,
    {
        let mut client = editor.lsp.entries[handle.0 as usize].reserve_and_take()?;
        let result = accessor(editor, &mut client);
        editor.lsp.entries[handle.0 as usize] = ClientEntry::Occupied(client);
        Some(result)
    }

    pub fn clients(&self) -> impl DoubleEndedIterator<Item = &Client> {
        self.entries.iter().flat_map(|e| match e {
            ClientEntry::Occupied(client) => Some(client.as_ref()),
            _ => None,
        })
    }
}

impl Plugin for LspPlugin {
    fn on_editor_events(
        &mut self,
        editor: &mut pepper::editor::Editor,
        platform: &mut pepper::platform::Platform,
        clients: &mut pepper::client::ClientManager,
        plugin_handle: PluginHandle,
    ) {
        let mut events = EditorEventIter::new();
        while let Some(event) = events.next(&editor.events) {
            if let EditorEvent::BufferRead { handle } = *event {
                let buffer_path = match editor.buffers.get(handle).path.to_str() {
                    Some(path) => path,
                    None => continue,
                };
                let (index, recipe) = match self
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
                        editor
                            .status_bar
                            .write(MessageKind::Error)
                            .fmt(format_args!("invalid lsp command '{}'", &recipe.command));
                        continue;
                    }
                };

                let root = if recipe.root.as_os_str().is_empty() {
                    editor.current_directory.clone()
                } else {
                    recipe.root.clone()
                };

                let log_file_path = if recipe.log_file_path.is_empty() {
                    None
                } else {
                    Some(recipe.log_file_path.clone())
                };

                let client_handle = self.start(
                    editor,
                    platform,
                    plugin_handle,
                    command,
                    root,
                    log_file_path,
                );
                self.recipes[index].running_client = Some(client_handle);
            }
        }

        for i in 0..editor.lsp.entries.len() {
            if let Some(mut client) = editor.lsp.entries[i].reserve_and_take() {
                client.on_editor_events(editor, platform);
                editor.lsp.entries[i] = ClientEntry::Occupied(client);
            }
        }
    }

    fn on_process_spawned(
        &mut self,
        editor: &mut pepper::editor::Editor,
        platform: &mut pepper::platform::Platform,
        clients: &mut pepper::client::ClientManager,
        process_index: pepper::platform::ProcessIndex,
        process_handle: pepper::platform::ProcessHandle,
    ) {
        if let ClientEntry::Occupied(ref mut client) = editor.lsp.entries[handle.0 as usize] {
            client.protocol.set_process_handle(process_handle);
            client.initialize(platform);
        }
    }

    fn on_process_output(
        &mut self,
        editor: &mut pepper::editor::Editor,
        platform: &mut pepper::platform::Platform,
        clients: &mut pepper::client::ClientManager,
        process_index: pepper::platform::ProcessIndex,
        bytes: &[u8],
    ) {
        let mut client = match editor.lsp.entries[handle.0 as usize].reserve_and_take() {
            Some(client) => client,
            None => return,
        };

        let mut events = client.protocol.parse_events(bytes);
        while let Some(event) = events.next(&mut client.protocol, &mut client.json) {
            match event {
                ServerEvent::ParseError => {
                    client.write_to_log_file(|buf, json| {
                        use io::Write;
                        let _ = write!(buf, "send parse error\nrequest_id: ");
                        let _ = json.write(buf, &JsonValue::Null);
                    });
                    client.respond(platform, JsonValue::Null, Err(ResponseError::parse_error()));
                }
                ServerEvent::Request(request) => {
                    let request_id = request.id.clone();
                    match client.on_request(editor, clients, request) {
                        Ok(value) => client.respond(platform, request_id, Ok(value)),
                        Err(ProtocolError::ParseError) => {
                            client.respond(platform, request_id, Err(ResponseError::parse_error()))
                        }
                        Err(ProtocolError::MethodNotFound) => client.respond(
                            platform,
                            request_id,
                            Err(ResponseError::method_not_found()),
                        ),
                    }
                }
                ServerEvent::Notification(notification) => {
                    let _ = client.on_notification(editor, notification);
                }
                ServerEvent::Response(response) => {
                    let _ = client.on_response(editor, platform, clients, response);
                }
            }
        }
        events.finish(&mut client.protocol);

        editor.lsp.entries[handle.0 as usize] = ClientEntry::Occupied(client);
    }

    fn on_process_exit(
        &mut self,
        editor: &mut pepper::editor::Editor,
        platform: &mut pepper::platform::Platform,
        clients: &mut pepper::client::ClientManager,
        process_index: pepper::platform::ProcessIndex,
    ) {
        let index = handle.0 as usize;
        let mut entry = ClientEntry::Vacant;
        std::mem::swap(&mut entry, &mut editor.lsp.entries[index]);
        if let ClientEntry::Occupied(mut client) = entry {
            client.write_to_log_file(|buf, _| {
                use io::Write;
                let _ = write!(buf, "lsp server stopped");
            });
        }

        for recipe in &mut editor.lsp.recipes {
            if recipe.running_client == Some(handle) {
                recipe.running_client = None;
            }
        }
    }
}

