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
    config::Config,
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
    script::ScriptValue,
    word_database::WordDatabase,
};

pub struct ClientContext<'a> {
    pub current_directory: &'a Path,
    pub config: &'a mut Config,

    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub word_database: &'a mut WordDatabase,

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
enum TextDocumentSyncKind {
    None,
    Full,
    Incremental,
}
struct TextDocumentSyncCapability {
    on: bool,
    open_close: bool,
    change: TextDocumentSyncKind,
    save: TextDocumentSyncKind,
}
impl Default for TextDocumentSyncCapability {
    fn default() -> Self {
        Self {
            on: false,
            open_close: false,
            change: TextDocumentSyncKind::None,
            save: TextDocumentSyncKind::None,
        }
    }
}
impl<'json> FromJson<'json> for TextDocumentSyncCapability {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        match value {
            JsonValue::Integer(0) => Ok(Self {
                on: false,
                open_close: false,
                change: TextDocumentSyncKind::None,
                save: TextDocumentSyncKind::None,
            }),
            JsonValue::Integer(1) => Ok(Self {
                on: true,
                open_close: true,
                change: TextDocumentSyncKind::Full,
                save: TextDocumentSyncKind::Full,
            }),
            JsonValue::Integer(2) => Ok(Self {
                on: true,
                open_close: true,
                change: TextDocumentSyncKind::Incremental,
                save: TextDocumentSyncKind::Incremental,
            }),
            JsonValue::Object(options) => {
                let mut open_close = false;
                let mut change = TextDocumentSyncKind::None;
                let mut save = TextDocumentSyncKind::None;
                for (key, value) in options.members(json) {
                    match key {
                        "change" => {
                            change = match value {
                                JsonValue::Integer(0) => TextDocumentSyncKind::None,
                                JsonValue::Integer(1) => TextDocumentSyncKind::Full,
                                JsonValue::Integer(2) => TextDocumentSyncKind::Incremental,
                                _ => return Err(JsonConvertError),
                            }
                        }
                        "openClose" => {
                            open_close = match value {
                                JsonValue::Boolean(b) => b,
                                _ => return Err(JsonConvertError),
                            }
                        }
                        "save" => {
                            save = match value {
                                JsonValue::Boolean(false) => TextDocumentSyncKind::None,
                                JsonValue::Boolean(true) => TextDocumentSyncKind::Incremental,
                                JsonValue::Object(options) => {
                                    match options.get("includeText", json) {
                                        JsonValue::Boolean(true) => TextDocumentSyncKind::Full,
                                        _ => TextDocumentSyncKind::Incremental,
                                    }
                                }
                                _ => return Err(JsonConvertError),
                            }
                        }
                        _ => (),
                    }
                }
                Ok(Self {
                    on: true,
                    open_close,
                    change,
                    save,
                })
            }
            _ => Err(JsonConvertError),
        }
    }
}

declare_json_object! {
    #[derive(Default)]
    pub struct ServerCapabilities {
        hoverProvider: GenericCapability,
        renameProvider: RenameCapability,
        documentFormattingProvider: GenericCapability,
        referencesProvider: GenericCapability,
        definitionProvider: GenericCapability,
        declarationProvider: GenericCapability,
        implementationProvider: GenericCapability,
        documentSymbolProvider: GenericCapability,
        workspaceSymbolProvider: GenericCapability,
        textDocumentSync: TextDocumentSyncCapability,
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

fn are_same_path_with_root(root_a: &Path, a: &Path, b: &Path) -> bool {
    if a.is_absolute() {
        a.components().eq(b.components())
    } else {
        root_a.components().chain(a.components()).eq(b.components())
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
                if are_same_path_with_root(ctx.current_directory, buffer_path, path) {
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

    pub fn iter<'a>(
        &'a self,
    ) -> impl DoubleEndedIterator<Item = (&'a Path, Option<BufferHandle>, &'a [Diagnostic])> {
        self.buffer_diagnostics
            .iter()
            .map(|d| (d.path.as_path(), d.buffer_handle, &d.diagnostics[..d.len]))
    }

    pub fn on_load_buffer(&mut self, ctx: &ClientContext, buffer_handle: BufferHandle) {
        let buffer_path = match ctx.buffers.get(buffer_handle).and_then(|b| b.path()) {
            Some(path) => path,
            None => return,
        };

        for diagnostics in &mut self.buffer_diagnostics {
            if let None = diagnostics.buffer_handle {
                if are_same_path_with_root(ctx.current_directory, buffer_path, &diagnostics.path) {
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
        new_path: bool,
    ) {
        let buffer_path = match ctx.buffers.get(buffer_handle).and_then(|b| b.path()) {
            Some(path) => path,
            None => return,
        };

        for diagnostics in &mut self.buffer_diagnostics {
            if diagnostics.buffer_handle == Some(buffer_handle) {
                diagnostics.buffer_handle = None;
                if are_same_path_with_root(ctx.current_directory, buffer_path, &diagnostics.path) {
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
    protocol: Protocol,
    pending_requests: PendingRequestColection,

    initialized: bool,
    server_capabilities: ServerCapabilities,
    log_write_buf: Vec<u8>,
    log_buffer_handle: Option<BufferHandle>,
    document_selectors: Vec<Glob>,
    pub diagnostics: DiagnosticCollection,
}

impl Client {
    fn new(name: String, connection: ServerConnection) -> Self {
        Self {
            name,
            protocol: Protocol::new(connection),
            pending_requests: PendingRequestColection::default(),

            initialized: false,
            server_capabilities: ServerCapabilities::default(),

            log_write_buf: Vec::new(),
            log_buffer_handle: None,

            document_selectors: Vec::new(),
            diagnostics: DiagnosticCollection::default(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_log_buffer(&mut self, log_buffer_handle: Option<BufferHandle>) {
        self.log_buffer_handle = log_buffer_handle;
    }

    fn write_to_log_buffer<F>(&mut self, ctx: &mut ClientContext, writer: F)
    where
        F: FnOnce(&mut Vec<u8>),
    {
        let buffers = &mut *ctx.buffers;
        let word_database = &mut *ctx.word_database;
        let syntaxes = &ctx.config.syntaxes;
        if let Some((buffer, pool)) = self
            .log_buffer_handle
            .and_then(|h| buffers.get_mut_with_line_pool(h))
        {
            self.log_write_buf.clear();
            writer(&mut self.log_write_buf);
            self.log_write_buf.extend_from_slice(b"\n----\n\n");
            let content = buffer.content();
            let line_index = content.line_count() - 1;
            let position =
                BufferPosition::line_col(line_index, content.line_at(line_index).as_str().len());
            let text = String::from_utf8_lossy(&self.log_write_buf);
            buffer.insert_text(pool, word_database, syntaxes, position, &text, 0);
        }
    }

    fn on_request(
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

        self.write_to_log_buffer(ctx, |buf| {
            use io::Write;
            let _ = write!(buf, "request\nid: ");
            let _ = json.write(buf, &request.id);
            let _ = write!(
                buf,
                "\nmethod: '{}'\nparams:\n",
                request.method.as_str(json)
            );
            let _ = json.write(buf, &request.params);
        });

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
                                    return Self::respond_parse_error(
                                        &mut self.protocol,
                                        json,
                                        request.id,
                                    );
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

    fn on_notification(
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

        self.write_to_log_buffer(ctx, |buf| {
            use io::Write;
            let _ = write!(
                buf,
                "notification\nmethod: '{}'\nparams:\n",
                notification.method.as_str(json)
            );
            let _ = json.write(buf, &notification.params);
        });

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
                    Uri::AbsolutePath(path) => path,
                    _ => return Ok(()),
                };

                let diagnostics = self.diagnostics.path_diagnostics_mut(ctx, path);
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

    fn on_response(
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

        self.write_to_log_buffer(ctx, |buf| {
            use io::Write;
            let _ = write!(
                buf,
                "response\nid: {}\nmethod: '{}'\n",
                response.id.0, method
            );
            match &response.result {
                Ok(result) => {
                    let _ = write!(buf, "result:\n");
                    let _ = json.write(buf, result);
                }
                Err(error) => {
                    let _ = write!(
                        buf,
                        "error_code: {}\nerror_message: '{}'\nerror_data:\n",
                        error.code,
                        error.message.as_str(json)
                    );
                    let _ = json.write(buf, &error.data);
                }
            }
        });

        match method {
            "initialize" => match response.result {
                Ok(result) => {
                    self.server_capabilities = deserialize!(result.get("capabilities", &json));
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

    fn on_parse_error(
        &mut self,
        ctx: &mut ClientContext,
        json: &mut Json,
        request_id: JsonValue,
    ) -> io::Result<()> {
        self.write_to_log_buffer(ctx, |buf| {
            use io::Write;
            let _ = write!(buf, "parse error\nrequest_id: ");
            let _ = json.write(buf, &request_id);
        });

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

    fn on_editor_events(
        &mut self,
        ctx: &mut ClientContext,
        events: &[EditorEvent],
        json: &mut Json,
    ) -> io::Result<()> {
        fn send_on_open(
            client: &mut Client,
            ctx: &mut ClientContext,
            json: &mut Json,
            buffer_handle: BufferHandle,
        ) -> Option<()> {
            let buffer = ctx.buffers.get(buffer_handle)?;

            let mut text_document = JsonObject::default();

            let buffer_path = buffer.path()?;
            let uri = if buffer_path.is_absolute() {
                Uri::AbsolutePath(buffer_path)
            } else {
                Uri::RelativePath(ctx.current_directory, buffer_path)
            };
            let uri = json.fmt_string(format_args!("{}", uri));
            text_document.set("uri".into(), uri.into(), json);

            let mut params = JsonObject::default();
            params.set("textDocument".into(), text_document.into(), json);

            client
                .protocol
                .notify(json, "textDocument/didOpen", params.into())
                .ok()
        }

        if !self.initialized {
            return Ok(());
        }

        for event in events {
            match event {
                EditorEvent::BufferLoad { handle } => {
                    self.diagnostics.on_load_buffer(ctx, *handle);
                    send_on_open(self, ctx, json, *handle);
                }
                EditorEvent::BufferSave { handle, new_path } => {
                    self.diagnostics.on_save_buffer(ctx, *handle, *new_path);
                }
                EditorEvent::BufferClose { handle } => {
                    self.diagnostics.on_close_buffer(*handle);
                }
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

    fn initialize(&mut self, json: &mut Json, root: &Path) -> io::Result<()> {
        let mut params = JsonObject::default();
        params.set(
            "processId".into(),
            JsonValue::Integer(process::id() as _),
            json,
        );
        let root = json.fmt_string(format_args!("{}", Uri::AbsolutePath(root)));
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
impl_from_script!(ClientHandle, value => match value {
    ScriptValue::Integer(n) if n >= 0 => Some(Self(n as _)),
    _ => None,
});
impl_to_script!(ClientHandle, (self, _engine) => ScriptValue::Integer(self.0 as _));

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

    pub fn start(
        &mut self,
        name: String,
        command: Command,
        root: &Path,
    ) -> io::Result<ClientHandle> {
        let handle = self.find_free_slot();
        let json = SharedJson::new();
        let connection =
            ServerConnection::spawn(command, handle, json.clone(), self.event_sender.clone())?;
        let mut entry = ClientCollectionEntry {
            client: Client::new(name, connection),
            json,
        };
        entry
            .client
            .initialize(entry.json.write_lock().get(), root)?;
        self.entries[handle.0] = Some(entry);
        Ok(handle)
    }

    pub fn get_mut(&mut self, handle: ClientHandle) -> Option<&mut Client> {
        match self.entries[handle.0] {
            Some(ClientCollectionEntry { ref mut client, .. }) => Some(client),
            None => None,
        }
    }

    pub fn clients(&self) -> impl DoubleEndedIterator<Item = &Client> {
        self.entries.iter().flat_map(|e| match e {
            Some(e) => Some(&e.client),
            None => None,
        })
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
                    entry
                        .client
                        .on_parse_error(ctx, json.get(), JsonValue::Null)?;
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
