use std::{
    env, io,
    process::{self, Command},
    sync::{mpsc, Arc, Mutex},
};

use crate::{
    client_event::LocalEvent,
    json::{Json, JsonKey, JsonObject, JsonValue},
    lsp::{
        capabilities,
        protocol::{
            Protocol, ResponseError, ServerConnection, ServerEvent, ServerNotification,
            ServerRequest, ServerResponse,
        },
    },
};

pub struct Client {
    protocol: Protocol,
    json: Arc<Mutex<Json>>,
}

impl Client {
    pub fn on_request(&mut self, request: ServerRequest) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();

        match request.method.as_str(&json) {
            _ => {
                let error = ResponseError::method_not_found();
                self.protocol.respond(&mut json, request.id, Err(error))
            }
        }
    }

    pub fn on_notification(&mut self, notification: ServerNotification) -> io::Result<()> {
        let json = self.json.lock().unwrap();

        match notification.method.as_str(&json) {
            "initialize" => {
                eprintln!("");
            }
            _ => (),
        }

        Ok(())
    }

    pub fn on_response(&mut self, response: ServerResponse) -> io::Result<()> {
        let json = self.json.lock().unwrap();
        Ok(())
    }

    pub fn on_parse_error(&mut self) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();
        let error = ResponseError::parse_error();
        self.protocol
            .respond(&mut json, JsonValue::Null, Err(error))
    }

    pub fn initialize(&mut self) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();

        let current_dir = match env::current_dir()?.as_os_str().to_str() {
            Some(path) => json.create_string(path).into(),
            None => JsonValue::Null,
        };

        let mut params = JsonObject::default();
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

    pub fn on_event(&mut self, event: ServerEvent) -> io::Result<()> {
        match event {
            ServerEvent::Closed(handle) => {
                self.clients[handle.0] = None;
            }
            ServerEvent::ParseError(handle) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_parse_error()?;
                }
            }
            ServerEvent::Request(handle, request) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_request(request)?;
                }
            }
            ServerEvent::Notification(handle, notification) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_notification(notification)?;
                }
            }
            ServerEvent::Response(handle, response) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_response(response)?;
                }
            }
        }
        Ok(())
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
