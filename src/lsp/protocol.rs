use std::{
    io::{self, Cursor, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{mpsc, Arc, Mutex, MutexGuard},
    thread,
};

use crate::json::{Json, JsonInteger, JsonKey, JsonObject, JsonString, JsonValue};

pub struct ServerMessage {
    pub json: Arc<Mutex<Json>>,
    pub body: JsonValue,
}

pub struct ServerConnection {
    process: Child,
    stdin: ChildStdin,
    reader_handle: thread::JoinHandle<()>,
}

impl ServerConnection {
    pub fn spawn(
        mut command: Command,
        message_receiver: mpsc::Sender<ServerMessage>,
    ) -> io::Result<Self> {
        let mut process = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = process
            .stdin
            .take()
            .ok_or(io::Error::from(io::ErrorKind::UnexpectedEof))?;
        let stdout = process
            .stdout
            .take()
            .ok_or(io::Error::from(io::ErrorKind::WriteZero))?;

        let reader_handle = thread::spawn(move || {
            let mut stdout = stdout;
            let mut buf = ReadBuf::new();
            let json = Arc::new(Mutex::new(Json::new()));
            loop {
                let content_bytes = match buf.read_content_from(&mut stdout) {
                    Ok(bytes) => bytes,
                    Err(_) => break,
                };
                let mut json_guard = json.lock().unwrap();
                let mut reader = Cursor::new(content_bytes);
                let body = match json_guard.read(&mut reader) {
                    Ok(body) => body,
                    Err(_) => break,
                };
                let message = ServerMessage {
                    json: json.clone(),
                    body,
                };
                if let Err(_) = message_receiver.send(message) {
                    break;
                }
            }
        });

        Ok(Self {
            process,
            stdin,
            reader_handle,
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

pub struct ResponseError {
    pub code: JsonInteger,
    pub message: JsonKey,
    pub data: JsonValue,
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
    ) -> io::Result<()> {
        let mut body = JsonObject::new();
        body.set("jsonrpc".into(), "2.0".into(), json);
        body.set(
            "id".into(),
            JsonValue::Integer(self.next_request_id as _),
            json,
        );
        body.set("method".into(), method.into(), json);
        body.set("params".into(), params, json);

        self.next_request_id += 1;
        self.send_body(json, body.into())
    }

    pub fn notify(
        &mut self,
        json: &mut Json,
        method: &'static str,
        params: JsonValue,
    ) -> io::Result<()> {
        let mut body = JsonObject::new();
        body.set("jsonrpc".into(), "2.0".into(), json);
        body.set("method".into(), method.into(), json);
        body.set("params".into(), params, json);

        self.send_body(json, body.into())
    }

    pub fn respond(
        &mut self,
        json: &mut Json,
        request_id: usize,
        result: Result<JsonValue, ResponseError>,
    ) -> io::Result<()> {
        let mut body = JsonObject::new();
        body.set("id".into(), JsonValue::Integer(request_id as _), json);

        match result {
            Ok(result) => body.set("result".into(), result, json),
            Err(error) => {
                let mut e = JsonObject::new();
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

        {
            let msg = std::str::from_utf8(&self.write_buffer).unwrap();
            println!("msg:\n{}", msg);
        }

        self.server_connection.write(&self.write_buffer)?;
        Ok(())
    }
}

struct ReadBuf {
    buf: Vec<u8>,
    len: usize,
}

impl ReadBuf {
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(2 * 1024);
        buf.resize(buf.capacity(), 0);
        Self { buf, len: 0 }
    }

    pub fn read_content_from<R>(&mut self, mut reader: R) -> io::Result<&[u8]>
    where
        R: Read,
    {
        fn find_end<'a>(buf: &'a [u8], pattern: &[u8]) -> Option<usize> {
            buf.windows(pattern.len())
                .position(|w| w == pattern)
                .map(|p| p + pattern.len())
        }

        self.len = 0;
        let mut content_index = 0;
        let mut total_len = 0;
        loop {
            match reader.read(&mut self.buf[self.len..]) {
                Ok(len) => {
                    self.len += len;

                    if total_len == 0 {
                        let bytes = &self.buf[..self.len];
                        if let Some(cl_index) = find_end(bytes, b"Content-Length: ") {
                            let bytes = &bytes[cl_index..];
                            if let Some(c_index) = find_end(bytes, b"\r\n\r\n") {
                                let mut content_len = 0;
                                for b in bytes {
                                    if b.is_ascii_digit() {
                                        content_len *= 10;
                                        content_len += (b - b'0') as usize;
                                    } else {
                                        break;
                                    }
                                }

                                content_index = c_index;
                                total_len = cl_index + c_index + content_len;
                            }
                        }
                    }

                    if self.len >= total_len {
                        break;
                    }

                    self.buf.resize(self.buf.len() * 2, 0);
                }
                Err(e) => return Err(e),
            }
        }

        Ok(&self.buf[content_index..self.len])
    }
}
