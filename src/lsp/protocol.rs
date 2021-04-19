use std::{
    convert::From,
    fmt, fs, io,
    ops::Range,
    path::{Component, Path, Prefix},
};

use crate::{
    buffer::{BufferCapabilities, BufferHandle},
    buffer_position::{BufferPosition, BufferRange},
    editor::Editor,
    editor_utils::MessageKind,
    json::{
        FromJson, Json, JsonArray, JsonConvertError, JsonInteger, JsonKey, JsonObject, JsonString,
        JsonValue,
    },
    platform::{Platform, PlatformRequest, ProcessHandle},
};

pub const BUFFER_LEN: usize = 4 * 1024;

pub enum Uri<'a> {
    Path(&'a Path),
}
impl<'a> Uri<'a> {
    pub fn parse(root: &'a Path, uri: &'a str) -> Option<Self> {
        let uri = uri.strip_prefix("file:///")?;
        let path = Path::new(uri);
        let path = path.strip_prefix(root).unwrap_or(path);
        Some(Self::Path(path))
    }
}
impl<'a> fmt::Display for Uri<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn fmt_path(f: &mut fmt::Formatter, path: &Path) -> fmt::Result {
            let mut components = path.components().peekable();
            let mut has_prefix = false;
            while let Some(component) = components.next() {
                match component {
                    Component::Prefix(prefix) => match prefix.kind() {
                        Prefix::Verbatim(p) => match p.to_str() {
                            Some(p) => {
                                f.write_str(p)?;
                                has_prefix = true;
                            }
                            None => return Err(fmt::Error),
                        },
                        Prefix::VerbatimDisk(d) | Prefix::Disk(d) => {
                            f.write_fmt(format_args!("{}:", d as char))?;
                            has_prefix = true;
                        }
                        _ => continue,
                    },
                    Component::RootDir => {
                        if has_prefix {
                            continue;
                        }
                    }
                    Component::CurDir => f.write_str(".")?,
                    Component::ParentDir => f.write_str("..")?,
                    Component::Normal(component) => match component.to_str() {
                        Some(component) => f.write_str(component)?,
                        None => return Err(fmt::Error),
                    },
                }
                if let None = components.peek() {
                    break;
                }
                f.write_str("/")?;
            }
            Ok(())
        }

        match *self {
            Self::Path(path) => {
                f.write_str("file:///")?;
                fmt_path(f, path)
            }
        }
    }
}

pub fn path_to_language_id(path: &Path) -> &str {
    let extension = match path.extension().and_then(|e| e.to_str()) {
        Some(extension) => extension,
        None => return "",
    };

    let mut buf = [0; 8];
    let extension_len = extension.len();
    if extension_len > buf.len() {
        return extension;
    }

    for (bb, eb) in buf.iter_mut().zip(extension.bytes()) {
        *bb = eb.to_ascii_lowercase();
    }
    let extension_lowercase = &buf[..extension_len];

    match extension_lowercase {
        b"abap" => "abap",
        b"bat" | b"cmd" => "bat",
        b"bib" => "bibtex",
        b"clj" | b"cljs" | b"cljc" | b"edn" => "closure",
        b"coffee" | b"litcoffee" => "coffeescript",
        b"c" | b"h" => "c",
        b"cc" | b"cpp" | b"cxx" | b"c++" | b"hh" | b"hpp" | b"hxx" | b"h++" => "cpp",
        b"cs" | b"csx" => "csharp",
        b"css" => "css",
        b"diff" => "diff",
        b"dart" => "dart",
        b"dockerfile" => "dockerfile",
        b"ex" | b"exs" => "elixir",
        b"erl" | b"hrl" => "erlang",
        b"fs" | b"fsi" | b"fsx" | b"fsscript" => "fsharp",
        b"go" => "go",
        b"groovy" | b"gvy" | b"gy" | b"gsh" => "groovy",
        b"html" | b"htm" => "html",
        b"ini" => "ini",
        b"java" => "java",
        b"js" | b"mjs" => "javascript",
        b"json" => "json",
        b"less" => "less",
        b"lua" => "lua",
        b"md" => "markdown",
        b"m" => "objective-c",
        b"mm" => "objective-cpp",
        b"plx" | b"pl" | b"pm" | b"xs" | b"t" | b"pod" => "perl",
        b"php" | b"phtml" | b"php3" | b"php4" | b"php5" | b"php7" | b"phps" | b"php-s" | b"pht"
        | b"phar" => "php",
        b"ps1" | b"ps1xml" | b"psc1" | b"psd1" | b"psm1" | b"pssc" | b"psrc" | b"cdxml" => {
            "powershell"
        }
        b"py" | b"pyi" | b"pyc" | b"pyd" | b"pyo" | b"pyw" | b"pyz" => "python",
        b"r" | b"rdata" | b"rds" | b"rda" => "r",
        b"razor" | b"cshtml" | b"vbhtml" => "razor",
        b"rb" => "ruby",
        b"rs" => "rust",
        b"scss" => "scss",
        b"sass" => "sass",
        b"scala" | b"sc" => "scala",
        b"sh" => "shellscript",
        b"sql" => "sql",
        b"swift" => "swift",
        b"ts" | b"tsx" => "typescript",
        b"tex" => "tex",
        b"vb" => "vb",
        b"xml" => "xml",
        b"yaml" | b"yml" => "yaml",
        _ => extension,
    }
}

pub enum ServerEvent {
    Closed,
    ParseError,
    Request(ServerRequest),
    Notification(ServerNotification),
    Response(ServerResponse),
}

pub struct ServerRequest {
    pub id: JsonValue,
    pub method: JsonString,
    pub params: JsonValue,
}

pub struct ServerNotification {
    pub method: JsonString,
    pub params: JsonValue,
}

pub struct ServerResponse {
    pub id: RequestId,
    pub result: Result<JsonValue, ResponseError>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(pub usize);
impl From<RequestId> for JsonValue {
    fn from(id: RequestId) -> JsonValue {
        JsonValue::Integer(id.0 as _)
    }
}

#[derive(Default)]
pub struct ResponseError {
    pub code: JsonInteger,
    pub message: JsonKey,
    pub data: JsonValue,
}
impl ResponseError {
    pub fn parse_error() -> Self {
        Self {
            code: -32700,
            message: JsonKey::Str("ParseError"),
            data: JsonValue::Null,
        }
    }

    pub fn method_not_found() -> Self {
        Self {
            code: -32601,
            message: JsonKey::Str("MethodNotFound"),
            data: JsonValue::Null,
        }
    }
}
impl<'json> FromJson<'json> for ResponseError {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "code" => this.code = FromJson::from_json(value, json)?,
                "message" => this.message = FromJson::from_json(value, json)?,
                "data" => this.data = FromJson::from_json(value, json)?,
                _ => return Err(JsonConvertError),
            }
        }
        Ok(this)
    }
}

#[derive(Default, Clone, Copy)]
pub struct DocumentPosition {
    pub line: u32,
    pub character: u32,
}
impl DocumentPosition {
    pub fn to_json_value(self, json: &mut Json) -> JsonValue {
        let mut value = JsonObject::default();
        value.set("line".into(), JsonValue::Integer(self.line as _), json);
        value.set(
            "character".into(),
            JsonValue::Integer(self.character as _),
            json,
        );
        value.into()
    }
}
// TODO: handle utf8 to utf16
impl From<BufferPosition> for DocumentPosition {
    fn from(position: BufferPosition) -> Self {
        Self {
            line: position.line_index as _,
            character: position.column_byte_index as _,
        }
    }
}
// TODO: handle utf16 to utf8
impl From<DocumentPosition> for BufferPosition {
    fn from(position: DocumentPosition) -> Self {
        Self {
            line_index: position.line as _,
            column_byte_index: position.character as _,
        }
    }
}
impl<'json> FromJson<'json> for DocumentPosition {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "line" => this.line = FromJson::from_json(value, json)?,
                "character" => this.character = FromJson::from_json(value, json)?,
                _ => return Err(JsonConvertError),
            }
        }
        Ok(this)
    }
}

#[derive(Default, Clone, Copy)]
pub struct DocumentRange {
    pub start: DocumentPosition,
    pub end: DocumentPosition,
}
impl DocumentRange {
    pub fn to_json_value(self, json: &mut Json) -> JsonValue {
        let mut value = JsonObject::default();
        value.set("start".into(), self.start.to_json_value(json), json);
        value.set("end".into(), self.end.to_json_value(json), json);
        value.into()
    }
}
impl From<BufferRange> for DocumentRange {
    fn from(range: BufferRange) -> Self {
        Self {
            start: range.from.into(),
            end: range.to.into(),
        }
    }
}
impl From<DocumentRange> for BufferRange {
    fn from(range: DocumentRange) -> Self {
        BufferRange::between(range.start.into(), range.end.into())
    }
}
impl<'json> FromJson<'json> for DocumentRange {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "start" => this.start = FromJson::from_json(value, json)?,
                "end" => this.end = FromJson::from_json(value, json)?,
                _ => return Err(JsonConvertError),
            }
        }
        Ok(this)
    }
}

#[derive(Default)]
pub struct DocumentLocation {
    pub uri: JsonString,
    pub range: DocumentRange,
}
impl<'json> FromJson<'json> for DocumentLocation {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "uri" => this.uri = FromJson::from_json(value, json)?,
                "range" => this.range = FromJson::from_json(value, json)?,
                _ => return Err(JsonConvertError),
            }
        }
        Ok(this)
    }
}

#[derive(Default)]
pub struct TextEdit {
    pub range: DocumentRange,
    pub new_text: JsonString,
}
impl TextEdit {
    pub fn apply_edits(
        editor: &mut Editor,
        buffer_handle: BufferHandle,
        temp_edits: &mut Vec<(BufferRange, BufferRange)>,
        edits: JsonArray,
        json: &Json,
    ) {
        let buffer = match editor.buffers.get_mut(buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        buffer.commit_edits();
        temp_edits.clear();
        for edit in edits.elements(json) {
            let edit = match TextEdit::from_json(edit, json) {
                Ok(edit) => edit,
                Err(_) => continue,
            };

            let mut delete_range: BufferRange = edit.range.into();
            let text = edit.new_text.as_str(&json);

            for (d, i) in temp_edits.iter() {
                delete_range.from = delete_range.from.delete(*d);
                delete_range.to = delete_range.to.delete(*d);

                delete_range.from = delete_range.from.insert(*i);
                delete_range.to = delete_range.to.insert(*i);
            }

            buffer.delete_range(&mut editor.word_database, delete_range, &mut editor.events);
            let insert_range = buffer.insert_text(
                &mut editor.word_database,
                delete_range.from,
                text,
                &mut editor.events,
            );

            temp_edits.push((delete_range, insert_range));
        }
        buffer.commit_edits();
    }
}
impl<'json> FromJson<'json> for TextEdit {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "range" => this.range = FromJson::from_json(value, json)?,
                "newText" => this.new_text = FromJson::from_json(value, json)?,
                _ => return Err(JsonConvertError),
            }
        }
        Ok(this)
    }
}

#[derive(Default)]
pub struct DocumentEdit {
    pub uri: JsonString,
    pub edits: JsonArray,
}
impl<'json> FromJson<'json> for DocumentEdit {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "textDocument" => this.uri = JsonString::from_json(value.get("uri", json), json)?,
                "edits" => this.edits = JsonArray::from_json(value, json)?,
                _ => (),
            }
        }
        Ok(this)
    }
}

#[derive(Default)]
pub struct CreateFileOperation {
    pub uri: JsonString,
    pub overwrite: bool,
    pub ignore_if_exists: bool,
}
impl<'json> FromJson<'json> for CreateFileOperation {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "uri" => this.uri = JsonString::from_json(value, json)?,
                "options" => {
                    for (key, value) in value.members(json) {
                        match key {
                            "overwrite" => {
                                this.overwrite = matches!(value, JsonValue::Boolean(true))
                            }
                            "ignoreIfExists" => {
                                this.ignore_if_exists = matches!(value, JsonValue::Boolean(true))
                            }
                            _ => (),
                        }
                    }
                }
                _ => (),
            }
        }
        Ok(this)
    }
}

#[derive(Default)]
pub struct RenameFileOperation {
    pub old_uri: JsonString,
    pub new_uri: JsonString,
    pub overwrite: bool,
    pub ignore_if_exists: bool,
}
impl<'json> FromJson<'json> for RenameFileOperation {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "oldUri" => this.old_uri = JsonString::from_json(value, json)?,
                "newUri" => this.new_uri = JsonString::from_json(value, json)?,
                "options" => {
                    for (key, value) in value.members(json) {
                        match key {
                            "overwrite" => {
                                this.overwrite = matches!(value, JsonValue::Boolean(true))
                            }
                            "ignoreIfExists" => {
                                this.ignore_if_exists = matches!(value, JsonValue::Boolean(true))
                            }
                            _ => (),
                        }
                    }
                }
                _ => (),
            }
        }
        Ok(this)
    }
}

#[derive(Default)]
pub struct DeleteFileOperation {
    pub uri: JsonString,
    pub recursive: bool,
    pub ignore_if_not_exists: bool,
}
impl<'json> FromJson<'json> for DeleteFileOperation {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "uri" => this.uri = JsonString::from_json(value, json)?,
                "options" => {
                    for (key, value) in value.members(json) {
                        match key {
                            "recursive" => {
                                this.recursive = matches!(value, JsonValue::Boolean(true))
                            }
                            "ignoreIfNotExists" => {
                                this.ignore_if_not_exists =
                                    matches!(value, JsonValue::Boolean(true))
                            }
                            _ => (),
                        }
                    }
                }
                _ => (),
            }
        }
        Ok(this)
    }
}

pub enum WorkspaceEditChange {
    DocumentEdit(DocumentEdit),
    CreateFile(CreateFileOperation),
    RenameFile(RenameFileOperation),
    DeleteFile(DeleteFileOperation),
}
impl<'json> FromJson<'json> for WorkspaceEditChange {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let this = match value.clone().get("kind", json) {
            JsonValue::String(s) => match s.as_str(json) {
                "create" => Self::CreateFile(FromJson::from_json(value, json)?),
                "rename" => Self::RenameFile(FromJson::from_json(value, json)?),
                "delete" => Self::DeleteFile(FromJson::from_json(value, json)?),
                _ => return Err(JsonConvertError),
            },
            _ => Self::DocumentEdit(FromJson::from_json(value, json)?),
        };
        Ok(this)
    }
}

#[derive(Default)]
pub struct WorkspaceEdit {
    document_changes: JsonArray,
}
impl WorkspaceEdit {
    pub fn apply(
        &self,
        editor: &mut Editor,
        temp_edits: &mut Vec<(BufferRange, BufferRange)>,
        root: &Path,
        json: &Json,
    ) {
        for change in self.document_changes.clone().elements(json) {
            let change = match WorkspaceEditChange::from_json(change, json) {
                Ok(change) => change,
                Err(_) => return,
            };
            match change {
                WorkspaceEditChange::DocumentEdit(edit) => {
                    let path = match Uri::parse(&root, edit.uri.as_str(json)) {
                        Some(Uri::Path(path)) => path,
                        None => return,
                    };
                    let buffer_handle = editor
                        .buffers
                        .find_with_path(&editor.current_directory, path);

                    let (is_temp, buffer_handle) = match buffer_handle {
                        Some(handle) => (false, handle),
                        None => {
                            let buffer = editor.buffers.add_new();
                            buffer.set_path(path);
                            buffer.capabilities = BufferCapabilities::log();
                            buffer.capabilities.can_save = true;
                            (true, buffer.handle())
                        }
                    };

                    TextEdit::apply_edits(editor, buffer_handle, temp_edits, edit.edits, json);

                    if is_temp {
                        if let Some(buffer) = editor.buffers.get_mut(buffer_handle) {
                            let _ = buffer.save_to_file(None, &mut editor.events);
                        }

                        editor
                            .buffers
                            .defer_remove(buffer_handle, &mut editor.events);
                    }
                }
                WorkspaceEditChange::CreateFile(op) => {
                    let path = match Uri::parse(&root, op.uri.as_str(json)) {
                        Some(Uri::Path(path)) => path,
                        None => return,
                    };

                    let mut open_options = fs::OpenOptions::new();
                    open_options.write(true);
                    if op.overwrite {
                        open_options.truncate(true).create(true);
                    } else {
                        open_options.create_new(true);
                    }
                    if open_options.open(path).is_err() && !op.ignore_if_exists {
                        editor
                            .status_bar
                            .write(MessageKind::Error)
                            .fmt(format_args!("could not create file {:?}", path));
                    }
                }
                WorkspaceEditChange::RenameFile(op) => {
                    let old_path = match Uri::parse(&root, op.old_uri.as_str(json)) {
                        Some(Uri::Path(path)) => path,
                        None => return,
                    };
                    let new_path = match Uri::parse(&root, op.new_uri.as_str(json)) {
                        Some(Uri::Path(path)) => path,
                        None => return,
                    };

                    if op.overwrite || !new_path.exists() || !op.ignore_if_exists {
                        if fs::rename(old_path, new_path).is_err() && !op.ignore_if_exists {}
                    }
                }
                WorkspaceEditChange::DeleteFile(op) => {
                    let path = match Uri::parse(&root, op.uri.as_str(json)) {
                        Some(Uri::Path(path)) => path,
                        None => return,
                    };

                    if op.recursive {
                        if fs::remove_dir_all(path).is_err() && !op.ignore_if_not_exists {
                            editor
                                .status_bar
                                .write(MessageKind::Error)
                                .fmt(format_args!("could not delete directory {:?}", path));
                        }
                    } else {
                        if fs::remove_file(path).is_err() && !op.ignore_if_not_exists {
                            editor
                                .status_bar
                                .write(MessageKind::Error)
                                .fmt(format_args!("could not delete file {:?}", path));
                        }
                    }
                }
            }
        }
    }
}
impl<'json> FromJson<'json> for WorkspaceEdit {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        let changes = value.get("documentChanges", json);
        this.document_changes = FromJson::from_json(changes, json)?;
        Ok(this)
    }
}

#[derive(Default)]
pub struct DocumentDiagnostic {
    pub message: JsonString,
    pub range: DocumentRange,
    pub data: JsonValue,
}
impl DocumentDiagnostic {
    pub fn to_json_value(self, json: &mut Json) -> JsonValue {
        let mut value = JsonObject::default();
        value.set("message".into(), self.message.into(), json);
        value.set("range".into(), self.range.to_json_value(json), json);
        value.set("data".into(), self.data, json);
        value.into()
    }
}
impl<'json> FromJson<'json> for DocumentDiagnostic {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "message" => this.message = JsonString::from_json(value, json)?,
                "range" => this.range = DocumentRange::from_json(value, json)?,
                "data" => this.data = value,
                _ => (),
            }
        }
        Ok(this)
    }
}

#[derive(Default)]
pub struct DocumentCodeAction {
    pub title: JsonString,
    pub edit: WorkspaceEdit,
    pub disabled: bool,
}
impl<'json> FromJson<'json> for DocumentCodeAction {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "title" => this.title = JsonString::from_json(value, json)?,
                "edit" => this.edit = WorkspaceEdit::from_json(value, json)?,
                "disabled" => this.disabled = true,
                _ => (),
            }
        }
        Ok(this)
    }
}

#[derive(Default)]
pub struct DocumentSymbolInformation {
    pub name: JsonString,
    pub location: DocumentLocation,
    pub container_name: Option<JsonString>,
}
impl<'json> FromJson<'json> for DocumentSymbolInformation {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "name" => this.name = JsonString::from_json(value, json)?,
                "location" => this.location = DocumentLocation::from_json(value, json)?,
                "containerName" => this.container_name = FromJson::from_json(value, json)?,
                _ => (),
            }
        }
        Ok(this)
    }
}

#[derive(Default)]
pub struct DocumentCompletionItem {
    pub text: JsonString,
}
impl<'json> FromJson<'json> for DocumentCompletionItem {
    fn from_json(value: JsonValue, json: &'json Json) -> Result<Self, JsonConvertError> {
        let value = match value {
            JsonValue::Object(value) => value,
            _ => return Err(JsonConvertError),
        };
        let mut this = Self::default();
        for (key, value) in value.members(json) {
            match key {
                "label" => this.text = JsonString::from_json(value, json)?,
                "insertText" => this.text = JsonString::from_json(value, json)?,
                _ => (),
            }
        }
        Ok(this)
    }
}

fn try_get_content_range(buf: &[u8]) -> Option<Range<usize>> {
    fn find_pattern_end(buf: &[u8], pattern: &[u8]) -> Option<usize> {
        let len = pattern.len();
        buf.windows(len).position(|w| w == pattern).map(|p| p + len)
    }

    fn parse_number(buf: &[u8]) -> usize {
        let mut n = 0;
        for b in buf {
            if b.is_ascii_digit() {
                n *= 10;
                n += (b - b'0') as usize;
            } else {
                break;
            }
        }
        n
    }

    let content_length_index = find_pattern_end(buf, b"Content-Length: ")?;
    let buf = &buf[content_length_index..];
    let content_index = find_pattern_end(buf, b"\r\n\r\n")?;
    let content_len = parse_number(buf);
    let buf = &buf[content_index..];

    if buf.len() >= content_len {
        let start = content_length_index + content_index;
        let end = start + content_len;
        Some(start..end)
    } else {
        None
    }
}

fn parse_server_event(json: &Json, body: JsonValue) -> ServerEvent {
    declare_json_object! {
        struct Body {
            id: JsonValue,
            method: JsonValue,
            params: JsonValue,
            result: JsonValue,
            error: Option<ResponseError>,
        }
    }

    let body = match Body::from_json(body, json) {
        Ok(body) => body,
        Err(_) => return ServerEvent::ParseError,
    };

    if let JsonValue::String(method) = body.method {
        match body.id {
            JsonValue::Integer(_) | JsonValue::String(_) => ServerEvent::Request(ServerRequest {
                id: body.id,
                method,
                params: body.params,
            }),
            JsonValue::Null => ServerEvent::Notification(ServerNotification {
                method,
                params: body.params,
            }),
            _ => return ServerEvent::ParseError,
        }
    } else if let Some(error) = body.error {
        let id = match body.id {
            JsonValue::Integer(n) if n > 0 => n as _,
            _ => return ServerEvent::ParseError,
        };
        ServerEvent::Response(ServerResponse {
            id: RequestId(id),
            result: Err(error),
        })
    } else {
        let id = match body.id {
            JsonValue::Integer(n) if n > 0 => n as _,
            _ => return ServerEvent::ParseError,
        };
        ServerEvent::Response(ServerResponse {
            id: RequestId(id),
            result: Ok(body.result),
        })
    }
}

pub struct ServerEventIter {
    read_len: usize,
}
impl ServerEventIter {
    pub fn next(&mut self, protocol: &mut Protocol, json: &mut Json) -> Option<ServerEvent> {
        let slice = &protocol.read_buf[self.read_len..];
        if slice.is_empty() {
            return None;
        }

        let range = try_get_content_range(slice)?;
        self.read_len += range.end;
        let mut reader = io::Cursor::new(&slice[range]);
        let event = match json.read(&mut reader) {
            Ok(body) => parse_server_event(json, body),
            _ => ServerEvent::ParseError,
        };
        Some(event)
    }

    pub fn finish(&self, protocol: &mut Protocol) {
        protocol.read_buf.drain(..self.read_len);
    }
}

pub struct Protocol {
    process_handle: Option<ProcessHandle>,
    body_buf: Vec<u8>,
    read_buf: Vec<u8>,
    next_request_id: usize,
}

impl Protocol {
    pub fn new() -> Self {
        Self {
            process_handle: None,
            body_buf: Vec::new(),
            read_buf: Vec::new(),
            next_request_id: 1,
        }
    }

    pub fn set_process_handle(&mut self, handle: ProcessHandle) {
        self.process_handle = Some(handle);
    }

    pub fn parse_events(&mut self, bytes: &[u8]) -> ServerEventIter {
        self.read_buf.extend_from_slice(bytes);
        ServerEventIter { read_len: 0 }
    }

    pub fn request(
        &mut self,
        platform: &mut Platform,
        json: &mut Json,
        method: &'static str,
        params: JsonValue,
    ) -> RequestId {
        let id = self.next_request_id;

        let mut body = JsonObject::default();
        body.set("jsonrpc".into(), "2.0".into(), json);
        body.set("id".into(), JsonValue::Integer(id as _), json);
        body.set("method".into(), method.into(), json);
        body.set("params".into(), params, json);

        self.next_request_id += 1;
        self.send_body(platform, json, body.into());

        RequestId(id)
    }

    pub fn notify(
        &mut self,
        platform: &mut Platform,
        json: &mut Json,
        method: &'static str,
        params: JsonValue,
    ) {
        let mut body = JsonObject::default();
        body.set("jsonrpc".into(), "2.0".into(), json);
        body.set("method".into(), method.into(), json);
        body.set("params".into(), params, json);

        self.send_body(platform, json, body.into());
    }

    pub fn respond(
        &mut self,
        platform: &mut Platform,
        json: &mut Json,
        request_id: JsonValue,
        result: Result<JsonValue, ResponseError>,
    ) {
        let mut body = JsonObject::default();
        body.set("id".into(), request_id, json);

        match result {
            Ok(result) => body.set("result".into(), result, json),
            Err(error) => {
                let mut e = JsonObject::default();
                e.set("code".into(), error.code.into(), json);
                e.set("message".into(), error.message.into(), json);
                e.set("data".into(), error.data, json);

                body.set("error".into(), e.into(), json);
            }
        }

        self.send_body(platform, json, body.into());
    }

    fn send_body(&mut self, platform: &mut Platform, json: &mut Json, body: JsonValue) {
        use io::Write;

        let mut buf = platform.buf_pool.acquire();
        let write = buf.write();

        json.write(&mut self.body_buf, &body);
        let _ = write!(write, "Content-Length: {}\r\n\r\n", self.body_buf.len());
        write.append(&mut self.body_buf);

        if let Some(handle) = self.process_handle {
            let buf = buf.share();
            platform.enqueue_request(PlatformRequest::WriteToProcess { handle, buf });
        }
    }
}

struct PendingRequest {
    id: RequestId,
    method: &'static str,
}

#[derive(Default)]
pub struct PendingRequestColection {
    pending_requests: Vec<PendingRequest>,
}

impl PendingRequestColection {
    pub fn add(&mut self, id: RequestId, method: &'static str) {
        for request in &mut self.pending_requests {
            if request.id.0 == 0 {
                request.id = id;
                request.method = method;
                return;
            }
        }

        self.pending_requests.push(PendingRequest { id, method });
    }

    pub fn take(&mut self, id: RequestId) -> Option<&'static str> {
        for i in 0..self.pending_requests.len() {
            let request = &self.pending_requests[i];
            if request.id == id {
                let request = self.pending_requests.swap_remove(i);
                return Some(request.method);
            }
        }
        None
    }
}
