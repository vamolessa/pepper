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
    help::HelpPages,
    platform::{Platform, PlatformRequest, ProcessId, ProcessTag},
    plugin::{Plugin, PluginDefinition, PluginHandle},
};

mod capabilities;
mod client;
mod json;
mod mode;
mod protocol;

use client::{Client, ClientHandle};
use json::{JsonObject, JsonValue};
use protocol::{ProtocolError, ResponseError, ServerEvent};

const SERVER_PROCESS_BUFFER_LEN: usize = 4 * 1024;

pub static DEFINITION: LspPluginDefinition = LspPluginDefinition;

pub struct LspPluginDefinition;
impl PluginDefinition for LspPluginDefinition {
    fn instantiate(
        &self,
        _: &mut Editor,
        _: &mut Platform,
        handle: PluginHandle,
    ) -> Box<dyn Plugin> {
        Box::new(LspPlugin::new(handle))
    }

    fn help_pages(&self) -> &'static HelpPages {
        static HELP_PAGES: HelpPages = HelpPages::new(&[]);
        &HELP_PAGES
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
    plugin_handle: PluginHandle,
    clients: Vec<Option<Box<Client>>>,
    recipes: Vec<ClientRecipe>,
    read_line_client_handle: Option<ClientHandle>,
    picker_client_handle: Option<ClientHandle>,
}

impl LspPlugin {
    pub fn new(plugin_handle: PluginHandle) -> Self {
        Self {
            plugin_handle,
            clients: Vec::new(),
            recipes: Vec::new(),
            read_line_client_handle: None,
            picker_client_handle: None,
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
        mut command: Command,
        root: PathBuf,
        log_file_path: Option<String>,
    ) -> ClientHandle {
        fn find_vacant_entry(this: &mut LspPlugin) -> ClientHandle {
            for (i, client) in this.clients.iter_mut().enumerate() {
                if client.is_none() {
                    return ClientHandle(i as _);
                }
            }
            let handle = ClientHandle(this.clients.len() as _);
            this.clients.push(None);
            handle
        }

        let handle = find_vacant_entry(self);

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let process_id = editor.plugins.spawn_process(
            platform,
            command,
            self.plugin_handle,
            SERVER_PROCESS_BUFFER_LEN,
        );

        let client = Client::new(handle, process_id, root, log_file_path);
        self.clients[handle.0 as usize] = Some(Box::new(client));
        handle
    }

    pub fn stop(&mut self, platform: &mut Platform, handle: ClientHandle) {
        if let Some(client) = &mut self.clients[handle.0 as usize] {
            let _ = client.notify(platform, "exit", JsonObject::default());
            if let Some(process_handle) = client.protocol.process_handle() {
                platform.requests.enqueue(PlatformRequest::KillProcess {
                    handle: process_handle,
                });
            }

            self.clients[handle.0 as usize] = None;
            for recipe in &mut self.recipes {
                if recipe.running_client == Some(handle) {
                    recipe.running_client = None;
                }
            }
        }
    }

    pub fn stop_all(&mut self, platform: &mut Platform) {
        for i in 0..self.clients.len() {
            self.stop(platform, ClientHandle(i as _));
        }
    }

    pub fn get(&self, handle: ClientHandle) -> Option<&Client> {
        self.clients[handle.0 as usize].as_deref()
    }

    pub fn get_mut(&mut self, handle: ClientHandle) -> Option<&mut Client> {
        self.clients[handle.0 as usize].as_deref_mut()
    }

    pub fn clients(&self) -> impl DoubleEndedIterator<Item = &Client> {
        self.clients.iter().flat_map(Option::as_deref)
    }

    fn find_client_by_process_id(&mut self, process_id: ProcessId) -> Option<&mut Client> {
        for client in self.clients.iter_mut().flatten() {
            if client.process_id() == process_id {
                return Some(client);
            }
        }

        None
    }
}

impl Plugin for LspPlugin {
    fn on_editor_events(
        &mut self,
        editor: &mut pepper::editor::Editor,
        platform: &mut pepper::platform::Platform,
        clients: &mut pepper::client::ClientManager,
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

                let client_handle = self.start(editor, platform, command, root, log_file_path);
                self.recipes[index].running_client = Some(client_handle);
            }
        }

        for i in 0..self.clients.len() {
            if let Some(client) = &mut self.clients[i] {
                client.on_editor_events(editor, platform);
            }
        }
    }

    fn on_process_spawned(
        &mut self,
        editor: &mut pepper::editor::Editor,
        platform: &mut pepper::platform::Platform,
        clients: &mut pepper::client::ClientManager,
        process_id: pepper::platform::ProcessId,
        process_handle: pepper::platform::PlatformProcessHandle,
    ) {
        if let Some(client) = self.find_client_by_process_id(process_id) {
            client.protocol.set_process_handle(process_handle);
            client.initialize(platform);
        }
    }

    fn on_process_output(
        &mut self,
        editor: &mut pepper::editor::Editor,
        platform: &mut pepper::platform::Platform,
        clients: &mut pepper::client::ClientManager,
        process_id: pepper::platform::ProcessId,
        bytes: &[u8],
    ) {
        let client = match self.find_client_by_process_id(process_id) {
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
    }

    fn on_process_exit(
        &mut self,
        editor: &mut pepper::editor::Editor,
        platform: &mut pepper::platform::Platform,
        clients: &mut pepper::client::ClientManager,
        process_id: pepper::platform::ProcessId,
    ) {
        if let Some(client) = self.find_client_by_process_id(process_id) {
            client.write_to_log_file(|buf, _| {
                use io::Write;
                let _ = write!(buf, "lsp server stopped");
            });

            let client_handle = client.handle();
            for recipe in &mut self.recipes {
                if recipe.running_client == Some(client_handle) {
                    recipe.running_client = None;
                }
            }
        }
    }
}

