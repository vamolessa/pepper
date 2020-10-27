use std::{
    io::{self, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use crate::json::{Json, JsonInteger, JsonKey, JsonObject, JsonValue};

pub struct ServerConnection {
    process: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl ServerConnection {
    pub fn spawn(mut command: Command) -> io::Result<Self> {
        let mut process = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = process
            .stdin
            .take()
            .ok_or(io::Error::from(io::ErrorKind::UnexpectedEof))?;
        let stdout = process
            .stdout
            .take()
            .ok_or(io::Error::from(io::ErrorKind::WriteZero))?;
        Ok(Self {
            process,
            stdin,
            stdout,
        })
    }
}

impl Read for ServerConnection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stdout.read(buf)
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
    pub json: Json,
    json_buffer: Vec<u8>,

    server_connection: ServerConnection,
    write_buffer: Vec<u8>,
    read_buffer: ReadBuf,

    next_request_id: usize,
}

impl Protocol {
    pub fn new(server_connection: ServerConnection) -> Self {
        Self {
            json: Json::new(),
            json_buffer: Vec::new(),
            server_connection,
            write_buffer: Vec::new(),
            read_buffer: ReadBuf::new(),
            next_request_id: 1,
        }
    }

    pub fn request(&mut self, method: &'static str, params: JsonValue) -> io::Result<()> {
        let mut body = JsonObject::new();
        body.push("jsonrpc".into(), "2.0".into(), &mut self.json);
        body.push(
            "id".into(),
            JsonValue::Integer(self.next_request_id as _),
            &mut self.json,
        );
        body.push("method".into(), method.into(), &mut self.json);
        body.push("params".into(), params, &mut self.json);

        self.next_request_id += 1;
        self.send_body(body.into())
    }

    pub fn respond(
        &mut self,
        request_id: usize,
        result: Result<JsonValue, ResponseError>,
    ) -> io::Result<()> {
        let mut body = JsonObject::new();
        body.push(
            "id".into(),
            JsonValue::Integer(request_id as _),
            &mut self.json,
        );

        match result {
            Ok(result) => body.push("result".into(), result, &mut self.json),
            Err(error) => {
                let mut e = JsonObject::new();
                e.push("code".into(), error.code.into(), &mut self.json);
                e.push("message".into(), error.message.into(), &mut self.json);
                e.push("data".into(), error.data, &mut self.json);

                body.push("error".into(), e.into(), &mut self.json);
            }
        }

        self.send_body(body.into())
    }

    pub fn wait_response(&mut self) -> io::Result<&str> {
        let bytes = self.read_buffer.read_from(&mut self.server_connection)?;
        Ok(std::str::from_utf8(bytes).unwrap())
    }

    fn send_body(&mut self, body: JsonValue) -> io::Result<()> {
        self.json.write(&mut self.json_buffer, &body)?;

        self.write_buffer.clear();
        write!(
            self.write_buffer,
            "Content-Length: {}\r\n\r\n",
            self.json_buffer.len()
        )?;
        self.write_buffer.append(&mut self.json_buffer);

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

    pub fn read_from<R>(&mut self, mut reader: R) -> io::Result<&[u8]>
    where
        R: Read,
    {
        fn find_end<'a>(buf: &'a [u8], pattern: &[u8]) -> Option<usize> {
            buf.windows(pattern.len())
                .position(|w| w == pattern)
                .map(|p| p + pattern.len())
        }

        self.len = 0;
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

        Ok(&self.buf[..self.len])
    }
}
