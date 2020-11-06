use std::{
    env, io,
    process::{self, Command},
    sync::{mpsc, Arc, Mutex},
};

use crate::{
    buffer::BufferCollection,
    buffer_view::BufferViewCollection,
    client_event::LocalEvent,
    editor::{EditorEvent, StatusMessage},
    glob::Glob,
    json::{Json, JsonObject, JsonValue},
    lsp::{
        capabilities,
        protocol::{
            PendingRequestColection, Protocol, ResponseError, ServerConnection, ServerEvent,
            ServerNotification, ServerRequest, ServerResponse,
        },
    },
};

pub struct ClientContext<'a> {
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub status_message: &'a mut StatusMessage,
}

pub struct Client {
    name: String,
    protocol: Protocol,
    json: Arc<Mutex<Json>>,
    pending_requests: PendingRequestColection,

    initialized: bool,
    document_selectors: Vec<Glob>,
}

impl Client {
    fn new(name: String, connection: ServerConnection, json: Arc<Mutex<Json>>) -> Self {
        Self {
            name,
            protocol: Protocol::new(connection),
            json,
            pending_requests: PendingRequestColection::default(),

            initialized: false,
            document_selectors: Vec::new(),
        }
    }

    pub fn on_request(
        &mut self,
        ctx: &mut ClientContext,
        request: ServerRequest,
    ) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();

        match request.method.as_str(&json) {
            _ => {
                let error = ResponseError::method_not_found();
                self.protocol.respond(&mut json, request.id, Err(error))
            }
        }
    }

    pub fn on_notification(
        &mut self,
        ctx: &mut ClientContext,
        notification: ServerNotification,
    ) -> io::Result<()> {
        let json = self.json.lock().unwrap();

        match notification.method.as_str(&json) {
            _ => (),
        }

        Ok(())
    }

    pub fn on_response(
        &mut self,
        ctx: &mut ClientContext,
        response: ServerResponse,
    ) -> io::Result<()> {
        let idn = response.id.0;
        let method = match self.pending_requests.take(response.id) {
            Some(method) => method,
            None => {
                eprintln!("num acho request para a response {:?}", idn);
                return Ok(());
            }
        };
        let mut json = self.json.lock().unwrap();

        match method {
            "initialize" => {
                let mut bytes = Vec::new();
                match response.result {
                    Ok(result) => {
                        self.initialized = true;
                        self.protocol.notify(
                            &mut json,
                            "initialized",
                            JsonValue::Object(JsonObject::default()),
                        )?;
                        json.write(&mut bytes, &result)?;
                    }
                    Err(error) => json.write(&mut bytes, &error.message.into())?,
                }
                let text = String::from_utf8(bytes).unwrap();
                eprintln!("initialize response:\n{}\n---\n", text);
            }
            _ => (),
        }

        Ok(())
    }

    pub fn on_parse_error(&mut self) -> io::Result<()> {
        let mut json = self.json.lock().unwrap();
        let error = ResponseError::parse_error();
        self.protocol
            .respond(&mut json, JsonValue::Null, Err(error))
    }

    pub fn on_editor_events(
        &mut self,
        ctx: &mut ClientContext,
        events: &[EditorEvent],
    ) -> io::Result<()> {
        if !self.initialized {
            return Ok(());
        }

        for event in events {
            match event {
                _ => (),
            }
        }
        Ok(())
    }

    fn request(
        protocol: &mut Protocol,
        json: &mut Json,
        pending_requests: &mut PendingRequestColection,
        method: &'static str,
        params: JsonObject,
    ) -> io::Result<()> {
        let id = protocol.request(json, method, params.into())?;
        pending_requests.add(id, method);
        Ok(())
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

        Self::request(
            &mut self.protocol,
            &mut json,
            &mut self.pending_requests,
            "initialize",
            params,
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClientHandle(usize);

#[derive(Default)]
pub struct ClientCollection {
    clients: Vec<Option<Client>>,
}

impl ClientCollection {
    pub fn spawn(
        &mut self,
        name: &str,
        command: Command,
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> io::Result<ClientHandle> {
        for (handle, client) in self
            .clients
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.as_ref().map(|v| (ClientHandle(i), v)))
        {
            if client.name == name {
                return Ok(handle);
            }
        }

        let handle = self.find_free_slot();
        let json = Arc::new(Mutex::new(Json::new()));
        let connection = ServerConnection::spawn(command, handle, json.clone(), event_sender)?;
        self.clients[handle.0] = Some(Client::new(name.into(), connection, json));
        Ok(handle)
    }

    pub fn get(&mut self, handle: ClientHandle) -> Option<&mut Client> {
        self.clients[handle.0].as_mut()
    }

    pub fn on_server_event(
        &mut self,
        ctx: &mut ClientContext,
        handle: ClientHandle,
        event: ServerEvent,
    ) -> io::Result<()> {
        match event {
            ServerEvent::Closed => {
                self.clients[handle.0] = None;
            }
            ServerEvent::ParseError => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_parse_error()?;
                }
            }
            ServerEvent::Request(request) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_request(ctx, request)?;
                }
            }
            ServerEvent::Notification(notification) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_notification(ctx, notification)?;
                }
            }
            ServerEvent::Response(response) => {
                if let Some(client) = self.clients[handle.0].as_mut() {
                    client.on_response(ctx, response)?;
                }
            }
        }
        Ok(())
    }

    pub fn on_editor_events(
        &mut self,
        ctx: &mut ClientContext,
        events: &[EditorEvent],
    ) -> io::Result<()> {
        for client in self.clients.iter_mut().flatten() {
            client.on_editor_events(ctx, events)?;
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
