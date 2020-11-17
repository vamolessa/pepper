use std::{
    collections::HashMap,
    env, io,
    path::PathBuf,
    process::{self, Command},
    sync::mpsc,
};

use crate::{
    buffer::BufferCollection,
    buffer_position::BufferRange,
    buffer_view::BufferViewCollection,
    client_event::LocalEvent,
    editor::{EditorEvent, StatusMessage},
    glob::Glob,
    json::{FromJson, Json, JsonArray, JsonConvertError, JsonObject, JsonString, JsonValue},
    lsp::{
        capabilities,
        protocol::{
            PendingRequestColection, Protocol, ResponseError, ServerConnection, ServerEvent,
            ServerNotification, ServerRequest, ServerResponse, SharedJson, Uri,
        },
    },
};

pub struct ClientContext<'a> {
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub status_message: &'a mut StatusMessage,
}

#[derive(Default)]
struct GenericCapability(bool);
impl<'json> FromJson<'json> for GenericCapability {
    fn from_json(value: JsonValue, _: &'json Json) -> Result<Self, JsonConvertError> {
        match value {
            JsonValue::Boolean(b) => Ok(Self(b)),
            JsonValue::Object(_) => Ok(Self(true)),
            _ => Err(JsonConvertError),
        }
    }
}
#[derive(Default)]
struct RenameCapability {
    on: bool,
    prepare_provider: bool,
}
impl<'json> FromJson<'json> for RenameCapability {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        match value {
            JsonValue::Boolean(b) => Ok(Self {
                on: b,
                prepare_provider: false,
            }),
            JsonValue::Object(options) => Ok(Self {
                on: true,
                prepare_provider: matches!(
                    options.get("prepareProvider", &json),
                    JsonValue::Boolean(true)
                ),
            }),
            _ => Err(JsonConvertError),
        }
    }
}

declare_json_object! {
    #[derive(Default)]
    pub struct ClientCapabilities {
        hoverProvider: GenericCapability,
        renameProvider: RenameCapability,
        documentFormattingProvider: GenericCapability,
        referencesProvider: GenericCapability,
        definitionProvider: GenericCapability,
        declarationProvider: GenericCapability,
        implementationProvider: GenericCapability,
        documentSymbolProvider: GenericCapability,
        workspaceSymbolProvider: GenericCapability,
    }
}

struct Diagnostic {
    message: String,
    range: BufferRange,
}

struct BufferDiagnosticCollection {
    diagnostics: Vec<Diagnostic>,
    diagnostics_len: usize,
}

struct DiagnosticCollection {
    buffer_diagnostics: HashMap<PathBuf, BufferDiagnosticCollection>,
}

pub struct Client {
    name: String,
    protocol: Protocol,
    pending_requests: PendingRequestColection,

    initialized: bool,
    capabilities: ClientCapabilities,
    document_selectors: Vec<Glob>,
}

impl Client {
    fn new(name: String, connection: ServerConnection) -> Self {
        Self {
            name,
            protocol: Protocol::new(connection),
            pending_requests: PendingRequestColection::default(),

            initialized: false,
            capabilities: ClientCapabilities::default(),
            document_selectors: Vec::new(),
        }
    }

    pub fn on_request(
        &mut self,
        ctx: &mut ClientContext,
        json: &mut Json,
        request: ServerRequest,
    ) -> io::Result<()> {
        macro_rules! deserialize {
            ($value:expr) => {
                match FromJson::from_json($value, &json) {
                    Ok(value) => value,
                    Err(_) => return self.on_parse_error(json, request.id),
                }
            };
        }

        match request.method.as_str(&json) {
            "client/registerCapability" => {
                for registration in request.params.get("registrations", &json).elements(&json) {
                    declare_json_object! {
                        struct Registration {
                            method: JsonString,
                            registerOptions: JsonObject,
                        }
                    }

                    let registration: Registration = deserialize!(registration);
                    match registration.method.as_str(&json) {
                        "textDocument/didSave" => {
                            self.document_selectors.clear();
                            for filter in registration
                                .registerOptions
                                .get("documentSelector", &json)
                                .elements(&json)
                            {
                                declare_json_object! {
                                    struct Filter {
                                        pattern: Option<JsonString>,
                                    }
                                }
                                let filter: Filter = deserialize!(filter);
                                let pattern = match filter.pattern {
                                    Some(pattern) => pattern.as_str(&json),
                                    None => continue,
                                };
                                let mut glob = Glob::default();
                                if let Err(_) = glob.compile(pattern.as_bytes()) {
                                    self.document_selectors.clear();
                                    return self.on_parse_error(json, request.id);
                                }
                                self.document_selectors.push(glob);
                            }
                        }
                        _ => (),
                    }
                }
                self.protocol.respond(json, request.id, Ok(JsonValue::Null))
            }
            _ => {
                let error = ResponseError::method_not_found();
                self.protocol.respond(json, request.id, Err(error))
            }
        }
    }

    pub fn on_notification(
        &mut self,
        ctx: &mut ClientContext,
        json: &mut Json,
        notification: ServerNotification,
    ) -> io::Result<()> {
        macro_rules! deserialize {
            ($value:expr) => {
                match FromJson::from_json($value, &json) {
                    Ok(value) => value,
                    Err(_) => return self.on_parse_error(json, JsonValue::Null),
                }
            };
        }

        match notification.method.as_str(json) {
            "textDocument/publishDiagnostics" => {
                declare_json_object! {
                    struct Params {
                        uri: JsonString,
                        diagnostics: JsonArray,
                    }
                }

                let params: Params = deserialize!(notification.params);
                let uri = params.uri.as_str(json);
                let path = match Uri::parse(uri) {
                    Uri::None => return Ok(()),
                    Uri::Path(path) => path,
                };

                for diagnostic in params.diagnostics.elements(json) {
                    declare_json_object! {
                        #[derive(Default)]
                        struct Position {
                            line: usize,
                            character: usize,
                        }
                    }
                    declare_json_object! {
                        #[derive(Default)]
                        struct Range {
                            start: Position,
                            end: Position,
                        }
                    }
                    declare_json_object! {
                        struct Diagnostic {
                            message: JsonString,
                            range: Range,
                        }
                    }

                    let diagnostic: Diagnostic = deserialize!(diagnostic);
                }
            }
            _ => (),
        }

        Ok(())
    }

    pub fn on_response(
        &mut self,
        ctx: &mut ClientContext,
        json: &mut Json,
        response: ServerResponse,
    ) -> io::Result<()> {
        macro_rules! deserialize {
            ($value:expr) => {
                match FromJson::from_json($value, &json) {
                    Ok(value) => value,
                    Err(_) => return self.on_parse_error(json, JsonValue::Null),
                }
            };
        }

        let method = match self.pending_requests.take(response.id) {
            Some(method) => method,
            None => return Ok(()),
        };

        match method {
            "initialize" => match response.result {
                Ok(result) => {
                    self.capabilities = deserialize!(result.get("capabilities", &json));
                    self.initialized = true;

                    self.protocol.notify(
                        json,
                        "initialized",
                        JsonValue::Object(JsonObject::default()),
                    )?;
                }
                Err(_) => unimplemented!(),
            },
            _ => (),
        }

        Ok(())
    }

    pub fn on_parse_error(&mut self, json: &mut Json, request_id: JsonValue) -> io::Result<()> {
        let error = ResponseError::parse_error();
        self.protocol.respond(json, request_id, Err(error))
    }

    pub fn on_editor_events(
        &mut self,
        ctx: &mut ClientContext,
        events: &[EditorEvent],
        json: &mut Json,
    ) -> io::Result<()> {
        if !self.initialized {
            return Ok(());
        }

        for event in events {
            match event {
                EditorEvent::BufferOpen(handle) => {
                    //
                }
                EditorEvent::BufferSave(handle) => {
                    //
                }
                EditorEvent::BufferClose(handle) => {
                    //
                }
            }
        }
        Ok(())
    }

    fn request(
        protocol: &mut Protocol,
        json: &mut Json,
        pending_requests: &mut PendingRequestColection,
        method: &'static str,
        params: JsonObject,
    ) -> io::Result<()> {
        let id = protocol.request(json, method, params.into())?;
        pending_requests.add(id, method);
        Ok(())
    }

    pub fn initialize(&mut self, json: &mut Json) -> io::Result<()> {
        let current_dir = match env::current_dir()?.as_os_str().to_str() {
            Some(path) => json.create_string(path).into(),
            None => JsonValue::Null,
        };

        let mut params = JsonObject::default();
        params.set(
            "processId".into(),
            JsonValue::Integer(process::id() as _),
            json,
        );
        params.set("rootUri".into(), current_dir, json);
        params.set(
            "capabilities".into(),
            capabilities::client_capabilities(json),
            json,
        );

        Self::request(
            &mut self.protocol,
            json,
            &mut self.pending_requests,
            "initialize",
            params,
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClientHandle(usize);

struct ClientCollectionEntry {
    client: Client,
    json: SharedJson,
}

#[derive(Default)]
pub struct ClientCollection {
    entries: Vec<Option<ClientCollectionEntry>>,
}

impl ClientCollection {
    pub fn spawn(
        &mut self,
        name: &str,
        command: Command,
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> io::Result<ClientHandle> {
        for (handle, entry) in self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|e| (ClientHandle(i), e)))
        {
            if entry.client.name == name {
                return Ok(handle);
            }
        }

        let handle = self.find_free_slot();
        let json = SharedJson::new();
        let connection = ServerConnection::spawn(command, handle, json.clone(), event_sender)?;
        self.entries[handle.0] = Some(ClientCollectionEntry {
            client: Client::new(name.into(), connection),
            json,
        });
        Ok(handle)
    }

    pub fn try_access<F, E>(&mut self, handle: ClientHandle, accessor: F) -> Result<(), E>
    where
        F: FnOnce(&mut Client, &mut Json) -> Result<(), E>,
    {
        match &mut self.entries[handle.0] {
            Some(entry) => {
                let mut json = entry.json.write_lock();
                accessor(&mut entry.client, json.get())
            }
            None => Ok(()),
        }
    }

    pub fn on_server_event(
        &mut self,
        ctx: &mut ClientContext,
        handle: ClientHandle,
        event: ServerEvent,
    ) -> io::Result<()> {
        match event {
            ServerEvent::Closed => {
                self.entries[handle.0] = None;
            }
            ServerEvent::ParseError => {
                if let Some(entry) = self.entries[handle.0].as_mut() {
                    let mut json = entry.json.consume_lock();
                    entry.client.on_parse_error(json.get(), JsonValue::Null)?;
                }
            }
            ServerEvent::Request(request) => {
                if let Some(entry) = self.entries[handle.0].as_mut() {
                    let mut json = entry.json.consume_lock();
                    entry.client.on_request(ctx, json.get(), request)?;
                }
            }
            ServerEvent::Notification(notification) => {
                if let Some(entry) = self.entries[handle.0].as_mut() {
                    let mut json = entry.json.consume_lock();
                    entry
                        .client
                        .on_notification(ctx, json.get(), notification)?;
                }
            }
            ServerEvent::Response(response) => {
                if let Some(entry) = self.entries[handle.0].as_mut() {
                    let mut json = entry.json.consume_lock();
                    entry.client.on_response(ctx, json.get(), response)?;
                }
            }
        }
        Ok(())
    }

    pub fn on_editor_events(
        &mut self,
        ctx: &mut ClientContext,
        events: &[EditorEvent],
    ) -> io::Result<()> {
        for entry in self.entries.iter_mut().flatten() {
            let mut json = entry.json.write_lock();
            entry.client.on_editor_events(ctx, events, json.get())?;
        }
        Ok(())
    }

    fn find_free_slot(&mut self) -> ClientHandle {
        for (i, slot) in self.entries.iter_mut().enumerate() {
            if let None = slot {
                return ClientHandle(i);
            }
        }
        let handle = ClientHandle(self.entries.len());
        self.entries.push(None);
        handle
    }
}
