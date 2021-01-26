use std::{
    fmt,
    io::{self, Cursor, Read, Write},
    path::{Component, Path, Prefix},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{mpsc, Arc, Mutex, MutexGuard},
    thread,
};

use crate::{
    client_event::LocalEvent,
    json::{
        FromJson, Json, JsonConvertError, JsonInteger, JsonKey, JsonObject, JsonString, JsonValue,
    },
    lsp::client::ClientHandle,
};

pub struct SharedJsonGuard {
    json: Json,
    pending_consume_count: usize,
}
impl SharedJsonGuard {
    pub fn get(&mut self) -> &mut Json {
        &mut self.json
    }
}
#[derive(Clone)]
pub struct SharedJson(Arc<Mutex<SharedJsonGuard>>);
impl SharedJson {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(SharedJsonGuard {
            json: Json::new(),
            pending_consume_count: 0,
        })))
    }

    fn parse_lock(&self) -> MutexGuard<SharedJsonGuard> {
        let mut json = self.0.lock().unwrap();
        if json.pending_consume_count == 0 {
            json.json.clear();
        }
        json.pending_consume_count += 1;
        json
    }

    pub fn read_lock(&mut self) -> MutexGuard<SharedJsonGuard> {
        let mut json = self.0.lock().unwrap();
        json.pending_consume_count -= 1;
        json
    }

    pub fn write_lock(&mut self) -> MutexGuard<SharedJsonGuard> {
        self.0.lock().unwrap()
    }
}

pub enum Uri<'a> {
    None,
    AbsolutePath(&'a Path),
    RelativePath(&'a Path, &'a Path),
}
impl<'a> Uri<'a> {
    pub fn parse(uri: &'a str) -> Self {
        const FILE_SCHEME_PREFIX: &str = "file:///";
        if uri.starts_with(FILE_SCHEME_PREFIX) {
            Uri::AbsolutePath(Path::new(&uri[FILE_SCHEME_PREFIX.len()..]))
        } else {
            Uri::None
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
            Self::None => Ok(()),
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

pub struct ServerConnection {
    process: Child,
    stdin: ChildStdin,
}

impl ServerConnection {
    pub fn spawn(
        mut command: Command,
        handle: ClientHandle,
        json: SharedJson,
        //event_sender: mpsc::Sender<LocalEvent>,
    ) -> io::Result<Self> {
        let mut process = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = process
            .stdin
            .take()
            .ok_or(io::Error::from(io::ErrorKind::WriteZero))?;
        let stdout = process
            .stdout
            .take()
            .ok_or(io::Error::from(io::ErrorKind::UnexpectedEof))?;

        thread::spawn(move || {
            let mut stdout = stdout;
            let mut buf = ReadBuf::new();

            loop {
                let content_bytes = match buf.read_content_from(&mut stdout) {
                    [] => break,
                    bytes => bytes,
                };

                let mut reader = Cursor::new(content_bytes);
                let mut json = json.parse_lock();
                let json = json.get();
                let event = match json.read(&mut reader) {
                    Ok(body) => parse_server_event(&json, body),
                    _ => ServerEvent::ParseError,
                };
                /*
                if let Err(_) = event_sender.send(LocalEvent::Lsp(handle, event)) {
                    break;
                }
                */
            }
            //let _ = event_sender.send(LocalEvent::Lsp(handle, ServerEvent::Closed));
        });

        Ok(Self { process, stdin })
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

impl Write for ServerConnection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stdin.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stdin.flush()
    }
}

impl Drop for ServerConnection {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(pub usize);

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
        let mut this = Self {
            code: JsonInteger::default(),
            message: JsonKey::default(),
            data: JsonValue::Null,
        };
        for (key, value) in value.members(json) {
            match key {
                "code" => this.code = FromJson::from_json(value, json)?,
                "message" => this.message = FromJson::from_json(value, json)?,
                "data" => this.data = FromJson::from_json(value, json)?,
                _ => (),
            }
        }
        Ok(this)
    }
}

pub struct Protocol {
    server_connection: ServerConnection,
    body_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
    next_request_id: usize,
}

impl Protocol {
    pub fn new(server_connection: ServerConnection) -> Self {
        Self {
            server_connection,
            body_buffer: Vec::new(),
            write_buffer: Vec::new(),
            next_request_id: 1,
        }
    }

    pub fn request(
        &mut self,
        json: &mut Json,
        method: &'static str,
        params: JsonValue,
    ) -> io::Result<RequestId> {
        let id = self.next_request_id;

        let mut body = JsonObject::default();
        body.set("jsonrpc".into(), "2.0".into(), json);
        body.set("id".into(), JsonValue::Integer(id as _), json);
        body.set("method".into(), method.into(), json);
        body.set("params".into(), params, json);

        self.next_request_id += 1;
        self.send_body(json, body.into())?;

        Ok(RequestId(id))
    }

    pub fn notify(
        &mut self,
        json: &mut Json,
        method: &'static str,
        params: JsonValue,
    ) -> io::Result<()> {
        let mut body = JsonObject::default();
        body.set("jsonrpc".into(), "2.0".into(), json);
        body.set("method".into(), method.into(), json);
        body.set("params".into(), params, json);

        self.send_body(json, body.into())
    }

    pub fn respond(
        &mut self,
        json: &mut Json,
        request_id: JsonValue,
        result: Result<JsonValue, ResponseError>,
    ) -> io::Result<()> {
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

        self.send_body(json, body.into())
    }

    fn send_body(&mut self, json: &mut Json, body: JsonValue) -> io::Result<()> {
        json.write(&mut self.body_buffer, &body)?;

        self.write_buffer.clear();
        write!(
            self.write_buffer,
            "Content-Length: {}\r\n\r\n",
            self.body_buffer.len()
        )?;
        self.write_buffer.append(&mut self.body_buffer);

        self.server_connection.write(&self.write_buffer)?;
        Ok(())
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

        self.pending_requests.push(PendingRequest { id, method })
    }

    pub fn take(&mut self, id: RequestId) -> Option<&'static str> {
        for i in 0..self.pending_requests.len() {
            let request = &self.pending_requests[i];
            if request.id == id {
                let method = request.method;
                self.pending_requests.swap_remove(i);
                return Some(method);
            }
        }
        None
    }
}

struct ReadBuf {
    buf: Vec<u8>,
    read_index: usize,
    write_index: usize,
}

impl ReadBuf {
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(4 * 1024);
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
