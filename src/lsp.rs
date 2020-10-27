use std::{env, io, process};

use crate::json::{JsonObject, JsonValue};

mod capabilities;
mod protocol;

use protocol::{Protocol, ServerConnection};

pub struct Client {
    protocol: Protocol,
}

impl Client {
    pub fn new(server_executable: &str) -> io::Result<Self> {
        let server_command = process::Command::new(server_executable);
        let connection = ServerConnection::spawn(server_command)?;
        Ok(Self {
            protocol: Protocol::new(connection),
        })
    }

    pub fn initialize(&mut self) -> io::Result<()> {
        let json = &mut self.protocol.json;

        let current_dir = match env::current_dir()?.as_os_str().to_str() {
            Some(path) => json.create_string(path).into(),
            None => JsonValue::Null,
        };

        let mut params = JsonObject::new();
        params.push(
            "processId".into(),
            JsonValue::Integer(process::id() as _),
            json,
        );
        params.push("rootUri".into(), current_dir, json);
        params.push(
            "capabilities".into(),
            capabilities::client_capabilities(json),
            json,
        );

        self.protocol.request("initialize", params.into())
    }

    pub fn wait_response(&mut self) -> io::Result<&str> {
        self.protocol.wait_response()
    }
}
