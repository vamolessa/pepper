use std::{env, io, process, sync::mpsc};

use crate::json::{Json, JsonObject, JsonValue};

mod capabilities;
mod protocol;

use protocol::{Protocol, ServerConnection, ServerMessage};

pub struct Client {
    protocol: Protocol,
}

impl Client {
    pub fn new(
        server_executable: &str,
        message_receiver: mpsc::Sender<ServerMessage>,
    ) -> io::Result<Self> {
        let server_command = process::Command::new(server_executable);
        let connection = ServerConnection::spawn(server_command, message_receiver)?;
        Ok(Self {
            protocol: Protocol::new(connection),
        })
    }

    pub fn initialize(&mut self, json: &mut Json) -> io::Result<()> {
        let current_dir = match env::current_dir()?.as_os_str().to_str() {
            Some(path) => json.create_string(path).into(),
            None => JsonValue::Null,
        };

        let mut params = JsonObject::new();
        params.set(
            "processId".into(),
            JsonValue::Integer(process::id() as _),
            json,
        );
        params.set("rootUri".into(), current_dir, json);
        params.set(
            "capabilities".into(),
            capabilities::client_capabilities(json),
            json,
        );

        self.protocol.request(json, "initialize", params.into())
    }
}
