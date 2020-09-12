use std::{env, error::Error, fmt, fs, sync::mpsc, thread};

use crate::{
    client::{Client, ClientCollection, TargetClient},
    client_event::{ClientEvent, ClientEventSerializer, Key, LocalEvent},
    connection::{ConnectionWithClientCollection, ConnectionWithServer},
    editor::{Editor, EditorLoop},
    event_manager::{ConnectionEvent, EventManager},
    tui::Tui,
    Args,
};

#[derive(Debug)]
pub struct ApplicationError(String);
impl Error for ApplicationError {}
impl fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait UI {
    type Error: 'static + Send + Error;

    fn run_event_loop_in_background(
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> thread::JoinHandle<Result<(), Self::Error>>;

    fn init(&mut self) -> Result<(u16, u16), Self::Error> {
        Ok((0, 0))
    }

    fn render(
        &mut self,
        editor: &Editor,
        client: &Client,
        target_client: TargetClient,
        buffer: &mut Vec<u8>,
    ) -> Result<(), Self::Error>;

    fn display(&mut self, buffer: &[u8]) -> Result<(), Self::Error>;

    fn shutdown(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub fn run(args: Args) -> Result<(), Box<dyn Error>> {
    if let Err(e) = ctrlc::set_handler(|| {}) {
        return Err(Box::new(e));
    }

    let mut session_socket_path = env::temp_dir();
    session_socket_path.push(env!("CARGO_PKG_NAME"));
    if !session_socket_path.exists() {
        std::fs::create_dir_all(&session_socket_path).map_err(|e| Box::new(e))?;
    }

    let mut session_name = match args.session.clone() {
        Some(session) => session,
        None => env::current_dir()
            .map_err(|e| Box::new(e))?
            .to_string_lossy()
            .into_owned(),
    };
    session_name.retain(|c| c.is_alphanumeric());
    session_socket_path.push(session_name);

    let stdout = std::io::stdout();
    let stdout = stdout.lock();
    let ui = Tui::new(stdout);

    if let Ok(connection) = ConnectionWithServer::connect(&session_socket_path) {
        run_client(args, ui, connection)?;
    } else if let Ok(listener) = ConnectionWithClientCollection::listen(&session_socket_path) {
        run_server_with_client(args, ui, listener)?;
        fs::remove_file(session_socket_path)?;
    } else if let Ok(()) = fs::remove_file(&session_socket_path) {
        let listener = ConnectionWithClientCollection::listen(&session_socket_path)?;
        run_server_with_client(args, ui, listener)?;
        fs::remove_file(session_socket_path)?;
    } else {
        return Err(Box::new(ApplicationError(
            "could not connect to or start the server".into(),
        )));
    }

    Ok(())
}

fn client_events_from_args<F>(args: &Args, mut func: F) -> Result<EditorLoop, Box<dyn Error>>
where
    F: FnMut(ClientEvent),
{
    let mut result = EditorLoop::Continue;

    if args.as_focused_client {
        func(ClientEvent::AsFocusedClient);
        result = EditorLoop::Quit;
    } else if let Some(client_index) = args.as_client {
        func(ClientEvent::AsClient(client_index));
        result = EditorLoop::Quit;
    }

    for path in &args.files {
        func(ClientEvent::OpenFile(path));
    }

    if let Some(keys) = args.keys.as_ref() {
        for key in Key::parse_all(keys) {
            match key {
                Ok(key) => func(ClientEvent::Key(key)),
                Err(error) => return Err(Box::new(error)),
            }
        }

        result = EditorLoop::Quit;
    }

    Ok(result)
}

fn render_clients<I>(
    editor: &Editor,
    clients: &mut ClientCollection,
    ui: &mut I,
    connections: &mut ConnectionWithClientCollection,
) -> Result<(), I::Error>
where
    I: UI,
{
    for c in clients.client_refs() {
        c.buffer.clear();
        ui.render(&editor, c.client, c.target, c.buffer)?;
        match c.target {
            TargetClient::Local => ui.display(c.buffer)?,
            TargetClient::Remote(handle) => connections.send_serialized_display(handle, c.buffer),
        }
    }
    Ok(())
}

fn run_server_with_client<I>(
    args: Args,
    mut ui: I,
    mut connections: ConnectionWithClientCollection,
) -> Result<(), Box<dyn Error>>
where
    I: UI,
{
    let (event_sender, event_receiver) = mpsc::channel();

    let event_manager = EventManager::new()?;
    let event_registry = event_manager.registry();
    let event_manager_loop = event_manager.run_event_loop_in_background(event_sender.clone());
    let ui_event_loop = I::run_event_loop_in_background(event_sender);

    let mut editor = Editor::new();
    let mut clients = ClientCollection::default();

    if let Some(config_path) = args.config.as_ref() {
        editor.load_config(&mut clients, config_path);
    }

    let editor_loop = client_events_from_args(&args, |event| {
        editor.on_event(&mut clients, TargetClient::Local, event);
    })?;
    if editor_loop.is_quit() {
        return Ok(());
    }

    connections.register_listener(&event_registry)?;

    let (local_width, local_height) = ui.init()?;
    let _ = editor.on_event(
        &mut clients,
        TargetClient::Local,
        ClientEvent::Resize(local_width, local_height),
    );

    render_clients(&editor, &mut clients, &mut ui, &mut connections)?;
    editor.status_message.clear();

    for event in event_receiver.iter() {
        match event {
            LocalEvent::None => continue,
            LocalEvent::Key(key) => {
                let editor_loop =
                    editor.on_event(&mut clients, TargetClient::Local, ClientEvent::Key(key));
                if editor_loop.is_quit() {
                    break;
                }
            }
            LocalEvent::Resize(w, h) => {
                let editor_loop =
                    editor.on_event(&mut clients, TargetClient::Local, ClientEvent::Resize(w, h));
                if editor_loop.is_quit() {
                    break;
                }
            }
            LocalEvent::Connection(event) => {
                match event {
                    ConnectionEvent::NewConnection => {
                        let handle = connections.accept_connection(&event_registry)?;
                        editor.on_client_joined(&mut clients, handle);
                        connections.listen_next_listener_event(&event_registry)?;
                    }
                    ConnectionEvent::Stream(stream_id) => {
                        let handle = stream_id.into();
                        let editor_loop = connections.receive_events(handle, |event| {
                            editor.on_event(&mut clients, TargetClient::Remote(handle), event)
                        });
                        match editor_loop {
                            Ok(EditorLoop::QuitAll) => break,
                            Ok(EditorLoop::Quit) | Err(_) => {
                                connections.close_connection(handle);
                                editor.on_client_left(&mut clients, handle);
                            }
                            Ok(EditorLoop::Continue) => {
                                connections
                                    .listen_next_connection_event(handle, &event_registry)?;
                            }
                        }
                    }
                }

                connections.unregister_closed_connections(&event_registry)?;
            }
        }

        render_clients(&editor, &mut clients, &mut ui, &mut connections)?;
        editor.status_message.clear();
    }

    drop(event_manager_loop);
    drop(ui_event_loop);

    connections.close_all_connections();
    ui.shutdown()?;
    Ok(())
}

fn run_client<I>(
    args: Args,
    mut ui: I,
    mut connection: ConnectionWithServer,
) -> Result<(), Box<dyn Error>>
where
    I: UI,
{
    let (event_sender, event_receiver) = mpsc::channel();

    let mut client_events = ClientEventSerializer::default();
    let editor_loop = client_events_from_args(&args, |event| {
        client_events.serialize(event);
    })?;

    client_events.serialize(ClientEvent::Key(Key::None));
    let _ = connection.send_serialized_events_blocking(&mut client_events);
    if editor_loop.is_quit() {
        connection.receive_display(|_| ())?;
        connection.close();
        return Ok(());
    }

    let event_manager = EventManager::new()?;
    let event_registry = event_manager.registry();
    let event_manager_loop = event_manager.run_event_loop_in_background(event_sender.clone());
    let ui_event_loop = I::run_event_loop_in_background(event_sender);

    connection.register_connection(&event_registry)?;

    let (local_width, local_height) = ui.init()?;
    client_events.serialize(ClientEvent::Resize(local_width, local_height));
    connection.send_serialized_events(&mut client_events)?;

    for event in event_receiver.iter() {
        match event {
            LocalEvent::None => (),
            LocalEvent::Key(key) => {
                client_events.serialize(ClientEvent::Key(key));
                if let Err(_) = connection.send_serialized_events(&mut client_events) {
                    break;
                }
            }
            LocalEvent::Resize(w, h) => {
                client_events.serialize(ClientEvent::Resize(w, h));
                if let Err(_) = connection.send_serialized_events(&mut client_events) {
                    break;
                }
            }
            LocalEvent::Connection(event) => match event {
                ConnectionEvent::NewConnection => (),
                ConnectionEvent::Stream(_) => {
                    let empty = connection
                        .receive_display(|bytes| ui.display(bytes).map(|_| bytes.is_empty()))??;
                    if empty {
                        break;
                    }

                    connection.listen_next_event(&event_registry)?;
                }
            },
        }
    }

    drop(event_manager_loop);
    drop(ui_event_loop);

    connection.close();
    ui.shutdown()?;
    Ok(())
}
