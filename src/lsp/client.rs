use std::{
    io,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::mpsc,
};

use crate::{
    buffer::{BufferCollection, BufferHandle},
    buffer_position::{BufferPosition, BufferRange},
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
    pub root: &'a Path,
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

pub struct Diagnostic {
    pub message: String,
    pub utf16_range: BufferRange,
}

struct BufferDiagnosticCollection {
    path: PathBuf,
    buffer_handle: Option<BufferHandle>,
    diagnostics: Vec<Diagnostic>,
    len: usize,
}
impl BufferDiagnosticCollection {
    pub fn add(&mut self, message: &str, range: BufferRange) {
        if self.len < self.diagnostics.len() {
            let diagnostic = &mut self.diagnostics[self.len];
            diagnostic.message.clear();
            diagnostic.message.push_str(message);
            diagnostic.utf16_range = range;
        } else {
            self.diagnostics.push(Diagnostic {
                message: message.into(),
                utf16_range: range,
            });
        }
        self.len += 1;
    }

    pub fn sort(&mut self) {
        self.diagnostics.sort_by_key(|d| d.utf16_range.from);
    }
}

fn are_same_path_with_root(root_a: &Path, a: &Path, root_b: &Path, b: &Path) -> bool {
    match (a.is_absolute(), b.is_absolute()) {
        (false, false) => root_a
            .components()
            .chain(a.components())
            .eq(root_b.components().chain(b.components())),
        (false, true) => root_a.components().chain(a.components()).eq(b.components()),
        (true, false) => a.components().eq(root_b.components().chain(b.components())),
        (true, true) => a.components().eq(b.components()),
    }
}

#[derive(Default)]
pub struct DiagnosticCollection {
    buffer_diagnostics: Vec<BufferDiagnosticCollection>,
}
impl DiagnosticCollection {
    pub fn buffer_diagnostics(&self, buffer_handle: BufferHandle) -> &[Diagnostic] {
        for diagnostics in &self.buffer_diagnostics {
            if diagnostics.buffer_handle == Some(buffer_handle) {
                return &diagnostics.diagnostics[..diagnostics.len];
            }
        }
        &[]
    }

    fn path_diagnostics_mut(
        &mut self,
        ctx: &ClientContext,
        client_root: &Path,
        path: &Path,
    ) -> &mut BufferDiagnosticCollection {
        let buffer_diagnostics = &mut self.buffer_diagnostics;
        for i in 0..buffer_diagnostics.len() {
            if buffer_diagnostics[i].path == path {
                let diagnostics = &mut buffer_diagnostics[i];
                diagnostics.len = 0;
                return diagnostics;
            }
        }

        let mut buffer_handle = None;
        for (handle, buffer) in ctx.buffers.iter_with_handles() {
            if let Some(buffer_path) = buffer.path() {
                if are_same_path_with_root(ctx.root, buffer_path, client_root, path) {
                    buffer_handle = Some(handle);
                    break;
                }
            }
        }

        let last_index = buffer_diagnostics.len();
        buffer_diagnostics.push(BufferDiagnosticCollection {
            path: path.into(),
            buffer_handle,
            diagnostics: Vec::new(),
            len: 0,
        });
        &mut buffer_diagnostics[last_index]
    }

    pub fn clear_empty(&mut self) {
        let buffer_diagnostics = &mut self.buffer_diagnostics;
        for i in (0..buffer_diagnostics.len()).rev() {
            if buffer_diagnostics[i].len == 0 {
                buffer_diagnostics.swap_remove(i);
            }
        }
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = (&'a Path, Option<BufferHandle>, &'a [Diagnostic])> {
        self.buffer_diagnostics
            .iter()
            .map(|d| (d.path.as_path(), d.buffer_handle, &d.diagnostics[..d.len]))
    }

    pub fn on_open_buffer(
        &mut self,
        ctx: &ClientContext,
        buffer_handle: BufferHandle,
        client_root: &Path,
    ) {
        let buffer_path = match ctx.buffers.get(buffer_handle).and_then(|b| b.path()) {
            Some(path) => path,
            None => return,
        };

        for diagnostics in &mut self.buffer_diagnostics {
            if let None = diagnostics.buffer_handle {
                if are_same_path_with_root(ctx.root, buffer_path, client_root, &diagnostics.path) {
                    diagnostics.buffer_handle = Some(buffer_handle);
                    return;
                }
            }
        }
    }

    pub fn on_save_buffer(
        &mut self,
        ctx: &ClientContext,
        buffer_handle: BufferHandle,
        client_root: &Path,
    ) {
        let buffer_path = match ctx.buffers.get(buffer_handle).and_then(|b| b.path()) {
            Some(path) => path,
            None => return,
        };

        for diagnostics in &mut self.buffer_diagnostics {
            if diagnostics.buffer_handle == Some(buffer_handle) {
                diagnostics.buffer_handle = None;
                if are_same_path_with_root(ctx.root, buffer_path, client_root, &diagnostics.path) {
                    diagnostics.buffer_handle = Some(buffer_handle);
                    return;
                }
            }
        }
    }

    pub fn on_close_buffer(&mut self, buffer_handle: BufferHandle) {
        for diagnostics in &mut self.buffer_diagnostics {
            if diagnostics.buffer_handle == Some(buffer_handle) {
                diagnostics.buffer_handle = None;
                return;
            }
        }
    }
}

pub struct Client {
    name: String,
    root: PathBuf,
    protocol: Protocol,
    pending_requests: PendingRequestColection,

    initialized: bool,
    capabilities: ClientCapabilities,
    document_selectors: Vec<Glob>,
    pub diagnostics: DiagnosticCollection,
}

impl Client {
    fn new(name: String, connection: ServerConnection) -> Self {
        Self {
            name,
            root: PathBuf::new(),
            protocol: Protocol::new(connection),
            pending_requests: PendingRequestColection::default(),

            initialized: false,
            capabilities: ClientCapabilities::default(),
            document_selectors: Vec::new(),
            diagnostics: DiagnosticCollection::default(),
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
                    Err(_) => {
                        return Self::respond_parse_error(&mut self.protocol, json, JsonValue::Null)
                    }
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
                    Err(_) => {
                        return Self::respond_parse_error(&mut self.protocol, json, JsonValue::Null)
                    }
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

                let diagnostics = self.diagnostics.path_diagnostics_mut(ctx, &self.root, path);
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
                    let range = diagnostic.range;
                    let range = BufferRange::between(
                        BufferPosition::line_col(range.start.line, range.start.character),
                        BufferPosition::line_col(range.end.line, range.end.character),
                    );
                    diagnostics.add(diagnostic.message.as_str(json), range);
                }
                diagnostics.sort();
                self.diagnostics.clear_empty();
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
                    Err(_) => {
                        return Self::respond_parse_error(&mut self.protocol, json, JsonValue::Null)
                    }
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
        Self::respond_parse_error(&mut self.protocol, json, request_id)
    }

    fn respond_parse_error(
        protocol: &mut Protocol,
        json: &mut Json,
        request_id: JsonValue,
    ) -> io::Result<()> {
        let error = ResponseError::parse_error();
        protocol.respond(json, request_id, Err(error))
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
                    self.diagnostics.on_open_buffer(ctx, *handle, &self.root);
                    //
                }
                EditorEvent::BufferSave(handle) => {
                    self.diagnostics.on_save_buffer(ctx, *handle, &self.root);
                    //
                }
                EditorEvent::BufferClose(handle) => {
                    self.diagnostics.on_close_buffer(*handle);
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

    pub fn initialize(&mut self, json: &mut Json, root: &Path) -> io::Result<()> {
        self.root.clear();
        self.root.push(root);

        let mut params = JsonObject::default();
        params.set(
            "processId".into(),
            JsonValue::Integer(process::id() as _),
            json,
        );
        let root = json.fmt_string(format_args!("{}", Uri::Path(root)));
        params.set("rootUri".into(), root.into(), json);
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

pub struct ClientCollection {
    event_sender: mpsc::Sender<LocalEvent>,
    entries: Vec<Option<ClientCollectionEntry>>,
}

impl ClientCollection {
    pub fn new(event_sender: mpsc::Sender<LocalEvent>) -> Self {
        Self {
            event_sender,
            entries: Vec::new(),
        }
    }

    pub fn start(&mut self, name: &str, command: Command, root: &Path) -> io::Result<ClientHandle> {
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
        let connection =
            ServerConnection::spawn(command, handle, json.clone(), self.event_sender.clone())?;
        let mut entry = ClientCollectionEntry {
            client: Client::new(name.into(), connection),
            json,
        };
        entry
            .client
            .initialize(entry.json.write_lock().get(), root)?;
        self.entries[handle.0] = Some(entry);
        Ok(handle)
    }

    pub fn clients(&self) -> impl Iterator<Item = &Client> {
        self.entries.iter().flat_map(|e| match e {
            Some(e) => Some(&e.client),
            None => None,
        })
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
