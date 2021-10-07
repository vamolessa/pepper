use std::{
    fmt,
    fs::File,
    io,
    ops::Range,
    path::{Path, PathBuf},
    str::FromStr,
};

use pepper::{
    buffer::{BufferHandle, BufferProperties},
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::BufferViewHandle,
    client,
    cursor::Cursor,
    editor::Editor,
    editor_utils::{MessageKind, StatusBar},
    events::{EditorEvent, EditorEventIter},
    glob::Glob,
    mode::ModeContext,
    navigation_history::NavigationHistory,
    platform::{Platform, ProcessId},
};

use crate::{
    capabilities,
    json::{FromJson, Json, JsonArray, JsonConvertError, JsonObject, JsonValue},
    mode::{picker, read_line},
    protocol::{
        self, DocumentCodeAction, DocumentDiagnostic, DocumentLocation, DocumentPosition,
        DocumentRange, DocumentSymbolInformation, PendingRequestColection, Protocol, ProtocolError,
        ResponseError, Uri,
    },
};

#[derive(Default)]
struct GenericCapability(pub bool);
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
    pub on: bool,
    pub trigger_characters: String,
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
                for c in options.get("triggerCharacters", json).elements(json) {
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
    pub on: bool,
    pub prepare_provider: bool,
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
    pub open_close: bool,
    pub change: TextDocumentSyncKind,
    pub save: TextDocumentSyncKind,
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

#[derive(Default)]
pub(crate) struct ServerCapabilities {
    text_document_sync: TextDocumentSyncCapability,
    completion_provider: TriggerCharactersCapability,
    hover_provider: GenericCapability,
    signature_help_provider: TriggerCharactersCapability,
    declaration_provider: GenericCapability,
    definition_provider: GenericCapability,
    implementation_provider: GenericCapability,
    references_provider: GenericCapability,
    document_symbol_provider: GenericCapability,
    code_action_provider: GenericCapability,
    document_formatting_provider: GenericCapability,
    rename_provider: RenameCapability,
    workspace_symbol_provider: GenericCapability,
}
impl<'json> FromJson<'json> for ServerCapabilities {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "textDocumentSync" => this.text_document_sync = FromJson::from_json(value, json)?,
                "completionProvider" => {
                    this.completion_provider = FromJson::from_json(value, json)?
                }
                "hoverProvider" => this.hover_provider = FromJson::from_json(value, json)?,
                "signatureHelpProvider" => {
                    this.signature_help_provider = FromJson::from_json(value, json)?
                }
                "declarationProvider" => {
                    this.declaration_provider = FromJson::from_json(value, json)?
                }
                "definitionProvider" => {
                    this.definition_provider = FromJson::from_json(value, json)?
                }
                "implementationProvider" => {
                    this.implementation_provider = FromJson::from_json(value, json)?
                }
                "referencesProvider" => {
                    this.references_provider = FromJson::from_json(value, json)?
                }
                "documentSymbolProvider" => {
                    this.document_symbol_provider = FromJson::from_json(value, json)?
                }
                "codeActionProvider" => {
                    this.code_action_provider = FromJson::from_json(value, json)?
                }
                "documentFormattingProvider" => {
                    this.document_formatting_provider = FromJson::from_json(value, json)?
                }
                "renameProvider" => this.rename_provider = FromJson::from_json(value, json)?,
                "workspaceSymbolProvider" => {
                    this.workspace_symbol_provider = FromJson::from_json(value, json)?
                }
                _ => (),
            }
        }
        Ok(this)
    }
}

pub struct Diagnostic {
    pub message: String,
    pub range: BufferRange,
    data: Vec<u8>,
}
impl Diagnostic {
    pub fn as_document_diagnostic(&self, json: &mut Json) -> DocumentDiagnostic {
        let mut reader = io::Cursor::new(&self.data);
        let data = match json.read(&mut reader) {
            Ok(value) => value,
            Err(_) => JsonValue::Null,
        };
        let range = DocumentRange::from_buffer_range(self.range);
        DocumentDiagnostic {
            message: json.create_string(&self.message),
            range,
            data,
        }
    }
}

pub(crate) struct BufferDiagnosticCollection {
    path: PathBuf,
    buffer_handle: Option<BufferHandle>,
    diagnostics: Vec<Diagnostic>,
    len: usize,
}
impl BufferDiagnosticCollection {
    pub fn add(&mut self, diagnostic: DocumentDiagnostic, json: &Json) {
        let message = diagnostic.message.as_str(json);
        let range = diagnostic.range.into_buffer_range();

        if self.len < self.diagnostics.len() {
            let diagnostic = &mut self.diagnostics[self.len];
            diagnostic.message.clear();
            diagnostic.message.push_str(message);
            diagnostic.range = range;
            diagnostic.data.clear();
        } else {
            self.diagnostics.push(Diagnostic {
                message: message.into(),
                range,
                data: Vec::new(),
            });
        }

        let _ = json.write(&mut self.diagnostics[self.len].data, &diagnostic.data);
        self.len += 1;
    }

    pub fn sort(&mut self) {
        self.diagnostics.sort_unstable_by_key(|d| d.range.from);
    }
}

fn is_editor_path_equals_to_lsp_path(
    editor_root: &Path,
    editor_path: &Path,
    lsp_root: &Path,
    lsp_path: &Path,
) -> bool {
    let lsp_components = lsp_root.components().chain(lsp_path.components());
    if editor_path.is_absolute() {
        editor_path.components().eq(lsp_components)
    } else {
        editor_root
            .components()
            .chain(editor_path.components())
            .eq(lsp_components)
    }
}

struct VersionedBufferEdit {
    buffer_range: BufferRange,
    text_range: Range<u32>,
}
struct VersionedBuffer {
    version: usize,
    texts: String,
    pending_edits: Vec<VersionedBufferEdit>,
}
impl VersionedBuffer {
    pub fn new() -> Self {
        Self {
            version: 2,
            texts: String::new(),
            pending_edits: Vec::new(),
        }
    }

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
        let index = buffer_handle.0 as usize;
        if index >= self.buffers.len() {
            self.buffers.resize_with(index + 1, VersionedBuffer::new);
        }
        let buffer = &mut self.buffers[index];
        let text_range_start = buffer.texts.len();
        buffer.texts.push_str(text);
        buffer.pending_edits.push(VersionedBufferEdit {
            buffer_range: range,
            text_range: text_range_start as u32..buffer.texts.len() as u32,
        });
    }

    pub fn dispose(&mut self, buffer_handle: BufferHandle) {
        if let Some(buffer) = self.buffers.get_mut(buffer_handle.0 as usize) {
            buffer.dispose();
        }
    }

    pub fn iter_pending_mut(
        &mut self,
    ) -> impl Iterator<Item = (BufferHandle, &mut VersionedBuffer)> {
        self.buffers
            .iter_mut()
            .enumerate()
            .filter(|(_, e)| !e.pending_edits.is_empty())
            .map(|(i, e)| (BufferHandle(i as _), e))
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

    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&Path, Option<BufferHandle>, &[Diagnostic])> {
        self.buffer_diagnostics
            .iter()
            .map(|d| (d.path.as_path(), d.buffer_handle, &d.diagnostics[..d.len]))
    }

    pub(crate) fn diagnostics_at_path(
        &mut self,
        editor: &Editor,
        root: &Path,
        path: &Path,
    ) -> &mut BufferDiagnosticCollection {
        fn find_buffer_with_path(
            editor: &Editor,
            root: &Path,
            path: &Path,
        ) -> Option<BufferHandle> {
            for buffer in editor.buffers.iter() {
                if is_editor_path_equals_to_lsp_path(
                    &editor.current_directory,
                    &buffer.path,
                    root,
                    path,
                ) {
                    return Some(buffer.handle());
                }
            }
            None
        }

        for i in 0..self.buffer_diagnostics.len() {
            if self.buffer_diagnostics[i].path == path {
                let diagnostics = &mut self.buffer_diagnostics[i];
                diagnostics.len = 0;

                if diagnostics.buffer_handle.is_none() {
                    diagnostics.buffer_handle = find_buffer_with_path(editor, root, path);
                }

                return diagnostics;
            }
        }

        let end_index = self.buffer_diagnostics.len();
        self.buffer_diagnostics.push(BufferDiagnosticCollection {
            path: path.into(),
            buffer_handle: find_buffer_with_path(editor, root, path),
            diagnostics: Vec::new(),
            len: 0,
        });
        &mut self.buffer_diagnostics[end_index]
    }

    pub(crate) fn clear_empty(&mut self) {
        for i in (0..self.buffer_diagnostics.len()).rev() {
            if self.buffer_diagnostics[i].len == 0 {
                self.buffer_diagnostics.swap_remove(i);
            }
        }
    }

    pub(crate) fn on_load_buffer(
        &mut self,
        editor: &Editor,
        buffer_handle: BufferHandle,
        root: &Path,
    ) {
        let buffer_path = &editor.buffers.get(buffer_handle).path;
        for diagnostics in &mut self.buffer_diagnostics {
            if diagnostics.buffer_handle.is_none()
                && is_editor_path_equals_to_lsp_path(
                    &editor.current_directory,
                    buffer_path,
                    root,
                    &diagnostics.path,
                )
            {
                diagnostics.buffer_handle = Some(buffer_handle);
                return;
            }
        }
    }

    pub(crate) fn on_save_buffer(
        &mut self,
        editor: &Editor,
        buffer_handle: BufferHandle,
        root: &Path,
    ) {
        let buffer_path = &editor.buffers.get(buffer_handle).path;
        for diagnostics in &mut self.buffer_diagnostics {
            if diagnostics.buffer_handle == Some(buffer_handle) {
                diagnostics.buffer_handle = None;
                if is_editor_path_equals_to_lsp_path(
                    &editor.current_directory,
                    buffer_path,
                    root,
                    &diagnostics.path,
                ) {
                    diagnostics.buffer_handle = Some(buffer_handle);
                    return;
                }
            }
        }
    }

    pub(crate) fn on_close_buffer(&mut self, buffer_handle: BufferHandle) {
        for diagnostics in &mut self.buffer_diagnostics {
            if diagnostics.buffer_handle == Some(buffer_handle) {
                diagnostics.buffer_handle = None;
                return;
            }
        }
    }
}

pub(crate) enum RequestState {
    Idle,
    Definition {
        client_handle: client::ClientHandle,
    },
    Declaration {
        client_handle: client::ClientHandle,
    },
    Implementation {
        client_handle: client::ClientHandle,
    },
    References {
        client_handle: client::ClientHandle,
        context_len: usize,
        auto_close_buffer: bool,
    },
    Rename {
        client_handle: client::ClientHandle,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
    },
    FinishRename {
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
    },
    CodeAction {
        client_handle: client::ClientHandle,
    },
    FinishCodeAction,
    DocumentSymbols {
        client_handle: client::ClientHandle,
        buffer_view_handle: BufferViewHandle,
    },
    FinishDocumentSymbols {
        buffer_view_handle: BufferViewHandle,
    },
    WorkspaceSymbols {
        client_handle: client::ClientHandle,
    },
    FinishWorkspaceSymbols,
    Formatting {
        buffer_handle: BufferHandle,
    },
    Completion {
        client_handle: client::ClientHandle,
        buffer_handle: BufferHandle,
    },
}
impl RequestState {
    pub fn is_idle(&self) -> bool {
        matches!(self, RequestState::Idle)
    }
}

#[must_use]
pub enum ClientOperation {
    None,
    EnteredReadLineMode,
    EnteredPickerMode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClientHandle(pub(crate) u8);
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

pub struct Client {
    handle: ClientHandle,
    process_id: ProcessId,
    pub(crate) protocol: Protocol,
    pub(crate) json: Json,
    pub(crate) root: PathBuf,
    pub(crate) pending_requests: PendingRequestColection,

    pub(crate) initialized: bool,
    pub(crate) server_capabilities: ServerCapabilities,

    pub(crate) document_selectors: Vec<Glob>,
    versioned_buffers: VersionedBufferCollection,
    pub(crate) diagnostics: DiagnosticCollection,

    pub(crate) temp_edits: Vec<(BufferRange, BufferRange)>,

    pub(crate) request_state: RequestState,
    pub(crate) request_raw_json: Vec<u8>,

    log_file_path: String,
    log_file: Option<io::BufWriter<File>>,
}

impl Client {
    pub(crate) fn new(
        handle: ClientHandle,
        process_id: ProcessId,
        root: PathBuf,
        log_file_path: Option<String>,
    ) -> Self {
        let (log_file_path, log_file) = match log_file_path {
            Some(path) => match File::create(&path) {
                Ok(file) => (path, Some(io::BufWriter::new(file))),
                Err(_) => (String::new(), None),
            },
            None => (String::new(), None),
        };

        Self {
            handle,
            process_id,
            protocol: Protocol::new(),
            json: Json::new(),
            root,
            pending_requests: PendingRequestColection::default(),

            initialized: false,
            server_capabilities: ServerCapabilities::default(),

            document_selectors: Vec::new(),
            versioned_buffers: VersionedBufferCollection::default(),
            diagnostics: DiagnosticCollection::default(),

            request_state: RequestState::Idle,
            request_raw_json: Vec::new(),
            temp_edits: Vec::new(),

            log_file_path,
            log_file,
        }
    }

    pub fn handle(&self) -> ClientHandle {
        self.handle
    }

    pub fn process_id(&self) -> ProcessId {
        self.process_id
    }

    pub fn handles_path(&self, path: &str) -> bool {
        if self.document_selectors.is_empty() {
            true
        } else {
            self.document_selectors.iter().any(|g| g.matches(path))
        }
    }

    pub fn log_file_path(&self) -> Option<&str> {
        if self.log_file_path.is_empty() {
            None
        } else {
            Some(&self.log_file_path)
        }
    }

    pub fn diagnostics(&self) -> &DiagnosticCollection {
        &self.diagnostics
    }

    pub fn signature_help_triggers(&self) -> &str {
        &self
            .server_capabilities
            .signature_help_provider
            .trigger_characters
    }

    pub fn completion_triggers(&self) -> &str {
        &self
            .server_capabilities
            .completion_provider
            .trigger_characters
    }

    pub fn cancel_current_request(&mut self) {
        self.request_state = RequestState::Idle;
    }

    pub fn hover(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
    ) -> ClientOperation {
        if !self.server_capabilities.hover_provider.0 {
            return ClientOperation::None;
        }

        util::send_pending_did_change(self, editor, platform);

        let buffer = editor.buffers.get(buffer_handle);
        let text_document = util::text_document_with_id(&self.root, &buffer.path, &mut self.json);
        let position = DocumentPosition::from_buffer_position(buffer_position);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), &mut self.json);
        params.set(
            "position".into(),
            position.to_json_value(&mut self.json),
            &mut self.json,
        );

        self.request(platform, "textDocument/hover", params);

        ClientOperation::None
    }

    pub fn signature_help(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
    ) -> ClientOperation {
        if !self.server_capabilities.signature_help_provider.on {
            return ClientOperation::None;
        }

        util::send_pending_did_change(self, editor, platform);

        let buffer = editor.buffers.get(buffer_handle);
        let text_document = util::text_document_with_id(&self.root, &buffer.path, &mut self.json);
        let position = DocumentPosition::from_buffer_position(buffer_position);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), &mut self.json);
        params.set(
            "position".into(),
            position.to_json_value(&mut self.json),
            &mut self.json,
        );

        self.request(platform, "textDocument/signatureHelp", params);

        ClientOperation::None
    }

    pub fn definition(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
        client_handle: client::ClientHandle,
    ) -> ClientOperation {
        if !self.server_capabilities.definition_provider.0 || !self.request_state.is_idle() {
            return ClientOperation::None;
        }

        let params =
            util::create_definition_params(self, editor, platform, buffer_handle, buffer_position);
        self.request_state = RequestState::Definition { client_handle };
        self.request(platform, "textDocument/definition", params);

        ClientOperation::None
    }

    pub fn declaration(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
        client_handle: client::ClientHandle,
    ) -> ClientOperation {
        if !self.server_capabilities.declaration_provider.0 || !self.request_state.is_idle() {
            return ClientOperation::None;
        }

        let params =
            util::create_definition_params(self, editor, platform, buffer_handle, buffer_position);
        self.request_state = RequestState::Declaration { client_handle };
        self.request(platform, "textDocument/declaration", params);

        ClientOperation::None
    }

    pub fn implementation(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
        client_handle: client::ClientHandle,
    ) -> ClientOperation {
        if !self.server_capabilities.implementation_provider.0 || !self.request_state.is_idle() {
            return ClientOperation::None;
        }

        let params =
            util::create_definition_params(self, editor, platform, buffer_handle, buffer_position);
        self.request_state = RequestState::Implementation { client_handle };
        self.request(platform, "textDocument/implementation", params);

        ClientOperation::None
    }

    pub fn references(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
        context_len: usize,
        auto_close_buffer: bool,
        client_handle: client::ClientHandle,
    ) -> ClientOperation {
        if !self.server_capabilities.references_provider.0 || !self.request_state.is_idle() {
            return ClientOperation::None;
        }

        util::send_pending_did_change(self, editor, platform);

        let buffer = editor.buffers.get(buffer_handle);
        let text_document = util::text_document_with_id(&self.root, &buffer.path, &mut self.json);
        let position = DocumentPosition::from_buffer_position(buffer_position);

        let mut context = JsonObject::default();
        context.set("includeDeclaration".into(), true.into(), &mut self.json);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), &mut self.json);
        params.set(
            "position".into(),
            position.to_json_value(&mut self.json),
            &mut self.json,
        );
        params.set("context".into(), context.into(), &mut self.json);

        self.request_state = RequestState::References {
            client_handle,
            context_len,
            auto_close_buffer,
        };
        self.request(platform, "textDocument/references", params);

        ClientOperation::None
    }

    pub fn rename(
        &mut self,
        editor: &mut Editor,
        platform: &mut Platform,
        clients: &mut client::ClientManager,
        client_handle: client::ClientHandle,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
    ) -> ClientOperation {
        if !self.server_capabilities.rename_provider.on || !self.request_state.is_idle() {
            return ClientOperation::None;
        }

        util::send_pending_did_change(self, editor, platform);

        let buffer = editor.buffers.get(buffer_handle);
        let text_document = util::text_document_with_id(&self.root, &buffer.path, &mut self.json);
        let position = DocumentPosition::from_buffer_position(buffer_position);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), &mut self.json);
        params.set(
            "position".into(),
            position.to_json_value(&mut self.json),
            &mut self.json,
        );

        if self.server_capabilities.rename_provider.prepare_provider {
            self.request_state = RequestState::Rename {
                client_handle,
                buffer_handle,
                buffer_position,
            };
            self.request(platform, "textDocument/prepareRename", params);

            ClientOperation::None
        } else {
            self.request_state = RequestState::FinishRename {
                buffer_handle,
                buffer_position,
            };
            let mut ctx = ModeContext {
                editor,
                platform,
                clients,
                client_handle,
            };

            read_line::enter_rename_mode(&mut ctx, "")
        }
    }

    pub(crate) fn finish_rename(&mut self, editor: &Editor, platform: &mut Platform) {
        let (buffer_handle, buffer_position) = match self.request_state {
            RequestState::FinishRename {
                buffer_handle,
                buffer_position,
            } => (buffer_handle, buffer_position),
            _ => return,
        };
        self.request_state = RequestState::Idle;
        if !self.server_capabilities.rename_provider.on {
            return;
        }

        util::send_pending_did_change(self, editor, platform);

        let buffer = editor.buffers.get(buffer_handle);
        let text_document = util::text_document_with_id(&self.root, &buffer.path, &mut self.json);
        let position = DocumentPosition::from_buffer_position(buffer_position);
        let new_name = self.json.create_string(editor.read_line.input());

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), &mut self.json);
        params.set(
            "position".into(),
            position.to_json_value(&mut self.json),
            &mut self.json,
        );
        params.set("newName".into(), new_name.into(), &mut self.json);

        self.request(platform, "textDocument/rename", params);
    }

    pub fn code_action(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        client_handle: client::ClientHandle,
        buffer_handle: BufferHandle,
        range: BufferRange,
    ) -> ClientOperation {
        if !self.server_capabilities.code_action_provider.0 || !self.request_state.is_idle() {
            return ClientOperation::None;
        }

        util::send_pending_did_change(self, editor, platform);

        let buffer = editor.buffers.get(buffer_handle);
        let text_document = util::text_document_with_id(&self.root, &buffer.path, &mut self.json);

        let mut diagnostics = JsonArray::default();
        for diagnostic in self.diagnostics.buffer_diagnostics(buffer_handle) {
            if diagnostic.range.from <= range.from && range.from < diagnostic.range.to
                || diagnostic.range.from <= range.to && range.to < diagnostic.range.to
            {
                let diagnostic = diagnostic.as_document_diagnostic(&mut self.json);
                diagnostics.push(diagnostic.to_json_value(&mut self.json), &mut self.json);
            }
        }

        let mut context = JsonObject::default();
        context.set("diagnostics".into(), diagnostics.into(), &mut self.json);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), &mut self.json);
        params.set(
            "range".into(),
            DocumentRange::from_buffer_range(range).to_json_value(&mut self.json),
            &mut self.json,
        );
        params.set("context".into(), context.into(), &mut self.json);

        self.request_state = RequestState::CodeAction { client_handle };
        self.request(platform, "textDocument/codeAction", params);

        ClientOperation::None
    }

    pub(crate) fn finish_code_action(&mut self, editor: &mut Editor, index: usize) {
        match self.request_state {
            RequestState::FinishCodeAction => (),
            _ => return,
        }
        self.request_state = RequestState::Idle;
        if !self.server_capabilities.code_action_provider.0 {
            return;
        }

        let mut reader = io::Cursor::new(&self.request_raw_json);
        let code_actions = match self.json.read(&mut reader) {
            Ok(actions) => actions,
            Err(_) => return,
        };
        if let Some(edit) = code_actions
            .elements(&self.json)
            .filter_map(|a| DocumentCodeAction::from_json(a, &self.json).ok())
            .filter(|a| !a.disabled)
            .map(|a| a.edit)
            .nth(index)
        {
            edit.apply(editor, &mut self.temp_edits, &self.root, &self.json);
        }
    }

    pub fn document_symbols(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        client_handle: client::ClientHandle,
        buffer_view_handle: BufferViewHandle,
    ) -> ClientOperation {
        if !self.server_capabilities.document_symbol_provider.0 || !self.request_state.is_idle() {
            return ClientOperation::None;
        }

        util::send_pending_did_change(self, editor, platform);

        let buffer_handle = editor.buffer_views.get(buffer_view_handle).buffer_handle;
        let buffer_path = &editor.buffers.get(buffer_handle).path;
        let text_document = util::text_document_with_id(&self.root, buffer_path, &mut self.json);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), &mut self.json);

        self.request_state = RequestState::DocumentSymbols {
            client_handle,
            buffer_view_handle,
        };
        self.request(platform, "textDocument/documentSymbol", params);

        ClientOperation::None
    }

    pub(crate) fn finish_document_symbols(
        &mut self,
        editor: &mut Editor,
        clients: &mut client::ClientManager,
        client_handle: client::ClientHandle,
        index: usize,
    ) {
        let buffer_view_handle = match self.request_state {
            RequestState::FinishDocumentSymbols { buffer_view_handle } => buffer_view_handle,
            _ => return,
        };
        self.request_state = RequestState::Idle;
        if !self.server_capabilities.document_symbol_provider.0 {
            return;
        }

        let mut reader = io::Cursor::new(&self.request_raw_json);
        let symbols = match self.json.read(&mut reader) {
            Ok(symbols) => match symbols {
                JsonValue::Array(symbols) => symbols,
                _ => return,
            },
            Err(_) => return,
        };

        fn find_symbol_position(
            symbols: JsonArray,
            json: &Json,
            mut index: usize,
        ) -> Result<DocumentPosition, usize> {
            for symbol in symbols
                .elements(json)
                .filter_map(|s| DocumentSymbolInformation::from_json(s, json).ok())
            {
                if index == 0 {
                    return Ok(symbol.range.start);
                } else {
                    match find_symbol_position(symbol.children.clone(), json, index - 1) {
                        Ok(position) => return Ok(position),
                        Err(i) => index = i,
                    }
                }
            }
            Err(index)
        }

        if let Ok(position) = find_symbol_position(symbols, &self.json, index) {
            NavigationHistory::save_snapshot(clients.get_mut(client_handle), &editor.buffer_views);

            let buffer_view = editor.buffer_views.get_mut(buffer_view_handle);
            let position = position.into_buffer_position();
            let mut cursors = buffer_view.cursors.mut_guard();
            cursors.clear();
            cursors.add(Cursor {
                anchor: position,
                position,
            });
        }
    }

    pub fn workspace_symbols(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        client_handle: client::ClientHandle,
        query: &str,
    ) -> ClientOperation {
        if !self.server_capabilities.workspace_symbol_provider.0 || !self.request_state.is_idle() {
            return ClientOperation::None;
        }

        util::send_pending_did_change(self, editor, platform);

        let query = self.json.create_string(query);
        let mut params = JsonObject::default();
        params.set("query".into(), query.into(), &mut self.json);

        self.request_state = RequestState::WorkspaceSymbols { client_handle };
        self.request(platform, "workspace/symbol", params);

        ClientOperation::None
    }

    pub(crate) fn finish_workspace_symbols(
        &mut self,
        editor: &mut Editor,
        clients: &mut client::ClientManager,
        client_handle: client::ClientHandle,
        index: usize,
    ) {
        self.request_state = RequestState::Idle;
        if !self.server_capabilities.workspace_symbol_provider.0 {
            return;
        }

        let mut reader = io::Cursor::new(&self.request_raw_json);
        let symbols = match self.json.read(&mut reader) {
            Ok(symbols) => symbols,
            Err(_) => return,
        };
        if let Some(symbol) = symbols
            .elements(&self.json)
            .filter_map(|s| DocumentSymbolInformation::from_json(s, &self.json).ok())
            .nth(index)
        {
            let path = match Uri::parse(&self.root, symbol.uri.as_str(&self.json)) {
                Ok(Uri::Path(path)) => path,
                Err(_) => return,
            };

            match editor.buffer_view_handle_from_path(
                client_handle,
                path,
                BufferProperties::text(),
                false,
            ) {
                Ok(buffer_view_handle) => {
                    let client = clients.get_mut(client_handle);
                    client.set_buffer_view_handle(
                        Some(buffer_view_handle),
                        &editor.buffer_views,
                        &mut editor.events,
                    );

                    let buffer_view = editor.buffer_views.get_mut(buffer_view_handle);
                    let position = symbol.range.start.into_buffer_position();
                    let mut cursors = buffer_view.cursors.mut_guard();
                    cursors.clear();
                    cursors.add(Cursor {
                        anchor: position,
                        position,
                    });
                }
                Err(error) => editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error)),
            }
        }
    }

    pub fn formatting(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
    ) -> ClientOperation {
        if !self.server_capabilities.document_formatting_provider.0 || !self.request_state.is_idle()
        {
            return ClientOperation::None;
        }

        util::send_pending_did_change(self, editor, platform);

        let buffer_path = &editor.buffers.get(buffer_handle).path;
        let text_document = util::text_document_with_id(&self.root, buffer_path, &mut self.json);
        let mut options = JsonObject::default();
        options.set(
            "tabSize".into(),
            JsonValue::Integer(editor.config.tab_size.get() as _),
            &mut self.json,
        );
        options.set(
            "insertSpaces".into(),
            (!editor.config.indent_with_tabs).into(),
            &mut self.json,
        );
        options.set("trimTrailingWhitespace".into(), true.into(), &mut self.json);
        options.set("trimFinalNewlines".into(), true.into(), &mut self.json);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), &mut self.json);
        params.set("options".into(), options.into(), &mut self.json);

        self.request_state = RequestState::Formatting { buffer_handle };
        self.request(platform, "textDocument/formatting", params);

        ClientOperation::None
    }

    pub fn completion(
        &mut self,
        editor: &Editor,
        platform: &mut Platform,
        client_handle: client::ClientHandle,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
    ) -> ClientOperation {
        if !self.server_capabilities.completion_provider.on || !self.request_state.is_idle() {
            return ClientOperation::None;
        }

        util::send_pending_did_change(self, editor, platform);

        let buffer = editor.buffers.get(buffer_handle);
        let text_document = util::text_document_with_id(&self.root, &buffer.path, &mut self.json);
        let position = DocumentPosition::from_buffer_position(buffer_position);

        let mut params = JsonObject::default();
        params.set("textDocument".into(), text_document.into(), &mut self.json);
        params.set(
            "position".into(),
            position.to_json_value(&mut self.json),
            &mut self.json,
        );

        self.request_state = RequestState::Completion {
            client_handle,
            buffer_handle,
        };

        self.request(platform, "textDocument/completion", params);

        ClientOperation::None
    }

    pub(crate) fn write_to_log_file<F>(&mut self, writer: F)
    where
        F: FnOnce(&mut io::BufWriter<File>, &mut Json),
    {
        if let Some(ref mut buf) = self.log_file {
            use io::Write;
            writer(buf, &mut self.json);
            let _ = buf.write_all(b"\n\n");
            let _ = buf.flush();
        }
    }

    pub(crate) fn on_editor_events(&mut self, editor: &Editor, platform: &mut Platform) {
        if !self.initialized {
            return;
        }

        let mut events = EditorEventIter::new();
        while let Some(event) = events.next(&editor.events) {
            match *event {
                EditorEvent::Idle => {
                    util::send_pending_did_change(self, editor, platform);
                }
                EditorEvent::BufferRead { handle } => {
                    let handle = handle;
                    self.versioned_buffers.dispose(handle);
                    self.diagnostics.on_load_buffer(editor, handle, &self.root);
                    util::send_did_open(self, editor, platform, handle);
                }
                EditorEvent::BufferInsertText {
                    handle,
                    range,
                    text,
                    ..
                } => {
                    let text = text.as_str(&editor.events);
                    let range = BufferRange::between(range.from, range.from);
                    self.versioned_buffers.add_edit(handle, range, text);
                }
                EditorEvent::BufferDeleteText { handle, range, .. } => {
                    self.versioned_buffers.add_edit(handle, range, "");
                }
                EditorEvent::BufferWrite { handle, .. } => {
                    self.diagnostics.on_save_buffer(editor, handle, &self.root);
                    util::send_pending_did_change(self, editor, platform);
                    util::send_did_save(self, editor, platform, handle);
                }
                EditorEvent::BufferClose { handle } => {
                    self.versioned_buffers.dispose(handle);
                    self.diagnostics.on_close_buffer(handle);
                    util::send_pending_did_change(self, editor, platform);
                    util::send_did_close(self, editor, platform, handle);
                }
                EditorEvent::FixCursors { .. } => (),
                EditorEvent::BufferViewLostFocus { .. } => (),
            }
        }
    }

    fn request(&mut self, platform: &mut Platform, method: &'static str, params: JsonObject) {
        if !self.initialized {
            return;
        }

        let params = params.into();
        self.write_to_log_file(|buf, json| {
            use io::Write;
            let _ = write!(buf, "send request\nmethod: '{}'\nparams:\n", method);
            let _ = json.write(buf, &params);
        });
        let id = self
            .protocol
            .request(platform, &mut self.json, method, params);
        self.json.clear();

        self.pending_requests.add(id, method);
    }

    pub(crate) fn respond(
        &mut self,
        platform: &mut Platform,
        request_id: JsonValue,
        result: Result<JsonValue, ResponseError>,
    ) {
        self.write_to_log_file(|buf, json| {
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
        self.protocol
            .respond(platform, &mut self.json, request_id, result);
        self.json.clear();
    }

    pub(crate) fn notify(
        &mut self,
        platform: &mut Platform,
        method: &'static str,
        params: JsonObject,
    ) {
        let params = params.into();
        self.write_to_log_file(|buf, json| {
            use io::Write;
            let _ = write!(buf, "send notification\nmethod: '{}'\nparams:\n", method);
            let _ = json.write(buf, &params);
        });
        self.protocol
            .notify(platform, &mut self.json, method, params);
        self.json.clear();
    }

    pub fn initialize(&mut self, platform: &mut Platform) {
        let mut params = JsonObject::default();
        params.set(
            "processId".into(),
            JsonValue::Integer(std::process::id() as _),
            &mut self.json,
        );

        let mut client_info = JsonObject::default();
        client_info.set("name".into(), env!("CARGO_PKG_NAME").into(), &mut self.json);
        client_info.set(
            "version".into(),
            env!("CARGO_PKG_VERSION").into(),
            &mut self.json,
        );
        params.set("clientInfo".into(), client_info.into(), &mut self.json);

        let root = self
            .json
            .fmt_string(format_args!("{}", Uri::Path(&self.root)));
        params.set("rootUri".into(), root.into(), &mut self.json);

        params.set(
            "capabilities".into(),
            capabilities::client_capabilities(&mut self.json),
            &mut self.json,
        );

        self.initialized = true;
        self.request(platform, "initialize", params);
        self.initialized = false;
    }
}

pub(crate) mod util {
    use super::*;

    pub fn write_response_error(status_bar: &mut StatusBar, error: ResponseError, json: &Json) {
        status_bar
            .write(MessageKind::Error)
            .str(error.message.as_str(json));
    }

    pub fn text_document_with_id(root: &Path, path: &Path, json: &mut Json) -> JsonObject {
        let uri = if path.is_absolute() {
            json.fmt_string(format_args!("{}", Uri::Path(path)))
        } else {
            match path.to_str() {
                Some(path) => json.fmt_string(format_args!("{}/{}", Uri::Path(root), path)),
                None => return JsonObject::default(),
            }
        };
        let mut id = JsonObject::default();
        id.set("uri".into(), uri.into(), json);
        id
    }

    pub fn extract_markup_content(content: JsonValue, json: &Json) -> &str {
        match content {
            JsonValue::String(s) => s.as_str(json),
            JsonValue::Object(o) => match o.get("value", json) {
                JsonValue::String(s) => s.as_str(json),
                _ => "",
            },
            _ => "",
        }
    }

    pub fn create_definition_params(
        client: &mut Client,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
        buffer_position: BufferPosition,
    ) -> JsonObject {
        util::send_pending_did_change(client, editor, platform);

        let buffer = editor.buffers.get(buffer_handle);
        let text_document =
            util::text_document_with_id(&client.root, &buffer.path, &mut client.json);
        let position = DocumentPosition::from_buffer_position(buffer_position);

        let mut params = JsonObject::default();
        params.set(
            "textDocument".into(),
            text_document.into(),
            &mut client.json,
        );
        params.set(
            "position".into(),
            position.to_json_value(&mut client.json),
            &mut client.json,
        );

        params
    }

    pub fn send_did_open(
        client: &mut Client,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
    ) {
        if !client.server_capabilities.text_document_sync.open_close {
            return;
        }

        let buffer = editor.buffers.get(buffer_handle);
        if !buffer.properties.can_save {
            return;
        }

        let mut text_document = text_document_with_id(&client.root, &buffer.path, &mut client.json);
        let language_id = client
            .json
            .create_string(protocol::path_to_language_id(&buffer.path));
        text_document.set("languageId".into(), language_id.into(), &mut client.json);
        text_document.set("version".into(), JsonValue::Integer(1), &mut client.json);
        let text = client.json.fmt_string(format_args!("{}", buffer.content()));
        text_document.set("text".into(), text.into(), &mut client.json);

        let mut params = JsonObject::default();
        params.set(
            "textDocument".into(),
            text_document.into(),
            &mut client.json,
        );

        client.notify(platform, "textDocument/didOpen", params);
    }

    pub fn send_pending_did_change(client: &mut Client, editor: &Editor, platform: &mut Platform) {
        let mut versioned_buffers = std::mem::take(&mut client.versioned_buffers);
        for (buffer_handle, versioned_buffer) in versioned_buffers.iter_pending_mut() {
            if versioned_buffer.pending_edits.is_empty() {
                continue;
            }
            let buffer = editor.buffers.get(buffer_handle);
            if !buffer.properties.can_save {
                versioned_buffer.flush();
                continue;
            }

            let mut text_document =
                text_document_with_id(&client.root, &buffer.path, &mut client.json);
            text_document.set(
                "version".into(),
                JsonValue::Integer(versioned_buffer.version as _),
                &mut client.json,
            );

            let mut params = JsonObject::default();
            params.set(
                "textDocument".into(),
                text_document.into(),
                &mut client.json,
            );

            let mut content_changes = JsonArray::default();
            match client.server_capabilities.text_document_sync.change {
                TextDocumentSyncKind::None => (),
                TextDocumentSyncKind::Full => {
                    let text = client.json.fmt_string(format_args!("{}", buffer.content()));
                    let mut change_event = JsonObject::default();
                    change_event.set("text".into(), text.into(), &mut client.json);
                    content_changes.push(change_event.into(), &mut client.json);
                }
                TextDocumentSyncKind::Incremental => {
                    for edit in &versioned_buffer.pending_edits {
                        let mut change_event = JsonObject::default();

                        let edit_range = DocumentRange::from_buffer_range(edit.buffer_range)
                            .to_json_value(&mut client.json);
                        change_event.set("range".into(), edit_range, &mut client.json);

                        let edit_text_range =
                            edit.text_range.start as usize..edit.text_range.end as usize;
                        let text = &versioned_buffer.texts[edit_text_range];
                        let text = client.json.create_string(text);
                        change_event.set("text".into(), text.into(), &mut client.json);

                        content_changes.push(change_event.into(), &mut client.json);
                    }
                }
            }

            params.set(
                "contentChanges".into(),
                content_changes.into(),
                &mut client.json,
            );

            versioned_buffer.flush();
            client.notify(platform, "textDocument/didChange", params);
        }
        std::mem::swap(&mut client.versioned_buffers, &mut versioned_buffers);
    }

    pub fn send_did_save(
        client: &mut Client,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
    ) {
        if let TextDocumentSyncKind::None = client.server_capabilities.text_document_sync.save {
            return;
        }

        let buffer = editor.buffers.get(buffer_handle);
        if !buffer.properties.can_save {
            return;
        }

        let text_document = text_document_with_id(&client.root, &buffer.path, &mut client.json);
        let mut params = JsonObject::default();
        params.set(
            "textDocument".into(),
            text_document.into(),
            &mut client.json,
        );

        if let TextDocumentSyncKind::Full = client.server_capabilities.text_document_sync.save {
            let text = client.json.fmt_string(format_args!("{}", buffer.content()));
            params.set("text".into(), text.into(), &mut client.json);
        }

        client.notify(platform, "textDocument/didSave", params)
    }

    pub fn send_did_close(
        client: &mut Client,
        editor: &Editor,
        platform: &mut Platform,
        buffer_handle: BufferHandle,
    ) {
        if !client.server_capabilities.text_document_sync.open_close {
            return;
        }

        let buffer = editor.buffers.get(buffer_handle);
        if !buffer.properties.can_save {
            return;
        }

        let text_document = text_document_with_id(&client.root, &buffer.path, &mut client.json);
        let mut params = JsonObject::default();
        params.set(
            "textDocument".into(),
            text_document.into(),
            &mut client.json,
        );

        client.notify(platform, "textDocument/didClose", params);
    }
}
