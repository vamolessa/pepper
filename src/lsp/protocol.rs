use std::{
    convert::From,
    fmt, io,
    ops::Range,
    path::{Component, Path, Prefix},
};

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    client,
    json::{
        FromJson, Json, JsonConvertError, JsonInteger, JsonKey, JsonObject, JsonString, JsonValue,
    },
    platform::{Platform, PlatformRequest, ProcessHandle},
};

pub const BUFFER_LEN: usize = 4 * 1024;

pub enum Uri<'a> {
    AbsolutePath(&'a Path),
    RelativePath(&'a Path, &'a Path),
}
impl<'a> Uri<'a> {
    pub fn parse(base: &'a Path, uri: &'a str) -> Option<Self> {
        let uri = uri.strip_prefix("file:///")?;
        let path = Path::new(uri);
        match path.strip_prefix(base) {
            Ok(path) => Some(Uri::RelativePath(base, path)),
            Err(_) => Some(Uri::AbsolutePath(path)),
        }
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
            Self::AbsolutePath(path) => {
                f.write_str("file:///")?;
                fmt_path(f, path)
            }
            Self::RelativePath(base, path) => {
                f.write_str("file:///")?;
                fmt_path(f, base)?;
                f.write_str("/")?;
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
            JsonValue::Object(object) => object,
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

#[derive(Default)]
pub struct DocumentPosition {
    pub line: u32,
    pub character: u32,
}
impl DocumentPosition {
    pub fn to_json_value(&self, json: &mut Json) -> JsonValue {
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
impl From<BufferPosition> for DocumentPosition {
    fn from(position: BufferPosition) -> Self {
        Self {
            line: position.line_index as _,
            character: position.column_byte_index as _,
        }
    }
}
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
            JsonValue::Object(object) => object,
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

#[derive(Default)]
pub struct DocumentRange {
    pub start: DocumentPosition,
    pub end: DocumentPosition,
}
impl DocumentRange {
    pub fn to_json_value(&self, json: &mut Json) -> JsonValue {
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
            JsonValue::Object(object) => object,
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
            JsonValue::Object(object) => object,
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

pub struct PendingRequest {
    pub id: RequestId,
    pub method: &'static str,
    pub requesting_client: Option<client::ClientHandle>,
}

#[derive(Default)]
pub struct PendingRequestColection {
    pending_requests: Vec<PendingRequest>,
}

impl PendingRequestColection {
    pub fn add(
        &mut self,
        id: RequestId,
        method: &'static str,
        requesting_client: Option<client::ClientHandle>,
    ) {
        for request in &mut self.pending_requests {
            if request.id.0 == 0 {
                request.id = id;
                request.method = method;
                return;
            }
        }

        self.pending_requests.push(PendingRequest {
            id,
            method,
            requesting_client,
        })
    }

    pub fn take(&mut self, id: RequestId) -> Option<PendingRequest> {
        for i in 0..self.pending_requests.len() {
            let request = &self.pending_requests[i];
            if request.id == id {
                let request = self.pending_requests.swap_remove(i);
                return Some(request);
            }
        }
        None
    }
}

/*
struct ReadBuf {
    buf: Vec<u8>,
    read_index: usize,
    write_index: usize,
}

impl ReadBuf {
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(BUFFER_LEN);
        buf.resize(buf.capacity(), 0);
        Self {
            buf,
            read_index: 0,
            write_index: 0,
        }
    }

    pub fn read_content_from<R>(&mut self, mut reader: R) -> &[u8]
    where
        R: Read,
    {
        fn find_pattern_end<'a>(buf: &'a [u8], pattern: &[u8]) -> Option<usize> {
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

        let mut content_start_index = 0;
        let mut content_end_index = 0;

        loop {
            if content_end_index == 0 {
                let bytes = &self.buf[self.read_index..self.write_index];
                if let Some(cl_index) = find_pattern_end(bytes, b"Content-Length: ") {
                    let bytes = &bytes[cl_index..];
                    if let Some(c_index) = find_pattern_end(bytes, b"\r\n\r\n") {
                        let content_len = parse_number(bytes);
                        content_start_index = self.read_index + cl_index + c_index;
                        content_end_index = content_start_index + content_len;
                    }
                }
            }

            if content_end_index > 0 && self.write_index >= content_end_index {
                break;
            }

            if self.read_index > self.buf.len() / 2 {
                self.buf.copy_within(self.read_index..self.write_index, 0);
                if content_end_index > 0 {
                    content_start_index -= self.read_index;
                    content_end_index -= self.read_index;
                }
                self.write_index -= self.read_index;
                self.read_index = 0;
            } else {
                while self.write_index == self.buf.len() || content_end_index > self.buf.len() {
                    self.buf.resize(self.buf.len() * 2, 0);
                }

                match reader.read(&mut self.buf[self.write_index..]) {
                    Ok(0) | Err(_) => {
                        self.read_index = 0;
                        self.write_index = 0;
                        return &[];
                    }
                    Ok(len) => self.write_index += len,
                }
            }
        }

        self.read_index = content_end_index;

        if self.write_index == self.read_index {
            self.read_index = 0;
            self.write_index = 0;
        }

        &self.buf[content_start_index..content_end_index]
    }
}
*/
