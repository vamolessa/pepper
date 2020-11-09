use std::{
    env, io,
    process::{self, Command},
    sync::{mpsc, Arc, Mutex},
};

use crate::{
    buffer::BufferCollection,
    buffer_view::BufferViewCollection,
    client_event::LocalEvent,
    editor::{EditorEvent, StatusMessage},
    glob::Glob,
    json::{Json, JsonObject, JsonValue},
    lsp::{
        capabilities,
        protocol::{
            PendingRequestColection, Protocol, ResponseError, ServerConnection, ServerEvent,
            ServerNotification, ServerRequest, ServerResponse,
        },
    },
};

pub struct ClientContext<'a> {
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub status_message: &'a mut StatusMessage,
}

#[derive(Default)]
pub struct ClientCapabilities {
    pub hover_provider: bool,
    pub rename_provider: bool,
    pub prepare_rename_provider: bool,
    pub document_formatting_provider: bool,
    pub references_provider: bool,
    pub definition_provider: bool,
    pub declaration_provider: bool,
    pub implementation_provider: bool,
    pub document_symbol_provider: bool,
    pub workspace_symbol_provider: bool,
}

pub struct Client {
    name: String,
    protocol: Protocol,
    json: Arc<Mutex<Json>>,
    pending_requests: PendingRequestColection,

    initialized: bool,
    capabilities: ClientCapabilities,
    document_selectors: Vec<Glob>,
}

impl Client {
    fn new(name: String, connection: ServerConnection, json: Arc<Mutex<Json>>) -> Self {
        Self {
            name,
            protocol: Protocol::new(connection),
            json,
            pending_requests: PendingRequestColection::default(),

            initialized: false,
            capabilities: ClientCapabilities::default(),
            document_selectors: Vec::new(),
        }
    }

    pub fn on_request(
        &mut self,
        ctx: &mut ClientContext,
        request: ServerRequest,
    ) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();

        macro_rules! parse_error {
            () => {{
                let error = ResponseError::parse_error();
                return self.protocol.respond(&mut json, request.id, Err(error));
            }};
        }
        macro_rules! expect_json_array {
            ($value:expr) => {
                match $value {
                    JsonValue::Array(array) => array,
                    _ => parse_error!(),
                }
            };
        }
        macro_rules! expect_json_object {
            ($value:expr) => {
                match $value {
                    JsonValue::Object(object) => object,
                    _ => parse_error!(),
                }
            };
        }

        match request.method.as_str(&json) {
            "client/registerCapability" => {
                let params = expect_json_object!(request.params);
                let registrations = expect_json_array!(params.get("registrations", &json));
                for registration in registrations.iter(&json) {
                    let registration = expect_json_object!(registration);
                    let method = match registration.get("method", &json) {
                        JsonValue::Str(s) => *s,
                        JsonValue::String(s) => s.as_str(&json),
                        _ => parse_error!(),
                    };
                    let options = expect_json_object!(registration.get("registerOptions", &json));
                    match method {
                        "textDocument/didSave" => {
                            let document_selector =
                                expect_json_array!(options.get("documentSelector", &json));
                            self.document_selectors.clear();
                            for filter in document_selector.iter(&json) {
                                let filter = expect_json_object!(filter);
                                let pattern = match filter.get("pattern", &json) {
                                    JsonValue::Str(s) => *s,
                                    JsonValue::String(s) => s.as_str(&json),
                                    _ => continue,
                                };
                                let mut glob = Glob::default();
                                if let Err(_) = glob.compile(pattern.as_bytes()) {
                                    self.document_selectors.clear();
                                    parse_error!();
                                }
                                self.document_selectors.push(glob);
                            }
                        }
                        _ => (),
                    }
                }
                self.protocol
                    .respond(&mut json, request.id, Ok(JsonValue::Null))
            }
            _ => {
                let error = ResponseError::method_not_found();
                self.protocol.respond(&mut json, request.id, Err(error))
            }
        }
    }

    pub fn on_notification(
        &mut self,
        ctx: &mut ClientContext,
        notification: ServerNotification,
    ) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();

        macro_rules! parse_error {
            () => {{
                let error = ResponseError::parse_error();
                eprintln!("\n\nPARSE ERROR at {}:{}\n\n", file!(), line!());
                return self
                    .protocol
                    .respond(&mut json, JsonValue::Null, Err(error));
            }};
        }
        macro_rules! expect_json_integer {
            ($value:expr) => {
                match $value {
                    JsonValue::Integer(integer) => integer,
                    _ => parse_error!(),
                }
            };
        }
        macro_rules! expect_json_array {
            ($value:expr) => {
                match $value {
                    JsonValue::Array(array) => array,
                    _ => parse_error!(),
                }
            };
        }
        macro_rules! expect_json_object {
            ($value:expr) => {
                match $value {
                    JsonValue::Object(object) => object,
                    _ => parse_error!(),
                }
            };
        }

        {
            let mut buf = Vec::new();
            json.write(&mut buf, &notification.params).unwrap();
            let text = std::str::from_utf8(&buf).unwrap();
            eprintln!(
                "\n\nfrom client notification '{}' params:\n{}\n\n",
                notification.method.as_str(&json),
                text
            );
            let params = match &notification.params {
                JsonValue::Object(obj) => obj.clone(),
                v => {
                    dbg!(v);
                    return Ok(());
                }
            };
            buf.clear();
            json.write(&mut buf, &JsonValue::Object(params)).unwrap();
            let text = std::str::from_utf8(&buf).unwrap();
            eprintln!("\n\nuri: {}\n\n", text);
        }

        match notification.method.as_str(&json) {
            "textDocument/publishDiagnostics" => {
                let params = expect_json_object!(notification.params);

                for (k, _) in params.iter(&json) {
                    dbg!(k);
                }

                let uri = match params.get("uri", &json) {
                    JsonValue::Str(s) => *s,
                    JsonValue::String(s) => s.as_str(&json),
                    v => parse_error!(),
                };
                let diagnostics = expect_json_array!(params.get("diagnostics", &json));
                for diagnostic in diagnostics.iter(&json) {
                    let diagnostic = expect_json_object!(diagnostic);
                    for (name, v) in diagnostic.iter(&json) {
                        match name {
                            "message" => (),
                            "range" => {
                                let range = expect_json_object!(v);
                                let start = expect_json_object!(range.get("start", &json));
                                let start_line = expect_json_integer!(start.get("line", &json));
                                let start_column =
                                    expect_json_integer!(start.get("character", &json));
                                let end = expect_json_object!(range.get("end", &json));
                                let end_line = expect_json_integer!(end.get("line", &json));
                                let end_column = expect_json_integer!(end.get("character", &json));
                            }
                            _ => (),
                        }
                    }
                }
            }
            _ => (),
        }

        Ok(())
    }

    pub fn on_response(
        &mut self,
        ctx: &mut ClientContext,
        response: ServerResponse,
    ) -> io::Result<()> {
        let method = match self.pending_requests.take(response.id) {
            Some(method) => method,
            None => return Ok(()),
        };
        let mut json = self.json.lock().unwrap();

        macro_rules! expect_json_object {
            ($value:expr) => {
                match $value {
                    JsonValue::Object(object) => object,
                    _ => {
                        let error = ResponseError::parse_error();
                        return self
                            .protocol
                            .respond(&mut json, JsonValue::Null, Err(error));
                    }
                }
            };
        }

        fn is_true_or_object(value: &JsonValue) -> bool {
            match value {
                JsonValue::Boolean(true) | JsonValue::Object(_) => true,
                _ => false,
            }
        }

        match method {
            "initialize" => match response.result {
                Ok(result) => {
                    let body = expect_json_object!(result);
                    let capabilities = expect_json_object!(body.get("capabilities", &json));

                    self.initialized = true;
                    let c = &mut self.capabilities;
                    for (name, v) in capabilities.iter(&json) {
                        match name {
                            "hoverProvider" => c.hover_provider = is_true_or_object(v),
                            "renameProvider" => match v {
                                JsonValue::Boolean(true) => c.rename_provider = true,
                                JsonValue::Object(options) => {
                                    c.rename_provider = true;
                                    c.prepare_rename_provider = matches!(
                                        options.get("prepareProvider", &json),
                                        JsonValue::Boolean(true)
                                    );
                                }
                                _ => (),
                            },
                            "documentFormattingProvider" => {
                                c.document_formatting_provider = is_true_or_object(v)
                            }
                            "referencesProvider" => c.references_provider = is_true_or_object(v),
                            "definitionProvider" => c.definition_provider = is_true_or_object(v),
                            "declarationProvider" => c.declaration_provider = is_true_or_object(v),
                            "implementationProvider" => {
                                c.implementation_provider = is_true_or_object(v)
                            }
                            "documentSymbolProvider" => {
                                c.document_symbol_provider = is_true_or_object(v)
                            }
                            "workspaceSymbolProvider" => {
                                c.workspace_symbol_provider = is_true_or_object(v)
                            }
                            _ => (),
                        }
                    }

                    self.protocol.notify(
                        &mut json,
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

    pub fn on_parse_error(&mut self) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();
        let error = ResponseError::parse_error();
        self.protocol
            .respond(&mut json, JsonValue::Null, Err(error))
    }

    pub fn on_editor_events(
        &mut self,
        ctx: &mut ClientContext,
        events: &[EditorEvent],
    ) -> io::Result<()> {
        if !self.initialized {
            return Ok(());
        }

        for event in events {
            match event {
                _ => (),
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

    pub fn initialize(&mut self) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();

        let current_dir = match env::current_dir()?.as_os_str().to_str() {
            Some(path) => json.create_string(path).into(),
            None => JsonValue::Null,
        };

        let mut params = JsonObject::default();
        params.set(
            "processId".into(),
            JsonValue::Integer(process::id() as _),
            &mut json,
        );
        params.set("rootUri".into(), current_dir, &mut json);
        params.set(
            "capabilities".into(),
            capabilities::client_capabilities(&mut json),
            &mut json,
        );

        Self::request(
            &mut self.protocol,
            &mut json,
            &mut self.pending_requests,
            "initialize",
            params,
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClientHandle(usize);

#[derive(Default)]
pub struct ClientCollection {
    clients: Vec<Option<Client>>,
}

impl ClientCollection {
    pub fn spawn(
        &mut self,
        name: &str,
        command: Command,
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> io::Result<ClientHandle> {
        for (handle, client) in self
            .clients
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.as_ref().map(|v| (ClientHandle(i), v)))
        {
            if client.name == name {
                return Ok(handle);
            }
        }

        let handle = self.find_free_slot();
        let json = Arc::new(Mutex::new(Json::new()));
        let connection = ServerConnection::spawn(command, handle, json.clone(), event_sender)?;
        self.clients[handle.0] = Some(Client::new(name.into(), connection, json));
        Ok(handle)
    }

    pub fn get(&mut self, handle: ClientHandle) -> Option<&mut Client> {
        self.clients[handle.0].as_mut()
    }

    pub fn on_server_event(
        &mut self,
        ctx: &mut ClientContext,
        handle: ClientHandle,
        event: ServerEvent,
    ) -> io::Result<()> {
        match event {
            ServerEvent::Closed => {
                self.clients[handle.0] = None;
            }
            ServerEvent::ParseError => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_parse_error()?;
                }
            }
            ServerEvent::Request(request) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_request(ctx, request)?;
                }
            }
            ServerEvent::Notification(notification) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_notification(ctx, notification)?;
                }
            }
            ServerEvent::Response(response) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_response(ctx, response)?;
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
        for client in self.clients.iter_mut().flatten() {
            client.on_editor_events(ctx, events)?;
        }
        Ok(())
    }

    fn find_free_slot(&mut self) -> ClientHandle {
        for (i, slot) in self.clients.iter_mut().enumerate() {
            if slot.is_none() {
                return ClientHandle(i);
            }
        }
        let handle = ClientHandle(self.clients.len());
        self.clients.push(None);
        handle
    }
}
