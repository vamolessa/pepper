use std::{fmt, convert::From, env, fs, io, sync::mpsc, thread};

use crate::{
    client::{Client, ClientResponse},
    client_event::{ClientEvent, ClientEventSerializer},
    connection::{ConnectionWithClientCollection, ConnectionWithServer, TargetClient},
    editor::{Editor, EditorLoop},
    editor_operation::{
        EditorOperationDeserializeResult, EditorOperationDeserializer, EditorOperationSerializer,
        StatusMessageKind,
    },
    event_manager::{ConnectionEvent, EventManager},
};

pub trait UiError: 'static + Send + fmt::Debug {}

#[derive(Debug)]
pub enum ApplicationError<UIE>
where
    UIE: UiError,
{
    IO(io::Error),
    UI(UIE),
    CouldNotConnectToOrStartServer,
}

impl<UIE> From<io::Error> for ApplicationError<UIE>
where
    UIE: UiError,
{
    fn from(error: io::Error) -> Self {
        ApplicationError::IO(error)
    }
}

impl<UIE> From<UIE> for ApplicationError<UIE>
where
    UIE: UiError,
{
    fn from(error: UIE) -> Self {
        ApplicationError::UI(error)
    }
}

pub trait UI {
    type Error: UiError;

    fn run_event_loop_in_background(
        event_sender: mpsc::Sender<ClientEvent>,
    ) -> thread::JoinHandle<Result<(), Self::Error>>;

    fn init(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn resize(&mut self, _width: u16, _height: u16) -> Result<(), Self::Error> {
        Ok(())
    }

    fn draw(
        &mut self,
        client: &Client,
        status_message_kind: StatusMessageKind,
        status_message: &str,
    ) -> Result<(), Self::Error>;

    fn shutdown(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub fn run<I>(ui: I) -> Result<(), ApplicationError<I::Error>>
where
    I: UI,
{
    let session_socket_path = env::current_dir()?.join("session_socket");
    if let Ok(connection) = ConnectionWithServer::connect(&session_socket_path) {
        run_client(ui, connection)?;
    } else if let Ok(listener) = ConnectionWithClientCollection::listen(&session_socket_path) {
        run_server_with_client(ui, listener)?;
        fs::remove_file(session_socket_path)?;
    } else if let Ok(()) = fs::remove_file(&session_socket_path) {
        let listener = ConnectionWithClientCollection::listen(&session_socket_path)?;
        run_server_with_client(ui, listener)?;
        fs::remove_file(session_socket_path)?;
    } else {
        return Err(ApplicationError::CouldNotConnectToOrStartServer);
    }

    Ok(())
}

fn send_operations<I>(
    operations: &mut EditorOperationSerializer,
    local_client: &mut Client,
    remote_clients: &mut ConnectionWithClientCollection,
    ui: &mut I,
) -> ClientResponse
where
    I: UI,
{
    for handle in remote_clients.all_handles() {
        remote_clients.send_serialized_operations(handle, &operations);
    }

    let mut deserializer = EditorOperationDeserializer::from_slice(operations.local_bytes());
    let mut response = ClientResponse::None;
    while let EditorOperationDeserializeResult::Some(op) = deserializer.deserialize_next() {
        response = response.or(local_client.on_editor_operation(&op, ui));
    }

    operations.clear();
    response
}

fn run_server_with_client<I>(
    mut ui: I,
    mut connections: ConnectionWithClientCollection,
) -> Result<(), ApplicationError<I::Error>>
where
    I: UI,
{
    let (event_sender, event_receiver) = mpsc::channel();
    let event_manager = EventManager::new()?;
    let event_registry = event_manager.registry();
    let event_manager_loop = event_manager.run_event_loop_in_background(event_sender.clone());
    let ui_event_loop = I::run_event_loop_in_background(event_sender);

    let mut local_client = Client::new();
    let mut editor = Editor::new();
    let mut editor_operations = EditorOperationSerializer::default();

    local_client.load_config(
        &editor.commands,
        &mut editor.keymaps,
        &mut editor_operations,
        &mut ui,
    );

    connections.register_listener(&event_registry)?;
    ui.init()?;

    'main_loop: for event in event_receiver.iter() {
        match event {
            ClientEvent::None => (),
            ClientEvent::Key(key) => {
                let mut result = editor.on_key(
                    &local_client.config,
                    key,
                    TargetClient::Local,
                    &mut editor_operations,
                );

                loop {
                    match result {
                        EditorLoop::Quit => break 'main_loop,
                        EditorLoop::Continue => {
                            send_operations(
                                &mut editor_operations,
                                &mut local_client,
                                &mut connections,
                                &mut ui,
                            );
                            break;
                        }
                        EditorLoop::WaitForSpawnOutputOnClient => {
                            match send_operations(
                                &mut editor_operations,
                                &mut local_client,
                                &mut connections,
                                &mut ui,
                            ) {
                                ClientResponse::None => break,
                                ClientResponse::SpawnResult(spawn_result) => {
                                    result = editor.on_spawn_result(
                                        &local_client.config,
                                        spawn_result,
                                        TargetClient::Local,
                                        &mut editor_operations,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            ClientEvent::Resize(w, h) => ui.resize(w, h)?,
            ClientEvent::Connection(event) => {
                match event {
                    ConnectionEvent::NewConnection => {
                        let handle = connections.accept_connection(&event_registry)?;
                        editor.on_client_joined(
                            handle,
                            &local_client.config,
                            &mut editor_operations,
                        );
                        connections.listen_next_listener_event(&event_registry)?;
                    }
                    ConnectionEvent::Stream(stream_id) => {
                        let handle = stream_id.into();

                        let mut result = connections.receive_keys(handle, |key| {
                            editor.on_key(
                                &local_client.config,
                                key,
                                TargetClient::Remote(handle),
                                &mut editor_operations,
                            )
                        });

                        loop {
                            match result {
                                Ok(EditorLoop::Quit) | Err(_) => {
                                    connections.close_connection(handle);
                                    editor.on_client_left(handle, &mut editor_operations);
                                    break;
                                }
                                Ok(EditorLoop::Continue) => {
                                    connections
                                        .listen_next_connection_event(handle, &event_registry)?;
                                    break;
                                }
                                Ok(EditorLoop::WaitForSpawnOutputOnClient) => {
                                    connections
                                        .listen_next_connection_event(handle, &event_registry)?;
                                    let spawn_result =
                                        match connections.receive_spawn_result(handle)? {
                                            Some(spawn_result) => spawn_result,
                                            None => break,
                                        };

                                    result = Ok(editor.on_spawn_result(
                                        &local_client.config,
                                        spawn_result,
                                        TargetClient::Remote(handle),
                                        &mut editor_operations,
                                    ));
                                    connections
                                        .listen_next_connection_event(handle, &event_registry)?;
                                }
                            }
                        }
                    }
                }

                connections.unregister_closed_connections(&event_registry)?;
                send_operations(
                    &mut editor_operations,
                    &mut local_client,
                    &mut connections,
                    &mut ui,
                );
                connections.unregister_closed_connections(&event_registry)?;
            }
        }

        ui.draw(
            &local_client,
            local_client.status_message_kind,
            &local_client.status_message[..],
        )?;
        local_client.status_message.clear();
    }

    drop(event_manager_loop);
    drop(ui_event_loop);

    connections.close_all_connections();
    ui.shutdown()?;
    Ok(())
}

fn run_client<I>(
    mut ui: I,
    mut connection: ConnectionWithServer,
) -> Result<(), ApplicationError<I::Error>>
where
    I: UI,
{
    let (event_sender, event_receiver) = mpsc::channel();
    let event_manager = EventManager::new()?;
    let event_registry = event_manager.registry();
    let event_manager_loop = event_manager.run_event_loop_in_background(event_sender.clone());
    let ui_event_loop = I::run_event_loop_in_background(event_sender);

    let mut local_client = Client::new();
    let mut events = ClientEventSerializer::default();

    connection.register_connection(&event_registry)?;
    ui.init()?;

    for event in event_receiver.iter() {
        match event {
            ClientEvent::None => (),
            ClientEvent::Key(key) => {
                events.serialize(key);
                if connection.send_serialized_events(&mut events).is_err() {
                    break;
                }
            }
            ClientEvent::Resize(w, h) => ui.resize(w, h)?,
            ClientEvent::Connection(event) => match event {
                ConnectionEvent::NewConnection => (),
                ConnectionEvent::Stream(_) => {
                    let response = connection
                        .receive_operations(|op| local_client.on_editor_operation(&op, &mut ui))?;
                    match response {
                        Some(ClientResponse::None) => (),
                        Some(ClientResponse::SpawnResult(spawn_result)) => {
                            events.serialize(spawn_result);
                            if connection.send_serialized_events(&mut events).is_err() {
                                break;
                            }
                        }
                        None => break,
                    }

                    connection.listen_next_event(&event_registry)?;
                }
            },
        }

        ui.draw(
            &local_client,
            local_client.status_message_kind,
            &local_client.status_message[..],
        )?;
        local_client.status_message.clear();
    }

    drop(event_manager_loop);
    drop(ui_event_loop);

    connection.close();
    ui.shutdown()?;
    Ok(())
}
