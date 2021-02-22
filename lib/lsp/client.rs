use std::{
    fmt, io,
    ops::Range,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    str::FromStr,
};

use crate::{
    application::ProcessTag,
    buffer::BufferHandle,
    buffer_position::{BufferPosition, BufferRange},
    editor::{Editor, EditorOutput, EditorOutputKind},
    events::EditorEvent,
    glob::Glob,
    json::{
        FromJson, Json, JsonArray, JsonConvertError, JsonInteger, JsonObject, JsonString, JsonValue,
    },
    lsp::{
        capabilities,
        protocol::{
            self, PendingRequestColection, Protocol, ResponseError, ServerEvent,
            ServerNotification, ServerRequest, ServerResponse, Uri,
        },
    },
    platform::{Platform, PlatformRequest, ProcessHandle},
};

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

// TODO: move to buffer.rs
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
        editor: &Editor,
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
        for buffer in editor.buffers.iter() {
            if let Some(buffer_path) = buffer.path() {
                if are_same_path_with_root(&editor.current_directory, buffer_path, path) {
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

    pub fn on_load_buffer(&mut self, editor: &Editor, buffer_handle: BufferHandle) {
        let buffer_path = match editor.buffers.get(buffer_handle).and_then(|b| b.path()) {
            Some(path) => path,
            None => return,
        };

        for diagnostics in &mut self.buffer_diagnostics {
            if let None = diagnostics.buffer_handle {
                if are_same_path_with_root(
                    &editor.current_directory,
                    buffer_path,
                    &diagnostics.path,
                ) {
                    diagnostics.buffer_handle = Some(buffer_handle);
                    return;
                }
            }
        }
    }

    pub fn on_save_buffer(&mut self, editor: &Editor, buffer_handle: BufferHandle) {
        let buffer_path = match editor.buffers.get(buffer_handle).and_then(|b| b.path()) {
            Some(path) => path,
            None => return,
        };

        for diagnostics in &mut self.buffer_diagnostics {
            if diagnostics.buffer_handle == Some(buffer_handle) {
                diagnostics.buffer_handle = None;
                if are_same_path_with_root(
                    &editor.current_directory,
                    buffer_path,
                    &diagnostics.path,
                ) {
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
    root: PathBuf,
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
    fn new(root: PathBuf) -> Self {
        Self {
            protocol: Protocol::new(),
            root,
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
        editor: &Editor,
        platform: &mut Platform,
        json: &mut Json,
        buffer_handle: BufferHandle,
        position: BufferPosition,
    ) {
        if !self.server_capabilities.hoverProvider.0 {
            return;
        }

        if let Some(buffer_path) = editor.buffers.get(buffer_handle).and_then(|b| b.path()) {
            let text_document =
                helper::text_document_with_id(&editor.current_directory, buffer_path, json);
            let position = helper::position(position, json);

            let mut params = JsonObject::default();
            params.set("textDocument".into(), text_document.into(), json);
            params.set("position".into(), position.into(), json);

            self.request(platform, json, "textDocument/hover", params);
        }
    }

    pub fn signature_help(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        json: &mut Json,
        buffer_handle: BufferHandle,
        position: BufferPosition,
    ) {
        if !self.server_capabilities.signatureHelpProvider.on {
            return;
        }

        if let Some(buffer_path) = editor.buffers.get(buffer_handle).and_then(|b| b.path()) {
            let text_document =
                helper::text_document_with_id(&editor.current_directory, buffer_path, json);
            let position = helper::position(position, json);

            let mut params = JsonObject::default();
            params.set("textDocument".into(), text_document.into(), json);
            params.set("position".into(), position.into(), json);

            self.request(platform, json, "textDocument/signatureHelp", params);
        }
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

    fn flush_log_buffer(&mut self, editor: &mut Editor) {
        let buffers = &mut editor.buffers;
        if let Some(buffer) = self.log_buffer_handle.and_then(|h| buffers.get_mut(h)) {
            let content = buffer.content();
            let line_index = content.line_count() - 1;
            let position =
                BufferPosition::line_col(line_index, content.line_at(line_index).as_str().len());
            let text = String::from_utf8_lossy(&self.log_write_buf);
            buffer.insert_text(
                &mut editor.word_database,
                position,
                &text,
                &mut editor.events,
            );
            self.log_write_buf.clear();
        }
    }

    fn on_request(
        &mut self,
        editor: &mut Editor,
        platform: &mut Platform,
        json: &mut Json,
        request: ServerRequest,
    ) {
        macro_rules! deserialize {
            ($value:expr) => {
                match FromJson::from_json($value, &json) {
                    Ok(value) => value,
                    Err(_) => {
                        self.respond(
                            platform,
                            json,
                            JsonValue::Null,
                            Err(ResponseError::parse_error()),
                        );
                        return;
                    }
                }
            };
        }

        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "receive request\nid: ");
            json.write(buf, &request.id);
            let _ = write!(
                buf,
                "\nmethod: '{}'\nparams:\n",
                request.method.as_str(json)
            );
            json.write(buf, &request.params);
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
                                    self.respond(
                                        platform,
                                        json,
                                        request.id,
                                        Err(ResponseError::parse_error()),
                                    );
                                    return;
                                }
                                self.document_selectors.push(glob);
                            }
                        }
                        _ => (),
                    }
                }
                self.respond(platform, json, request.id, Ok(JsonValue::Null));
            }
            _ => self.respond(
                platform,
                json,
                request.id,
                Err(ResponseError::method_not_found()),
            ),
        }
    }

    fn on_notification(
        &mut self,
        editor: &mut Editor,
        platform: &mut Platform,
        json: &mut Json,
        notification: ServerNotification,
    ) {
        macro_rules! deserialize {
            ($value:expr) => {
                match FromJson::from_json($value, &json) {
                    Ok(value) => value,
                    Err(_) => {
                        self.respond(
                            platform,
                            json,
                            JsonValue::Null,
                            Err(ResponseError::parse_error()),
                        );
                        return;
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
            json.write(buf, &notification.params);
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
                    1 => editor.output.write(EditorOutputKind::Error).str(message),
                    2 => editor
                        .output
                        .write(EditorOutputKind::Info)
                        .fmt(format_args!("warning: {}", message)),
                    3 => editor
                        .output
                        .write(EditorOutputKind::Info)
                        .fmt(format_args!("info: {}", message)),
                    4 => editor.output.write(EditorOutputKind::Info).str(message),
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
                    _ => return,
                };

                let diagnostics = self.diagnostics.path_diagnostics_mut(editor, path);
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
    }

    fn on_response(
        &mut self,
        editor: &mut Editor,
        platform: &mut Platform,
        json: &mut Json,
        response: ServerResponse,
    ) {
        macro_rules! deserialize {
            ($value:expr) => {
                match FromJson::from_json($value, json) {
                    Ok(value) => value,
                    Err(_) => {
                        self.respond(
                            platform,
                            json,
                            JsonValue::Null,
                            Err(ResponseError::parse_error()),
                        );
                        return;
                    }
                }
            };
        }

        let method = match self.pending_requests.take(response.id) {
            Some(method) => method,
            None => return,
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
                    json.write(buf, result);
                }
                Err(error) => {
                    let _ = write!(
                        buf,
                        "error_code: {}\nerror_message: '{}'\nerror_data:\n",
                        error.code,
                        error.message.as_str(json)
                    );
                    json.write(buf, &error.data);
                }
            }
        });

        let result = match response.result {
            Ok(result) => result,
            Err(error) => {
                helper::write_response_error(&mut editor.output, method, error, json);
                return;
            }
        };

        match method {
            "initialize" => {
                self.server_capabilities = deserialize!(result.get("capabilities", json));
                self.initialized = true;
                self.notify(platform, json, "initialized", JsonObject::default());

                for buffer in editor.buffers.iter() {
                    helper::send_did_open(self, platform, editor, json, buffer.handle());
                }
            }
            "textDocument/hover" => {
                let contents = result.get("contents".into(), json);
                let info = helper::extract_markup_content(contents, json);
                editor.output.write(EditorOutputKind::Info).str(info);
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

                let signature_help: Option<SignatureHelp> = deserialize!(result);
                if let Some(signature) = signature_help
                    .and_then(|sh| sh.signatures.elements(json).nth(sh.activeSignature))
                {
                    let signature: SignatureInformation = deserialize!(signature);
                    let label = signature.label.as_str(json);
                    let documentation =
                        helper::extract_markup_content(signature.documentation, json);

                    if documentation.is_empty() {
                        editor.output.write(EditorOutputKind::Info).str(label);
                    } else {
                        editor
                            .output
                            .write(EditorOutputKind::Info)
                            .fmt(format_args!("{}\n{}", documentation, label));
                    }
                }
            }
            _ => (),
        }
    }

    fn on_parse_error(&mut self, platform: &mut Platform, json: &mut Json, request_id: JsonValue) {
        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "send parse error\nrequest_id: ");
            json.write(buf, &request_id);
        });
        self.respond(
            platform,
            json,
            request_id,
            Err(ResponseError::parse_error()),
        )
    }

    fn on_editor_events(&mut self, editor: &Editor, platform: &mut Platform, json: &mut Json) {
        if !self.initialized {
            return;
        }

        for event in editor.events.iter() {
            match event {
                EditorEvent::Idle => {
                    helper::send_pending_did_change(self, platform, editor, json);
                }
                EditorEvent::BufferOpen { handle } => {
                    let handle = *handle;
                    self.versioned_buffers.dispose(handle);
                    self.diagnostics.on_load_buffer(editor, handle);
                    helper::send_did_open(self, platform, editor, json, handle);
                }
                EditorEvent::BufferInsertText {
                    handle,
                    range,
                    text,
                } => {
                    let text = text.as_str(&editor.events);
                    let range = BufferRange::between(range.from, range.from);
                    self.versioned_buffers.add_edit(*handle, range, text);
                }
                EditorEvent::BufferDeleteText { handle, range } => {
                    self.versioned_buffers.add_edit(*handle, *range, "");
                }
                EditorEvent::BufferSave { handle, .. } => {
                    let handle = *handle;
                    self.diagnostics.on_save_buffer(editor, handle);
                    helper::send_pending_did_change(self, platform, editor, json);
                    helper::send_did_save(self, platform, editor, json, handle);
                }
                EditorEvent::BufferClose { handle } => {
                    let handle = *handle;
                    if self.log_buffer_handle == Some(handle) {
                        self.log_buffer_handle = None;
                    }
                    self.versioned_buffers.dispose(handle);
                    self.diagnostics.on_close_buffer(handle);
                    helper::send_did_close(self, platform, editor, json, handle);
                }
            }
        }
    }

    fn request(
        &mut self,
        platform: &mut Platform,
        json: &mut Json,
        method: &'static str,
        params: JsonObject,
    ) {
        let params = params.into();
        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "send request\nmethod: '{}'\nparams:\n", method);
            json.write(buf, &params);
        });
        let id = self.protocol.request(platform, json, method, params);
        self.pending_requests.add(id, method);
    }

    fn respond(
        &mut self,
        platform: &mut Platform,
        json: &mut Json,
        request_id: JsonValue,
        result: Result<JsonValue, ResponseError>,
    ) {
        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "send response\nid: ");
            json.write(buf, &request_id);
            match &result {
                Ok(result) => {
                    let _ = write!(buf, "\nresult:\n");
                    json.write(buf, result);
                }
                Err(error) => {
                    let _ = write!(
                        buf,
                        "\nerror.code: {}\nerror.message: {}\nerror.data:\n",
                        error.code,
                        error.message.as_str(json)
                    );
                    json.write(buf, &error.data);
                }
            }
        });
        self.protocol.respond(platform, json, request_id, result);
    }

    fn notify(
        &mut self,
        platform: &mut Platform,
        json: &mut Json,
        method: &'static str,
        params: JsonObject,
    ) {
        let params = params.into();
        self.write_to_log_buffer(|buf| {
            use io::Write;
            let _ = write!(buf, "send notification\nmethod: '{}'\nparams:\n", method);
            json.write(buf, &params);
        });
        self.protocol.notify(platform, json, method, params);
    }

    fn initialize(&mut self, platform: &mut Platform, json: &mut Json) {
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

        let root = json.fmt_string(format_args!("{}", Uri::AbsolutePath(&self.root)));
        params.set("rootUri".into(), root.into(), json);

        params.set(
            "capabilities".into(),
            capabilities::client_capabilities(json),
            json,
        );

        self.request(platform, json, "initialize", params)
    }
}

mod helper {
    use super::*;

    pub fn write_response_error(
        output: &mut EditorOutput,
        method: &str,
        error: ResponseError,
        json: &Json,
    ) {
        let error_message = error.message.as_str(json);
        output.write(EditorOutputKind::Error).fmt(format_args!(
            "[lsp error code {}] {}: '{}'",
            error.code, method, error_message
        ));
    }

    pub fn get_path_uri<'a>(current_directory: &'a Path, path: &'a Path) -> Uri<'a> {
        if path.is_absolute() {
            Uri::AbsolutePath(path)
        } else {
            Uri::RelativePath(current_directory, path)
        }
    }

    pub fn text_document_with_id(
        current_directory: &Path,
        path: &Path,
        json: &mut Json,
    ) -> JsonObject {
        let mut id = JsonObject::default();
        let uri = json.fmt_string(format_args!("{}", get_path_uri(current_directory, path)));
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
        platform: &mut Platform,
        editor: &Editor,
        json: &mut Json,
        buffer_handle: BufferHandle,
    ) {
        if !client.server_capabilities.textDocumentSync.open_close {
            return;
        }

        let buffer = match editor.buffers.get(buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };
        if !buffer.capabilities().can_save {
            return;
        }
        let buffer_path = match buffer.path() {
            Some(path) => path,
            None => return,
        };

        let mut text_document = text_document_with_id(&editor.current_directory, buffer_path, json);
        let language_id = json.create_string(protocol::path_to_language_id(buffer_path));
        text_document.set("languageId".into(), language_id.into(), json);
        text_document.set("version".into(), JsonValue::Integer(0), json);
        let text = json.fmt_string(format_args!("{}", buffer.content()));
        text_document.set("text".into(), text.into(), json);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), json);

        client.notify(platform, json, "textDocument/didOpen", params.into());
    }

    pub fn send_pending_did_change(
        client: &mut Client,
        platform: &mut Platform,
        editor: &Editor,
        json: &mut Json,
    ) {
        if let TextDocumentSyncKind::None = client.server_capabilities.textDocumentSync.change {
            return;
        }

        let mut versioned_buffers = std::mem::take(&mut client.versioned_buffers);
        for (buffer_handle, versioned_buffer) in versioned_buffers.iter_pending_mut() {
            let buffer = match editor.buffers.get(buffer_handle) {
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

            let mut text_document =
                text_document_with_id(&editor.current_directory, buffer_path, json);
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
            client.notify(platform, json, "textDocument/didChange", params.into());
        }
        std::mem::swap(&mut client.versioned_buffers, &mut versioned_buffers);
    }

    pub fn send_did_save(
        client: &mut Client,
        platform: &mut Platform,
        editor: &Editor,
        json: &mut Json,
        buffer_handle: BufferHandle,
    ) {
        if let TextDocumentSyncKind::None = client.server_capabilities.textDocumentSync.save {
            return;
        }

        let buffer = match editor.buffers.get(buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };
        if !buffer.capabilities().can_save {
            return;
        }
        let buffer_path = match buffer.path() {
            Some(path) => path,
            None => return,
        };

        let text_document = text_document_with_id(&editor.current_directory, buffer_path, json);
        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), json);

        if let TextDocumentSyncKind::Full = client.server_capabilities.textDocumentSync.save {
            let text = json.fmt_string(format_args!("{}", buffer.content()));
            params.set("text".into(), text.into(), json);
        }

        client.notify(platform, json, "textDocument/didSave", params.into())
    }

    pub fn send_did_close(
        client: &mut Client,
        platform: &mut Platform,
        editor: &Editor,
        json: &mut Json,
        buffer_handle: BufferHandle,
    ) {
        if !client.server_capabilities.textDocumentSync.open_close {
            return;
        }

        let buffer = match editor.buffers.get(buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };
        if !buffer.capabilities().can_save {
            return;
        }
        let buffer_path = match buffer.path() {
            Some(path) => path,
            None => return,
        };

        let text_document = text_document_with_id(&editor.current_directory, buffer_path, json);
        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), json);

        client.notify(platform, json, "textDocument/didClose", params.into());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClientHandle(usize);
impl fmt::Display for ClientHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for ClientHandle {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse() {
            Ok(i) => Ok(Self(i)),
            Err(_) => Err(()),
        }
    }
}

struct ClientManagerEntry {
    client: Client,
    json: Json,
}

pub struct ClientManager {
    entries: Vec<Option<ClientManagerEntry>>,
}

impl ClientManager {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn start(
        &mut self,
        platform: &mut Platform,
        mut command: Command,
        root: PathBuf,
    ) -> ClientHandle {
        let handle = self.find_free_slot();
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        platform.enqueue_request(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Lsp(handle),
            command,
            stdout_buf_len: protocol::BUFFER_LEN,
            stderr_buf_len: 0,
        });
        self.entries[handle.0] = Some(ClientManagerEntry {
            client: Client::new(root),
            json: Json::new(),
        });
        handle
    }

    pub fn stop(&mut self, platform: &mut Platform, handle: ClientHandle) {
        if let Some(entry) = &mut self.entries[handle.0] {
            let _ = entry
                .client
                .notify(platform, &mut entry.json, "exit", JsonObject::default());
            self.entries[handle.0] = None;
        }
    }

    pub fn stop_all(&mut self, platform: &mut Platform) {
        for i in 0..self.entries.len() {
            self.stop(platform, ClientHandle(i));
        }
    }

    pub fn access<A, R>(editor: &mut Editor, handle: ClientHandle, accessor: A) -> Option<R>
    where
        A: FnOnce(&mut Editor, &mut Client, &mut Json) -> R,
    {
        let mut entry = editor.lsp.entries[handle.0].take()?;
        let result = accessor(editor, &mut entry.client, &mut entry.json);
        editor.lsp.entries[handle.0] = Some(entry);
        Some(result)
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

    pub fn on_process_spawned(
        editor: &mut Editor,
        platform: &mut Platform,
        handle: ClientHandle,
        process_handle: ProcessHandle,
    ) {
        if let Some(mut entry) = editor.lsp.entries[handle.0].take() {
            entry.client.protocol.set_process_handle(process_handle);
            entry.client.initialize(platform, &mut entry.json);
        }
    }

    pub fn on_process_stdout(
        editor: &mut Editor,
        platform: &mut Platform,
        handle: ClientHandle,
        bytes: &[u8],
    ) {
        let (mut client, mut json) = match editor.lsp.entries[handle.0].take() {
            Some(entry) => (entry.client, entry.json),
            None => return,
        };

        let mut events = client.protocol.parse_events(bytes);
        while let Some(event) = events.next(&mut client.protocol, &mut json) {
            match event {
                ServerEvent::Closed => editor.lsp.stop(platform, handle),
                ServerEvent::ParseError => {
                    client.on_parse_error(platform, &mut json, JsonValue::Null)
                }
                ServerEvent::Request(request) => {
                    client.on_request(editor, platform, &mut json, request)
                }
                ServerEvent::Notification(notification) => {
                    client.on_notification(editor, platform, &mut json, notification)
                }
                ServerEvent::Response(response) => {
                    client.on_response(editor, platform, &mut json, response)
                }
            }
            client.flush_log_buffer(editor);
        }
        events.finish(&mut client.protocol);

        editor.lsp.entries[handle.0] = Some(ClientManagerEntry { client, json });
    }

    pub fn on_process_exit(editor: &mut Editor, handle: ClientHandle) {
        editor.lsp.entries[handle.0] = None;
    }

    pub fn on_editor_events(editor: &mut Editor, platform: &mut Platform) {
        for i in 0..editor.lsp.entries.len() {
            if let Some(mut entry) = editor.lsp.entries[i].take() {
                entry
                    .client
                    .on_editor_events(editor, platform, &mut entry.json);
                editor.lsp.entries[i] = Some(entry);
            }
        }
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
