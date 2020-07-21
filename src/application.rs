use std::{convert::From, env, fs, io, sync::mpsc, thread};

use crate::{
    client::Client,
    connection::{ConnectionWithClientCollection, ConnectionWithServer, TargetClient},
    editor::{Editor, EditorLoop, EditorOperation, EditorOperationSender},
    event::Event,
    event_manager::{run_event_loop, EventManager},
    mode::Mode,
};

pub trait UiError: 'static + Send {}

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

    fn run_event_loop(
        event_sender: mpsc::Sender<Event>,
    ) -> thread::JoinHandle<Result<(), Self::Error>>;

    fn init(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn resize(&mut self, _width: u16, _height: u16) -> Result<(), Self::Error> {
        Ok(())
    }

    fn draw(&mut self, client: &Client, error: Option<String>) -> Result<(), Self::Error>;

    fn shutdown(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn bind_keys(editor: &mut Editor) {
    editor
        .keymaps
        .parse_map(Mode::Normal.discriminant(), "qq", ":quit<c-m>")
        .unwrap();
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

fn run_server_with_client<I>(
    mut ui: I,
    connections: ConnectionWithClientCollection,
) -> Result<(), ApplicationError<I::Error>>
where
    I: UI,
{
    let (event_sender, event_receiver) = mpsc::channel();
    let event_manager = EventManager::new(event_sender.clone(), 8)?;
    let _ = run_event_loop(event_manager);
    let _ = I::run_event_loop(event_sender);

    let mut local_client = Client::new();
    let mut editor = Editor::new();
    bind_keys(&mut editor);

    let mut editor_operations = EditorOperationSender::new();

    ui.init()?;

    for event in event_receiver.iter() {
        match event {
            Event::Key(key) => {
                match editor.on_key(key, TargetClient::Local, &mut editor_operations) {
                    EditorLoop::Quit => break,
                    EditorLoop::Continue => (),
                    EditorLoop::Error(_e) => (),
                }
            }
            Event::Resize(w, h) => ui.resize(w, h)?,
            Event::Stream(id) => {
                // read from stream
            }
            _ => (),
        }
    }

    ui.shutdown()?;
    Ok(())
}

fn run_client<I>(ui: I, connection: ConnectionWithServer) -> Result<(), ApplicationError<I::Error>>
where
    I: UI,
{
    Ok(())
}

// =========================================================================

/*
async fn send_operations_async(
    operations: &mut EditorOperationSender,
    local_client: &mut Client,
    remote_clients: &mut ConnectionWithClientCollection,
) {
    let mut had_remote_operation = false;
    for (target_client, operation, content) in operations.drain() {
        match target_client {
            TargetClient::All => {
                local_client.on_editor_operation(&operation, content);
                remote_clients.queue_operation_all(&operation, content);
                had_remote_operation = true;
            }
            TargetClient::Local => {
                local_client.on_editor_operation(&operation, content);
            }
            TargetClient::Remote(handle) => {
                remote_clients.queue_operation(handle, &operation, content);
                had_remote_operation = true;
            }
        }
    }

    if had_remote_operation {
        remote_clients.send_queued_operations().await;
    }
}

pub async fn run_async<E, I>(event_stream: E, ui: I) -> Result<(), ApplicationError<I::Error>>
where
    E: FusedStream<Item = Event>,
    I: UI,
{
    let session_socket_path = env::current_dir()?.join("session_socket");
    if let Ok(connection) = ConnectionWithServer::connect(&session_socket_path) {
        run_client_async(event_stream, ui, connection).await?;
    } else if let Ok(listener) = ClientListener::listen(&session_socket_path) {
        run_server_with_client_async(event_stream, ui, listener).await?;
        fs::remove_file(session_socket_path)?;
    } else if let Ok(()) = fs::remove_file(&session_socket_path) {
        let listener = ClientListener::listen(&session_socket_path)?;
        run_server_with_client_async(event_stream, ui, listener).await?;
        fs::remove_file(session_socket_path)?;
    } else {
        return Err(ApplicationError::CouldNotConnectToOrStartServer);
    }

    Ok(())
}

async fn run_server_with_client_async<E, I>(
    event_stream: E,
    mut ui: I,
    listener: ClientListener,
) -> Result<(), ApplicationError<I::Error>>
where
    E: FusedStream<Item = Event>,
    I: UI,
{
    ui.init().map_err(|e| ApplicationError::UI(e))?;

    let mut local_client = Client::new();
    let mut editor = Editor::new();
    bind_keys(&mut editor);

    let mut client_connections = ConnectionWithClientCollection::new();
    let mut client_key_streams = ClientKeyStreams::new();
    let mut editor_operations = EditorOperationSender::new();

    let listen_future = listener.accept().fuse();
    pin_mut!(event_stream, listen_future);
    loop {
        let mut error = None;

        select_biased! {
            event = event_stream.select_next_some() => {
                match event {
                    Event::Key(key) => {
                        match editor.on_key(key, TargetClient::Local, &mut editor_operations) {
                            EditorLoop::Quit => break,
                            EditorLoop::Continue => (),
                            EditorLoop::Error(e) => error = Some(e),
                        }
                        send_operations_async(&mut editor_operations, &mut local_client, &mut client_connections).await;
                    },
                    Event::Resize(w, h) => ui.resize(w, h).map_err(|e| ApplicationError::UI(e))?,
                    _ => (),
                }
            },
            (handle, key) = client_key_streams.select_next_some() => {
                match editor.on_key(key, TargetClient::Remote(handle), &mut editor_operations) {
                    EditorLoop::Quit => {
                        client_connections.close(handle);
                        editor_operations.send(TargetClient::All, EditorOperation::InputKeep(0));
                        editor_operations.send(TargetClient::All, EditorOperation::Mode(Mode::default()));
                        editor.on_client_left(TargetClient::Remote(handle), &mut editor_operations);
                    }
                    EditorLoop::Continue => (),
                    EditorLoop::Error(e) => error = Some(e),
                }
                send_operations_async(&mut editor_operations, &mut local_client, &mut client_connections).await;
            }
            connection = listen_future => {
                listen_future.set(listener.accept().fuse());
                let (handle, key_reader) = client_connections.open(connection?);
                client_key_streams.push(ClientKeyStreams::from_reader(key_reader));
                editor.on_client_joined(TargetClient::Remote(handle), &mut editor_operations);
                send_operations_async(&mut editor_operations, &mut local_client, &mut client_connections).await;
            },
        }

        ui.draw(&local_client, error)
            .map_err(|e| ApplicationError::UI(e))?;
    }

    ui.shutdown().map_err(|e| ApplicationError::UI(e))?;
    Ok(())
}

async fn run_client_async<E, I>(
    event_stream: E,
    mut ui: I,
    connection: ConnectionWithServer,
) -> Result<(), ApplicationError<I::Error>>
where
    E: FusedStream<Item = Event>,
    I: UI,
{
    ui.init().map_err(|e| ApplicationError::UI(e))?;

    let mut local_client = Client::new();
    let (operation_reader, mut key_writer) = connection.split();
    let mut operation_stream = operation_reader.to_stream();

    pin_mut!(event_stream);
    loop {
        select_biased! {
            result = operation_stream.next() => {
                if let Some((operation, content)) = result {
                    local_client.on_editor_operation(&operation, &content[..]);
                } else {
                    break;
                }
            }
            event = event_stream.select_next_some() => {
                match event {
                    Event::Key(key) => key_writer.send(key).await?,
                    Event::Resize(w, h) => ui.resize(w, h).map_err(|e| ApplicationError::UI(e))?,
                    _ => (),
                }
            },
        }

        ui.draw(&local_client, None)
            .map_err(|e| ApplicationError::UI(e))?;
    }

    ui.shutdown().map_err(|e| ApplicationError::UI(e))?;
    Ok(())
}
*/
