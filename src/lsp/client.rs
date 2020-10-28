use std::{
    env, io,
    process::{self, Command},
    sync::{mpsc, Arc, Mutex},
};

use crate::{
    client_event::LocalEvent,
    json::{Json, JsonObject, JsonValue},
    lsp::{
        capabilities,
        protocol::{Protocol, ServerConnection},
    },
};

pub struct Client {
    protocol: Protocol,
    json: Arc<Mutex<Json>>,
}

impl Client {
    pub fn initialize(&mut self) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();

        let current_dir = match env::current_dir()?.as_os_str().to_str() {
            Some(path) => json.create_string(path).into(),
            None => JsonValue::Null,
        };

        let mut params = JsonObject::new();
        params.set(
            "processId".into(),
            JsonValue::Integer(process::id() as _),
            &mut json,
        );
        params.set("rootUri".into(), current_dir, &mut json);
        params.set(
            "capabilities".into(),
            capabilities::client_capabilities(&mut json),
            &mut json,
        );

        self.protocol
            .request(&mut json, "initialize", params.into())?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClientHandle(usize);

pub struct ClientCollection {
    clients: Vec<Option<Client>>,
}

impl ClientCollection {
    pub fn spawn(
        &mut self,
        command: Command,
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> io::Result<ClientHandle> {
        let handle = self.find_free_slot();
        let json = Arc::new(Mutex::new(Json::new()));
        let connection = ServerConnection::spawn(command, handle, json.clone(), event_sender)?;
        self.clients[handle.0] = Some(Client {
            protocol: Protocol::new(connection),
            json,
        });
        Ok(handle)
    }

    pub fn close(&mut self, handle: ClientHandle) {
        self.clients[handle.0] = None;
    }

    fn find_free_slot(&mut self) -> ClientHandle {
        for (i, slot) in self.clients.iter_mut().enumerate() {
            if slot.is_none() {
                return ClientHandle(i);
            }
        }
        let handle = ClientHandle(self.clients.len());
        self.clients.push(None);
        handle
    }
}
