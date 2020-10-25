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

    pub fn send(&mut self, message: &[u8]) -> io::Result<()> {
        self.stdin.write(message)?;
        Ok(())
    }
}

impl Drop for ServerConnection {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}

pub struct Client {
    pub json: Json,
    server_connection: ServerConnection,
    message_buffer: Vec<u8>,
    json_buffer: Vec<u8>,
}

impl Client {
    pub fn from_server_connection(server_connection: ServerConnection) -> Self {
        Self {
            server_connection,
            message_buffer: Vec::new(),
            json: Json::new(),
            json_buffer: Vec::new(),
        }
    }

    pub fn send(&mut self, mut message: JsonObject) -> io::Result<()> {
        message.push("jsonrpc".into(), JsonValue::Str("2.0"), &mut self.json);
        message.push("id".into(), JsonValue::Integer(1), &mut self.json);

        let mut writer = Cursor::new(&mut self.json_buffer);
        let message = JsonValue::Object(message);
        self.json.write(&mut writer, &message)?;

        self.message_buffer.clear();
        write!(
            self.message_buffer,
            "Content-Length: {}\r\n\r\n",
            self.json_buffer.len()
        )?;
        self.message_buffer.append(&mut self.json_buffer);

        Ok(())
    }
}
