use std::{
    io,
    ops::Range,
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
    editor::{StatusMessage, StatusMessageKind},
    editor_event::{EditorEvent, EditorEventQueue, EditorEventsIter},
    glob::Glob,
    json::{
        FromJson, Json, JsonArray, JsonConvertError, JsonInteger, JsonObject, JsonString, JsonValue,
    },
    lsp::{
        capabilities,
        protocol::{
            self, PendingRequestColection, Protocol, ResponseError, ServerConnection, ServerEvent,
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
    pub editor_events: &'a mut EditorEventQueue,
}

#[derive(Default)]
struct GenericCapability(bool);
impl<'json> FromJson<'json> for GenericCapability {
    fn from_json(value: JsonValue, _: &'json Json) -> Result<Self, JsonConvertError> {
        match value {
            JsonValue::Null => Ok(Self(false)),
            JsonValue::Boolean(b) => Ok(Self(b)),
            JsonValue::Object(_) => Ok(Self(true)),
            _ => Err(JsonConvertError),
        }
    }
}
#[derive(Default)]
struct TriggerCharactersCapability {
    on: bool,
    trigger_characters: String,
}
impl<'json> FromJson<'json> for TriggerCharactersCapability {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        match value {
            JsonValue::Null => Ok(Self {
                on: false,
                trigger_characters: String::new(),
            }),
            JsonValue::Object(options) => {
                let mut trigger_characters = String::new();
                for c in options.get("triggerCharacters".into(), json).elements(json) {
                    if let JsonValue::String(c) = c {
                        let c = c.as_str(json);
                        trigger_characters.push_str(c);
                    }
                }
                Ok(Self {
                    on: true,
                    trigger_characters,
                })
            }
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
            JsonValue::Null => Ok(Self {
                on: false,
                prepare_provider: false,
            }),
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
    open_close: bool,
    change: TextDocumentSyncKind,
    save: TextDocumentSyncKind,
}
impl Default for TextDocumentSyncCapability {
    fn default() -> Self {
        Self {
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
                open_close: false,
                change: TextDocumentSyncKind::None,
                save: TextDocumentSyncKind::None,
            }),
            JsonValue::Integer(1) => Ok(Self {
                open_close: true,
                change: TextDocumentSyncKind::Full,
                save: TextDocumentSyncKind::Full,
            }),
            JsonValue::Integer(2) => Ok(Self {
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
    struct ServerCapabilities {
        textDocumentSync: TextDocumentSyncCapability,
        completionProvider: TriggerCharactersCapability,
        hoverProvider: GenericCapability,
        signatureHelpProvider: TriggerCharactersCapability,
        declarationProvider: GenericCapability,
        definitionProvider: GenericCapability,
        implementationProvider: GenericCapability,
        referencesProvider: GenericCapability,
        documentSymbolProvider: GenericCapability,
        documentFormattingProvider: GenericCapability,
        renameProvider: RenameCapability,
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

fn are_same_path_with_root(root_a: &Path, a: &Path, b: &Path) -> bool {
    if a.is_absolute() {
        a.components().eq(b.components())
    } else {
        root_a.components().chain(a.components()).eq(b.components())
    }
}

struct VersionedBufferEdit {
    buffer_range: BufferRange,
    text_range: Range<usize>,
}
#[derive(Default)]
struct VersionedBuffer {
    version: usize,
    texts: String,
    pending_edits: Vec<VersionedBufferEdit>,
}
impl VersionedBuffer {
    pub fn flush(&mut self) {
        self.texts.clear();
        self.pending_edits.clear();
        self.version += 1;
    }

    pub fn dispose(&mut self) {
        self.flush();
        self.version = 1;
    }
}
#[derive(Default)]
struct VersionedBufferCollection {
    buffers: Vec<VersionedBuffer>,
}
impl VersionedBufferCollection {
    pub fn add_edit(&mut self, buffer_handle: BufferHandle, range: BufferRange, text: &str) {
        let index = buffer_handle.0;
        if index >= self.buffers.len() {
            self.buffers
                .resize_with(index + 1, VersionedBuffer::default);
        }
        let buffer = &mut self.buffers[index];
        let text_range_start = buffer.texts.len();
        buffer.texts.push_str(text);
        buffer.pending_edits.push(VersionedBufferEdit {
            buffer_range: range,
            text_range: text_range_start..buffer.texts.len(),
        });
    }

    pub fn dispose(&mut self, buffer_handle: BufferHandle) {
        if let Some(buffer) = self.buffers.get_mut(buffer_handle.0) {
            buffer.dispose();
        }
    }

    pub fn iter_pending_mut<'a>(
        &'a mut self,
    ) -> impl 'a + Iterator<Item = (BufferHandle, &'a mut VersionedBuffer)> {
        self.buffers
            .iter_mut()
            .enumerate()
            .filter(|(_, e)| !e.pending_edits.is_empty())
            .map(|(i, e)| (BufferHandle(i), e))
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
        for buffer in ctx.buffers.iter() {
            if let Some(buffer_path) = buffer.path() {
                if are_same_path_with_root(ctx.current_directory, buffer_path, path) {
                    buffer_handle = Some(buffer.handle());
                    break;
                }
            }
        }

        let end_index = buffer_diagnostics.len();
        buffer_diagnostics.push(BufferDiagnosticCollection {
            path: path.into(),
            buffer_handle,
            diagnostics: Vec::new(),
            len: 0,
        });
        &mut buffer_diagnostics[end_index]
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

    pub fn on_save_buffer(&mut self, ctx: &ClientContext, buffer_handle: BufferHandle) {
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
    protocol: Protocol,
    pending_requests: PendingRequestColection,

    initialized: bool,
    server_capabilities: ServerCapabilities,
    log_write_buf: Vec<u8>,
    log_buffer_handle: Option<BufferHandle>,
    document_selectors: Vec<Glob>,
    versioned_buffers: VersionedBufferCollection,
    diagnostics: DiagnosticCollection,
}

impl Client {
    fn new(connection: ServerConnection) -> Self {
        Self {
            protocol: Protocol::new(connection),
            pending_requests: PendingRequestColection::default(),

            initialized: false,
            server_capabilities: ServerCapabilities::default(),

            log_write_buf: Vec::new(),
            log_buffer_handle: None,

            document_selectors: Vec::new(),
            versioned_buffers: VersionedBufferCollection::default(),
            diagnostics: DiagnosticCollection::default(),
        }
    }

    pub fn set_log_buffer(&mut self, log_buffer_handle: Option<BufferHandle>) {
        self.log_buffer_handle = log_buffer_handle;
    }

    pub fn handles_path(&self, path: &[u8]) -> bool {
        if self.document_selectors.is_empty() {
            true
        } else {
            self.document_selectors.iter().any(|g| g.matches(path))
        }
    }

    pub fn diagnostics(&self) -> &DiagnosticCollection {
        &self.diagnostics
    }

    pub fn hover(
        &mut self,
        ctx: &ClientContext,
        json: &mut Json,
        buffer_handle: BufferHandle,
        position: BufferPosition,
    ) -> io::Result<()> {
        if !self.server_capabilities.hoverProvider.0 {
            return Ok(());
        }

        if let Some(buffer_path) = ctx.buffers.get(buffer_handle).and_then(|b| b.path()) {
            let text_document = helper::text_document_with_id(ctx, buffer_path, json);
            let position = helper::position(position, json);

            let mut params = JsonObject::default();
            params.set("textDocument".into(), text_document.into(), json);
            params.set("position".into(), position.into(), json);

            self.request(json, "textDocument/hover", params)?;
        }
        Ok(())
    }

    pub fn signature_help(
        &mut self,
        ctx: &ClientContext,
        json: &mut Json,
        buffer_handle: BufferHandle,
        position: BufferPosition,
    ) -> io::Result<()> {
        if !self.server_capabilities.signatureHelpProvider.on {
            return Ok(());
        }

        if let Some(buffer_path) = ctx.buffers.get(buffer_handle).and_then(|b| b.path()) {
            let text_document = helper::text_document_with_id(ctx, buffer_path, json);
            let position = helper::position(position, json);

            let mut params = JsonObject::default();
            params.set("textDocument".into(), text_document.into(), json);
            params.set("position".into(), position.into(), json);

            self.request(json, "textDocument/signatureHelp", params)?;
        }
        Ok(())
    }

    fn write_to_log_buffer<F>(&mut self, writer: F)
    where
        F: FnOnce(&mut Vec<u8>),
    {
        if let Some(_) = self.log_buffer_handle {
            writer(&mut self.log_write_buf);
            self.log_write_buf.extend_from_slice(b"\n----\n\n");
        }
    }

    fn flush_log_buffer(&mut self, ctx: &mut ClientContext) {
        let buffers = &mut *ctx.buffers;
        if let Some(buffer) = self.log_buffer_handle.and_then(|h| buffers.get_mut(h)) {
            let content = buffer.content();
            let line_index = content.line_count() - 1;
            let position =
                BufferPosition::line_col(line_index, content.line_at(line_index).as_str().len());
            let text = String::from_utf8_lossy(&self.log_write_buf);
            buffer.insert_text(ctx.word_database, position, &text, ctx.editor_events);
            self.log_write_buf.clear();
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
                        return self.respond(
                            json,
                            JsonValue::Null,
                            Err(ResponseError::parse_error()),
                        )
                    }
                }
            };
        }

        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "receive request\nid: ");
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
                                    return self.respond(
                                        json,
                                        request.id,
                                        Err(ResponseError::parse_error()),
                                    );
                                }
                                self.document_selectors.push(glob);
                            }
                        }
                        _ => (),
                    }
                }
                self.respond(json, request.id, Ok(JsonValue::Null))
            }
            _ => self.respond(json, request.id, Err(ResponseError::method_not_found())),
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
                        return self.respond(
                            json,
                            JsonValue::Null,
                            Err(ResponseError::parse_error()),
                        )
                    }
                }
            };
        }

        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(
                buf,
                "receive notification\nmethod: '{}'\nparams:\n",
                notification.method.as_str(json)
            );
            let _ = json.write(buf, &notification.params);
        });

        match notification.method.as_str(json) {
            "window/showMessage" => {
                let mut message_type: JsonInteger = 0;
                let mut message = JsonString::default();
                for (key, value) in notification.params.members(json) {
                    match key {
                        "type" => message_type = deserialize!(value),
                        "value" => message = deserialize!(value),
                        _ => (),
                    }
                }
                let message = message.as_str(json);
                match message_type {
                    1 => ctx
                        .status_message
                        .write_str(StatusMessageKind::Error, message),
                    2 => ctx.status_message.write_fmt(
                        StatusMessageKind::Info,
                        format_args!("warning: {}", message),
                    ),
                    3 => ctx
                        .status_message
                        .write_fmt(StatusMessageKind::Info, format_args!("info: {}", message)),
                    4 => ctx
                        .status_message
                        .write_str(StatusMessageKind::Info, message),
                    _ => (),
                }
            }
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
                match FromJson::from_json($value, json) {
                    Ok(value) => value,
                    Err(_) => {
                        return self.respond(
                            json,
                            JsonValue::Null,
                            Err(ResponseError::parse_error()),
                        )
                    }
                }
            };
        }

        let method = match self.pending_requests.take(response.id) {
            Some(method) => method,
            None => return Ok(()),
        };

        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(
                buf,
                "receive response\nid: {}\nmethod: '{}'\n",
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

        let result = match response.result {
            Ok(result) => result,
            Err(error) => {
                helper::write_response_error(ctx, method, error, json);
                return Ok(());
            }
        };

        match method {
            "initialize" => {
                self.server_capabilities = deserialize!(result.get("capabilities", json));
                self.initialized = true;
                self.notify(json, "initialized", JsonObject::default())?;

                for buffer in ctx.buffers.iter() {
                    helper::send_did_open(self, ctx, json, buffer.handle())?;
                }
            }
            "textDocument/hover" => {
                let contents = result.get("contents".into(), json);
                let info = helper::extract_markup_content(contents, json);
                ctx.status_message.write_str(StatusMessageKind::Info, info);
            }
            "textDocument/signatureHelp" => {
                declare_json_object! {
                    struct SignatureHelp {
                        activeSignature: usize,
                        signatures: JsonArray,
                    }
                }
                declare_json_object! {
                    struct SignatureInformation {
                        label: JsonString,
                        documentation: JsonValue,
                    }
                }

                let SignatureHelp {
                    activeSignature: active_signature,
                    signatures,
                } = deserialize!(result);
                if let Some(signature) = signatures.elements(json).nth(active_signature) {
                    let signature: SignatureInformation = deserialize!(signature);
                    let label = signature.label.as_str(json);
                    let documentation =
                        helper::extract_markup_content(signature.documentation, json);

                    if documentation.is_empty() {
                        ctx.status_message.write_str(StatusMessageKind::Info, label);
                    } else {
                        ctx.status_message.write_fmt(
                            StatusMessageKind::Info,
                            format_args!("{}\n{}", documentation, label),
                        );
                    }
                }
            }
            _ => (),
        }

        Ok(())
    }

    fn on_parse_error(&mut self, json: &mut Json, request_id: JsonValue) -> io::Result<()> {
        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "send parse error\nrequest_id: ");
            let _ = json.write(buf, &request_id);
        });
        self.respond(json, request_id, Err(ResponseError::parse_error()))
    }

    fn on_editor_events(
        &mut self,
        ctx: &mut ClientContext,
        events: EditorEventsIter,
        json: &mut Json,
    ) -> io::Result<()> {
        if !self.initialized {
            return Ok(());
        }

        for event in events {
            match event {
                EditorEvent::Idle => {
                    helper::send_pending_did_change(self, ctx, json)?;
                }
                EditorEvent::BufferLoad { handle } => {
                    let handle = *handle;
                    self.versioned_buffers.dispose(handle);
                    self.diagnostics.on_load_buffer(ctx, handle);
                    helper::send_did_open(self, ctx, json, handle)?;
                }
                EditorEvent::BufferOpen { .. } => (),
                EditorEvent::BufferInsertText {
                    handle,
                    range,
                    text,
                } => {
                    let text = text.as_str(events);
                    let range = BufferRange::between(range.from, range.from);
                    self.versioned_buffers.add_edit(*handle, range, text);
                }
                EditorEvent::BufferDeleteText { handle, range } => {
                    self.versioned_buffers.add_edit(*handle, *range, "");
                }
                EditorEvent::BufferSave { handle, .. } => {
                    let handle = *handle;
                    self.diagnostics.on_save_buffer(ctx, handle);
                    helper::send_pending_did_change(self, ctx, json)?;
                    helper::send_did_save(self, ctx, json, handle)?;
                }
                EditorEvent::BufferClose { handle } => {
                    let handle = *handle;
                    if self.log_buffer_handle == Some(handle) {
                        self.log_buffer_handle = None;
                    }
                    self.versioned_buffers.dispose(handle);
                    self.diagnostics.on_close_buffer(handle);
                    helper::send_did_close(self, ctx, json, handle)?;
                }
            }
        }
        Ok(())
    }

    fn request(
        &mut self,
        json: &mut Json,
        method: &'static str,
        params: JsonObject,
    ) -> io::Result<()> {
        let params = params.into();
        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "send request\nmethod: '{}'\nparams:\n", method);
            let _ = json.write(buf, &params);
        });
        let id = self.protocol.request(json, method, params)?;
        self.pending_requests.add(id, method);
        Ok(())
    }

    fn respond(
        &mut self,
        json: &mut Json,
        request_id: JsonValue,
        result: Result<JsonValue, ResponseError>,
    ) -> io::Result<()> {
        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "send response\nid: ");
            let _ = json.write(buf, &request_id);
            match &result {
                Ok(result) => {
                    let _ = write!(buf, "\nresult:\n");
                    let _ = json.write(buf, result);
                }
                Err(error) => {
                    let _ = write!(
                        buf,
                        "\nerror.code: {}\nerror.message: {}\nerror.data:\n",
                        error.code,
                        error.message.as_str(json)
                    );
                    let _ = json.write(buf, &error.data);
                }
            }
        });
        self.protocol.respond(json, request_id, result)
    }

    fn notify(
        &mut self,
        json: &mut Json,
        method: &'static str,
        params: JsonObject,
    ) -> io::Result<()> {
        let params = params.into();
        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "send notification\nmethod: '{}'\nparams:\n", method);
            let _ = json.write(buf, &params);
        });
        self.protocol.notify(json, method, params)
    }

    fn initialize(&mut self, json: &mut Json, root: &Path) -> io::Result<()> {
        let mut params = JsonObject::default();
        params.set(
            "processId".into(),
            JsonValue::Integer(process::id() as _),
            json,
        );

        let mut client_info = JsonObject::default();
        client_info.set("name".into(), env!("CARGO_PKG_NAME").into(), json);
        client_info.set("name".into(), env!("CARGO_PKG_VERSION").into(), json);
        params.set("clientInfo".into(), client_info.into(), json);

        let root = json.fmt_string(format_args!("{}", Uri::AbsolutePath(root)));
        params.set("rootUri".into(), root.into(), json);

        params.set(
            "capabilities".into(),
            capabilities::client_capabilities(json),
            json,
        );

        self.request(json, "initialize", params)
    }
}

mod helper {
    use super::*;

    pub fn write_response_error(
        ctx: &mut ClientContext,
        method: &str,
        error: ResponseError,
        json: &Json,
    ) {
        let error_message = error.message.as_str(json);
        ctx.status_message.write_fmt(
            StatusMessageKind::Error,
            format_args!(
                "[lsp error code {}] {}: '{}'",
                error.code, method, error_message
            ),
        );
    }

    pub fn get_path_uri<'a>(ctx: &'a ClientContext, path: &'a Path) -> Uri<'a> {
        if path.is_absolute() {
            Uri::AbsolutePath(path)
        } else {
            Uri::RelativePath(ctx.current_directory, path)
        }
    }

    pub fn text_document_with_id(ctx: &ClientContext, path: &Path, json: &mut Json) -> JsonObject {
        let mut id = JsonObject::default();
        let uri = json.fmt_string(format_args!("{}", get_path_uri(ctx, path)));
        id.set("uri".into(), uri.into(), json);
        id
    }

    pub fn position(position: BufferPosition, json: &mut Json) -> JsonObject {
        let line = JsonValue::Integer(position.line_index as _);
        let character = JsonValue::Integer(position.column_byte_index as _);
        let mut p = JsonObject::default();
        p.set("line".into(), line, json);
        p.set("character".into(), character, json);
        p
    }

    pub fn range(range: BufferRange, json: &mut Json) -> JsonObject {
        let start = position(range.from, json);
        let end = position(range.to, json);
        let mut r = JsonObject::default();
        r.set("start".into(), start.into(), json);
        r.set("end".into(), end.into(), json);
        r
    }

    pub fn extract_markup_content<'json>(content: JsonValue, json: &'json Json) -> &'json str {
        match content {
            JsonValue::String(s) => s.as_str(json),
            JsonValue::Object(o) => match o.get("value".into(), json) {
                JsonValue::String(s) => s.as_str(json),
                _ => "",
            },
            _ => "",
        }
    }

    pub fn send_did_open(
        client: &mut Client,
        ctx: &ClientContext,
        json: &mut Json,
        buffer_handle: BufferHandle,
    ) -> io::Result<()> {
        if !client.server_capabilities.textDocumentSync.open_close {
            return Ok(());
        }

        let buffer = match ctx.buffers.get(buffer_handle) {
            Some(buffer) => buffer,
            None => return Ok(()),
        };
        if !buffer.capabilities().can_save {
            return Ok(());
        }
        let buffer_path = match buffer.path() {
            Some(path) => path,
            None => return Ok(()),
        };

        let mut text_document = text_document_with_id(ctx, buffer_path, json);
        let language_id = json.create_string(protocol::path_to_language_id(buffer_path));
        text_document.set("languageId".into(), language_id.into(), json);
        text_document.set("version".into(), JsonValue::Integer(0), json);
        let text = json.fmt_string(format_args!("{}", buffer.content()));
        text_document.set("text".into(), text.into(), json);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), json);

        client.notify(json, "textDocument/didOpen", params.into())
    }

    pub fn send_pending_did_change(
        client: &mut Client,
        ctx: &ClientContext,
        json: &mut Json,
    ) -> io::Result<()> {
        if let TextDocumentSyncKind::None = client.server_capabilities.textDocumentSync.change {
            return Ok(());
        }

        let mut versioned_buffers = std::mem::take(&mut client.versioned_buffers);
        for (buffer_handle, versioned_buffer) in versioned_buffers.iter_pending_mut() {
            let buffer = match ctx.buffers.get(buffer_handle) {
                Some(buffer) => buffer,
                None => continue,
            };
            if !buffer.capabilities().can_save {
                continue;
            }
            let buffer_path = match buffer.path() {
                Some(path) => path,
                None => continue,
            };

            let mut text_document = text_document_with_id(ctx, buffer_path, json);
            text_document.set(
                "version".into(),
                JsonValue::Integer(versioned_buffer.version as _),
                json,
            );

            let mut params = JsonObject::default();
            params.set("textDocument".into(), text_document.into(), json);

            let mut content_changes = JsonArray::default();
            match client.server_capabilities.textDocumentSync.save {
                TextDocumentSyncKind::None => (),
                TextDocumentSyncKind::Full => {
                    let text = json.fmt_string(format_args!("{}", buffer.content()));
                    let mut change_event = JsonObject::default();
                    change_event.set("text".into(), text.into(), json);
                    content_changes.push(change_event.into(), json);
                }
                TextDocumentSyncKind::Incremental => {
                    for edit in &versioned_buffer.pending_edits {
                        let mut change_event = JsonObject::default();

                        let edit_range = range(edit.buffer_range, json).into();
                        change_event.set("range".into(), edit_range, json);

                        let text = &versioned_buffer.texts[edit.text_range.clone()];
                        let text = json.create_string(text);
                        change_event.set("text".into(), text.into(), json);

                        content_changes.push(change_event.into(), json);
                    }
                }
            }

            params.set("contentChanges".into(), content_changes.into(), json);

            versioned_buffer.flush();
            client.notify(json, "textDocument/didChange", params.into())?;
        }
        std::mem::swap(&mut client.versioned_buffers, &mut versioned_buffers);

        return Ok(());
    }

    pub fn send_did_save(
        client: &mut Client,
        ctx: &ClientContext,
        json: &mut Json,
        buffer_handle: BufferHandle,
    ) -> io::Result<()> {
        if let TextDocumentSyncKind::None = client.server_capabilities.textDocumentSync.save {
            return Ok(());
        }

        let buffer = match ctx.buffers.get(buffer_handle) {
            Some(buffer) => buffer,
            None => return Ok(()),
        };
        if !buffer.capabilities().can_save {
            return Ok(());
        }
        let buffer_path = match buffer.path() {
            Some(path) => path,
            None => return Ok(()),
        };

        let text_document = text_document_with_id(ctx, buffer_path, json);
        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), json);

        if let TextDocumentSyncKind::Full = client.server_capabilities.textDocumentSync.save {
            let text = json.fmt_string(format_args!("{}", buffer.content()));
            params.set("text".into(), text.into(), json);
        }

        client.notify(json, "textDocument/didSave", params.into())
    }

    pub fn send_did_close(
        client: &mut Client,
        ctx: &ClientContext,
        json: &mut Json,
        buffer_handle: BufferHandle,
    ) -> io::Result<()> {
        if !client.server_capabilities.textDocumentSync.open_close {
            return Ok(());
        }

        let buffer = match ctx.buffers.get(buffer_handle) {
            Some(buffer) => buffer,
            None => return Ok(()),
        };
        if !buffer.capabilities().can_save {
            return Ok(());
        }
        let buffer_path = match buffer.path() {
            Some(path) => path,
            None => return Ok(()),
        };

        let text_document = text_document_with_id(ctx, buffer_path, json);
        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), json);

        client.notify(json, "textDocument/didClose", params.into())
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

    pub fn start(&mut self, command: Command, root: &Path) -> io::Result<ClientHandle> {
        let handle = self.find_free_slot();
        let json = SharedJson::new();
        let connection =
            ServerConnection::spawn(command, handle, json.clone(), self.event_sender.clone())?;
        let mut entry = ClientCollectionEntry {
            client: Client::new(connection),
            json,
        };
        entry
            .client
            .initialize(entry.json.write_lock().get(), root)?;
        self.entries[handle.0] = Some(entry);
        Ok(handle)
    }

    pub fn stop(&mut self, handle: ClientHandle) {
        if let Some(entry) = &mut self.entries[handle.0] {
            let mut json = entry.json.write_lock();
            let _ = entry
                .client
                .notify(json.get(), "exit", JsonObject::default());
        }
        self.entries[handle.0] = None;
    }

    pub fn access<F, R>(&mut self, handle: ClientHandle, func: F) -> Option<R>
    where
        F: FnOnce(&mut Client, &mut Json) -> R,
    {
        match &mut self.entries[handle.0] {
            Some(entry) => {
                let mut json = entry.json.write_lock();
                Some(func(&mut entry.client, json.get()))
            }
            None => None,
        }
    }

    pub fn clients(&self) -> impl DoubleEndedIterator<Item = &Client> {
        self.entries.iter().flat_map(|e| match e {
            Some(e) => Some(&e.client),
            None => None,
        })
    }

    pub fn client_with_handles(&self) -> impl Iterator<Item = (ClientHandle, &Client)> {
        self.entries.iter().enumerate().flat_map(|(i, e)| match e {
            Some(e) => Some((ClientHandle(i), &e.client)),
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
                self.stop(handle);
            }
            ServerEvent::ParseError => {
                if let Some(entry) = self.entries[handle.0].as_mut() {
                    let mut json = entry.json.read_lock();
                    entry.client.on_parse_error(json.get(), JsonValue::Null)?;
                    entry.client.flush_log_buffer(ctx);
                }
            }
            ServerEvent::Request(request) => {
                if let Some(entry) = self.entries[handle.0].as_mut() {
                    let mut json = entry.json.read_lock();
                    entry.client.on_request(ctx, json.get(), request)?;
                    entry.client.flush_log_buffer(ctx);
                }
            }
            ServerEvent::Notification(notification) => {
                if let Some(entry) = self.entries[handle.0].as_mut() {
                    let mut json = entry.json.read_lock();
                    entry
                        .client
                        .on_notification(ctx, json.get(), notification)?;
                    entry.client.flush_log_buffer(ctx);
                }
            }
            ServerEvent::Response(response) => {
                if let Some(entry) = self.entries[handle.0].as_mut() {
                    let mut json = entry.json.read_lock();
                    entry.client.on_response(ctx, json.get(), response)?;
                    entry.client.flush_log_buffer(ctx);
                }
            }
        }
        Ok(())
    }

    pub fn on_editor_events(
        &mut self,
        ctx: &mut ClientContext,
        events: EditorEventsIter,
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
