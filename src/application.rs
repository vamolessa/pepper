use std::{convert::From, env, fmt, fs, io, path::PathBuf, sync::mpsc, thread};

use argh::FromArgs;

use crate::{
    client::Client,
    client_event::{ClientEvent, ClientEventSerializer},
    connection::{ConnectionWithClientCollection, ConnectionWithServer, TargetClient},
    editor::{Editor, EditorLoop},
    editor_operation::{
        EditorOperationDeserializeResult, EditorOperationDeserializer, EditorOperationSerializer,
        StatusMessageKind,
    },
    event_manager::{ConnectionEvent, EventManager},
};

/// code editor inspired by vim and kakoune
#[derive(FromArgs)]
struct Args {
    /// path where config file is located
    #[argh(option, short = 'c')]
    config: Option<PathBuf>,

    /// session name
    #[argh(option, short = 's')]
    session: Option<String>,
}

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
    let args: Args = argh::from_env();

    let mut session_socket_path = env::temp_dir();
    session_socket_path.push(env!("CARGO_PKG_NAME"));
    if !session_socket_path.exists() {
        std::fs::create_dir_all(&session_socket_path)?;
    }

    let mut session_name = match args.session.clone() {
        Some(session) => session,
        None => env::current_dir()?.to_string_lossy().into_owned(),
    };
    session_name.retain(|c| c.is_alphanumeric());
    session_socket_path.push(session_name);

    if let Ok(connection) = ConnectionWithServer::connect(&session_socket_path) {
        run_client(ui, connection)?;
    } else if let Ok(listener) = ConnectionWithClientCollection::listen(&session_socket_path) {
        run_server_with_client(args, ui, listener)?;
        fs::remove_file(session_socket_path)?;
    } else if let Ok(()) = fs::remove_file(&session_socket_path) {
        let listener = ConnectionWithClientCollection::listen(&session_socket_path)?;
        run_server_with_client(args, ui, listener)?;
        fs::remove_file(session_socket_path)?;
    } else {
        return Err(ApplicationError::CouldNotConnectToOrStartServer);
    }

    Ok(())
}

fn send_operations(
    operations: &mut EditorOperationSerializer,
    local_client: &mut Client,
    remote_clients: &mut ConnectionWithClientCollection,
) {
    for handle in remote_clients.all_handles() {
        remote_clients.send_serialized_operations(handle, &operations);
    }

    let mut deserializer = EditorOperationDeserializer::from_slice(operations.local_bytes());
    while let EditorOperationDeserializeResult::Some(op) = deserializer.deserialize_next() {
        local_client.on_editor_operation(&op);
    }

    operations.clear();
}

fn run_server_with_client<I>(
    args: Args,
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

    if let Some(config_path) = args.config {
        editor.load_config(&mut local_client, &mut editor_operations, &config_path);
    }

    connections.register_listener(&event_registry)?;
    ui.init()?;

    for event in event_receiver.iter() {
        match event {
            ClientEvent::None => (),
            ClientEvent::Key(key) => {
                let result = editor.on_key(
                    &local_client.config,
                    key,
                    TargetClient::Local,
                    &mut editor_operations,
                );

                match result {
                    EditorLoop::Quit => break,
                    EditorLoop::Continue => {
                        send_operations(&mut editor_operations, &mut local_client, &mut connections)
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

                        let result = connections.receive_keys(handle, |key| {
                            editor.on_key(
                                &local_client.config,
                                key,
                                TargetClient::Remote(handle),
                                &mut editor_operations,
                            )
                        });

                        match result {
                            Ok(EditorLoop::Quit) | Err(_) => {
                                connections.close_connection(handle);
                                editor.on_client_left(handle, &mut editor_operations);
                            }
                            Ok(EditorLoop::Continue) => {
                                connections
                                    .listen_next_connection_event(handle, &event_registry)?;
                            }
                        }
                    }
                }

                connections.unregister_closed_connections(&event_registry)?;
                send_operations(&mut editor_operations, &mut local_client, &mut connections);
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
                if let Err(_) = connection.send_serialized_events(&mut events) {
                    break;
                }
            }
            ClientEvent::Resize(w, h) => ui.resize(w, h)?,
            ClientEvent::Connection(event) => match event {
                ConnectionEvent::NewConnection => (),
                ConnectionEvent::Stream(_) => {
                    let response = connection
                        .receive_operations(|op| local_client.on_editor_operation(&op))?;
                    if let None = response {
                        break;
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
