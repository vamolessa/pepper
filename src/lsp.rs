use std::{
    io::{self, Cursor, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use crate::json::{Json, JsonObject, JsonValue};

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

    pub fn guard(&mut self) -> ReadGuard {
        ReadGuard(self)
    }
}

struct ReadGuard<'a>(&'a mut ReadBuf);

impl<'a> ReadGuard<'a> {
    pub fn read_from<R>(&mut self, mut reader: R) -> io::Result<()>
    where
        R: Read,
    {
        fn find_end<'a>(buf: &'a [u8], pattern: &[u8]) -> Option<usize> {
            buf.windows(pattern.len())
                .position(|w| w == pattern)
                .map(|p| p + pattern.len())
        }

        let mut total_len = 0;
        loop {
            match reader.read(&mut self.0.buf[self.0.len..]) {
                Ok(len) => {
                    self.0.len += len;

                    if total_len == 0 {
                        let bytes = &self.0.buf[..self.0.len];
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

                    if self.0.len >= total_len {
                        break;
                    }

                    self.0.buf.resize(self.0.buf.len() * 2, 0);
                }
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    pub fn as_bytes(&'a self) -> &'a [u8] {
        &self.0.buf[..self.0.len]
    }
}

impl<'a> Drop for ReadGuard<'a> {
    fn drop(&mut self) {
        self.0.len = 0;
    }
}

pub struct Client {
    pub json: Json,
    json_buffer: Vec<u8>,

    server_connection: ServerConnection,
    write_buffer: Vec<u8>,
    read_buffer: ReadBuf,
}

impl Client {
    pub fn from_server_connection(server_connection: ServerConnection) -> Self {
        Self {
            json: Json::new(),
            json_buffer: Vec::new(),
            server_connection,
            write_buffer: Vec::new(),
            read_buffer: ReadBuf::new(),
        }
    }

    pub fn request(&mut self, method: &str, params: &JsonValue) -> io::Result<()> {
        write!(
            self.json_buffer,
            r#"{{"jsonrpc":"2.0","id":{},"method":"{}","params":"#,
            1, method
        )?;
        self.json.write(&mut self.json_buffer, params)?;
        self.json_buffer.push(b'}');

        self.write_buffer.clear();
        write!(
            self.write_buffer,
            "Content-Length: {}\r\n\r\n",
            self.json_buffer.len()
        )?;
        self.write_buffer.append(&mut self.json_buffer);

        let msg = std::str::from_utf8(&self.write_buffer).unwrap();
        println!("msg:\n{}", msg);

        self.server_connection.write(&self.write_buffer)?;
        Ok(())
    }

    pub fn wait_response<F>(&mut self, on_read: F) -> io::Result<()>
    where
        F: FnOnce(&str),
    {
        let mut reader = self.read_buffer.guard();
        reader.read_from(&mut self.server_connection)?;
        let s = std::str::from_utf8(reader.as_bytes()).unwrap();
        on_read(s);
        Ok(())
    }
}
