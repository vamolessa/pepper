use std::{
    convert::From,
    env, fs, io,
    sync::{mpsc, Arc, Barrier, Mutex},
    thread,
};

use crate::{
    client::Client,
    connection::{ConnectionWithClientCollection, ConnectionWithServer, TargetClient},
    editor::{Editor, EditorLoop, EditorOperationSender},
    event::Event,
    event_manager::{run_event_loop, ConnectionEvent, EventManager},
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

fn send_operations(
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
        remote_clients.send_queued_operations();
    }
}

fn run_server_with_client<I>(
    mut ui: I,
    mut connections: ConnectionWithClientCollection,
) -> Result<(), ApplicationError<I::Error>>
where
    I: UI,
{
    let (event_sender, event_receiver) = mpsc::channel();
    let mut event_manager = EventManager::new(event_sender.clone(), 8)?;
    connections.register_listener(&mut event_manager)?;

    let event_manager = Arc::new(Mutex::new(event_manager));
    let event_barrier = Arc::new(Barrier::new(2));
    let _ = run_event_loop(event_manager.clone(), event_barrier.clone());
    let _ = I::run_event_loop(event_sender);

    let mut local_client = Client::new();
    let mut editor = Editor::new();
    bind_keys(&mut editor);

    let mut editor_operations = EditorOperationSender::new();
    let mut received_keys = Vec::new();

    ui.init()?;

    for event in event_receiver.iter() {
        let mut error = None;

        match event {
            Event::None => (),
            Event::Key(key) => {
                match editor.on_key(key, TargetClient::Local, &mut editor_operations) {
                    EditorLoop::Quit => break,
                    EditorLoop::Continue => (),
                    EditorLoop::Error(e) => error = Some(e),
                }
                send_operations(&mut editor_operations, &mut local_client, &mut connections);
            }
            Event::Resize(w, h) => ui.resize(w, h)?,
            Event::Connection(event) => {
                dbg!("connection event");
                let mut event_manager = event_manager.lock().unwrap();
                match event {
                    ConnectionEvent::NewConnection => {
                        let handle = connections.accept_connection(&mut event_manager)?;
                        editor
                            .on_client_joined(TargetClient::Remote(handle), &mut editor_operations);
                    }
                    ConnectionEvent::StreamError(stream_id) => {
                        dbg!("stream error event");
                        let handle = stream_id.into();
                        connections.close_connection(handle);
                        editor.on_client_left(TargetClient::Remote(handle), &mut editor_operations);
                    }
                    ConnectionEvent::StreamIn(stream_id) => {
                        let handle = stream_id.into();
                        loop {
                            dbg!("stream in event loop");
                            match connections.receive_key(handle) {
                                Ok(Some(key)) => received_keys.push(key),
                                Ok(None) => break,
                                Err(e) => {
                                    dbg!("stream in event + error", e);
                                    connections.close_connection(handle);
                                    editor.on_client_left(
                                        TargetClient::Remote(handle),
                                        &mut editor_operations,
                                    );
                                    break;
                                }
                            }
                            break;
                        }

                        for key in received_keys.drain(..) {
                            match editor.on_key(key, TargetClient::Local, &mut editor_operations) {
                                EditorLoop::Quit => {
                                    dbg!("stream in event + remote quit");
                                    connections.close_connection(handle);
                                    editor.on_client_left(
                                        TargetClient::Remote(handle),
                                        &mut editor_operations,
                                    );
                                }
                                EditorLoop::Continue => (),
                                EditorLoop::Error(e) => error = Some(e),
                            }
                        }
                    }
                }

                connections.unregister_closed_connections(&mut event_manager)?;
                send_operations(&mut editor_operations, &mut local_client, &mut connections);
                connections.unregister_closed_connections(&mut event_manager)?;
                drop(event_manager);
                event_barrier.wait();
            }
        }

        ui.draw(&local_client, error)?;
    }

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
    let mut event_manager = EventManager::new(event_sender.clone(), 8)?;
    connection.register_connection(&mut event_manager)?;

    let event_manager = Arc::new(Mutex::new(event_manager));
    let event_barrier = Arc::new(Barrier::new(2));
    let _ = run_event_loop(event_manager, event_barrier.clone());
    let _ = I::run_event_loop(event_sender);

    let mut local_client = Client::new();
    let mut received_operations = Vec::new();

    ui.init()?;

    'main_loop: for event in event_receiver.iter() {
        match event {
            Event::None => (),
            Event::Key(key) => {
                if connection.send_key(key).is_err() {
                    dbg!("error sending key");
                    break;
                }
            }
            Event::Resize(w, h) => ui.resize(w, h)?,
            Event::Connection(event) => {
                match event {
                    ConnectionEvent::NewConnection => (),
                    ConnectionEvent::StreamError(_) => {
                        dbg!("connection error");
                        break;
                    }
                    ConnectionEvent::StreamIn(_) => {
                        loop {
                            dbg!("receive operation loop");
                            match connection.receive_operation() {
                                Ok(Some(operation)) => received_operations.push(operation),
                                Ok(None) => break,
                                Err(_) => {
                                    dbg!("error reading operation");
                                    break 'main_loop;
                                }
                            }
                        }

                        for (operation, content) in received_operations.drain(..) {
                            local_client.on_editor_operation(&operation, &content[..]);
                        }
                    }
                }
                event_barrier.wait();
            }
        }

        ui.draw(&local_client, None)?;
    }

    connection.close();
    ui.shutdown()?;
    Ok(())
}
