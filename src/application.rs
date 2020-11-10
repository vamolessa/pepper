use std::{env, error::Error, fmt, fs, path::Path, sync::mpsc, time::Instant};

use crate::{
    client::{ClientCollection, TargetClient},
    client_event::{ClientEvent, ClientEventSerializer, Key, LocalEvent},
    connection::{ConnectionWithClientCollection, ConnectionWithServer},
    editor::{Editor, EditorLoop},
    event_manager::{ConnectionEvent, EventManager},
    ui::{self, Ui, UiKind, UiResult},
    Args,
};

trait Profiler {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
}

struct DummyProfiler;
impl Profiler for DummyProfiler {}

struct SimpleProfiler(Option<Instant>);
impl SimpleProfiler {
    pub fn new() -> Self {
        Self(None)
    }
}
impl Profiler for SimpleProfiler {
    fn begin_frame(&mut self) {
        if let None = self.0 {
            self.0 = Some(Instant::now())
        }
    }

    fn end_frame(&mut self) {
        if let Some(instant) = self.0.take() {
            eprintln!("{}", instant.elapsed().as_millis());
        }
    }
}

#[derive(Debug)]
pub struct ApplicationError(String);
impl Error for ApplicationError {}
impl fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn run(args: Args) -> Result<(), Box<dyn Error>> {
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

    if args.as_focused_client || args.as_client.is_some() {
        run_with_ui(args, ui::none_ui::NoneUi, &session_socket_path)
    } else {
        let stdout = std::io::stdout();
        let stdout = stdout.lock();
        let ui = ui::tui::Tui::new(stdout);
        run_with_ui(args, ui, &session_socket_path)
    }
}

fn run_with_ui<I>(args: Args, ui: I, session_socket_path: &Path) -> Result<(), Box<dyn Error>>
where
    I: Ui,
{
    if let Ok(connection) = ConnectionWithServer::connect(session_socket_path) {
        if args.profile {
            run_client(args, SimpleProfiler::new(), ui, connection)?;
        } else {
            run_client(args, DummyProfiler, ui, connection)?;
        }
    } else if let Ok(listener) = ConnectionWithClientCollection::listen(session_socket_path) {
        if args.profile {
            run_server_with_client(args, SimpleProfiler::new(), ui, listener)?;
        } else {
            run_server_with_client(args, DummyProfiler, ui, listener)?;
        }
        fs::remove_file(session_socket_path)?;
    } else if let Ok(()) = fs::remove_file(session_socket_path) {
        let listener = ConnectionWithClientCollection::listen(session_socket_path)?;
        if args.profile {
            run_server_with_client(args, SimpleProfiler::new(), ui, listener)?;
        } else {
            run_server_with_client(args, DummyProfiler, ui, listener)?;
        }
        fs::remove_file(session_socket_path)?;
    } else {
        return Err(Box::new(ApplicationError(
            "could not connect to or start the server".into(),
        )));
    }

    Ok(())
}

fn client_events_from_args<F>(args: &Args, mut func: F)
where
    F: FnMut(ClientEvent),
{
    if args.as_focused_client {
        func(ClientEvent::Ui(UiKind::None));
        func(ClientEvent::AsFocusedClient);
    } else if let Some(target_client) = args.as_client {
        func(ClientEvent::Ui(UiKind::None));
        func(ClientEvent::AsClient(target_client));
    }

    for path in &args.files {
        func(ClientEvent::OpenBuffer(path));
    }
}

fn render_clients<I>(
    editor: &mut Editor,
    clients: &mut ClientCollection,
    ui: &mut I,
    connections: &mut ConnectionWithClientCollection,
) -> UiResult<()>
where
    I: Ui,
{
    for c in clients.client_refs() {
        c.ui.render(editor, c.client, c.target, c.buffer)?;
        match c.target {
            TargetClient::Local => ui.display(c.buffer)?,
            TargetClient::Remote(handle) => connections.send_serialized_display(handle, c.buffer),
        }
    }

    Ok(())
}

fn run_server_with_client<P, I>(
    args: Args,
    mut profiler: P,
    mut ui: I,
    mut connections: ConnectionWithClientCollection,
) -> Result<(), Box<dyn Error>>
where
    P: Profiler,
    I: Ui,
{
    let mut editor = Editor::new();
    let mut clients = ClientCollection::default();

    for path in &args.module_search_path {
        editor.add_module_search_path(path);
    }

    for config in &args.config {
        editor.load_config(&mut clients, config);
    }

    client_events_from_args(&args, |event| {
        editor.on_event(&mut clients, TargetClient::Local, event);
    });

    let (event_sender, event_receiver) = mpsc::channel();
    let event_manager = EventManager::new()?;
    let event_registry = event_manager.registry();
    let event_manager_loop = event_manager.run_event_loop_in_background(event_sender.clone());
    let ui_event_loop = ui.run_event_loop_in_background(event_sender);

    connections.register_listener(&event_registry)?;

    ui.init()?;

    for event in event_receiver.iter() {
        profiler.begin_frame();

        match event {
            LocalEvent::None => continue,
            LocalEvent::EndOfInput => break,
            LocalEvent::Key(key) => {
                editor.status_message.clear();
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

                        profiler.end_frame();
                        continue;
                    }
                    ConnectionEvent::Stream(stream_id) => {
                        editor.status_message.clear();
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
            LocalEvent::Lsp(handle, event) => {
                editor.on_lsp_event(handle, event);
            }
        }

        render_clients(&mut editor, &mut clients, &mut ui, &mut connections)?;
        profiler.end_frame();
    }

    drop(event_manager_loop);
    drop(ui_event_loop);

    connections.close_all_connections();
    ui.shutdown()?;
    Ok(())
}

fn run_client<P, I>(
    args: Args,
    mut profiler: P,
    mut ui: I,
    mut connection: ConnectionWithServer,
) -> Result<(), Box<dyn Error>>
where
    P: Profiler,
    I: Ui,
{
    let mut client_events = ClientEventSerializer::default();
    client_events_from_args(&args, |event| {
        client_events.serialize(event);
    });

    let (event_sender, event_receiver) = mpsc::channel();
    let event_manager = EventManager::new()?;
    let event_registry = event_manager.registry();
    let event_manager_loop = event_manager.run_event_loop_in_background(event_sender.clone());
    let ui_event_loop = ui.run_event_loop_in_background(event_sender);

    connection.register_connection(&event_registry)?;

    ui.init()?;

    client_events.serialize(ClientEvent::Key(Key::None));
    connection.send_serialized_events(&mut client_events)?;

    for event in event_receiver.iter() {
        match event {
            LocalEvent::None => (),
            LocalEvent::EndOfInput => break,
            LocalEvent::Key(key) => {
                profiler.begin_frame();

                client_events.serialize(ClientEvent::Key(key));
                if let Err(_) = connection.send_serialized_events(&mut client_events) {
                    break;
                }
            }
            LocalEvent::Resize(w, h) => {
                profiler.begin_frame();

                client_events.serialize(ClientEvent::Resize(w, h));
                if let Err(_) = connection.send_serialized_events(&mut client_events) {
                    break;
                }
            }
            LocalEvent::Connection(event) => {
                match event {
                    ConnectionEvent::NewConnection => (),
                    ConnectionEvent::Stream(_) => {
                        let bytes = connection.receive_display()?;
                        if bytes.is_empty() {
                            break;
                        }
                        ui.display(bytes)?;
                        connection.listen_next_event(&event_registry)?;
                    }
                }

                profiler.end_frame();
            }
            LocalEvent::Lsp(_, _) => (),
        }
    }

    drop(event_manager_loop);
    drop(ui_event_loop);

    connection.close();
    ui.shutdown()?;
    Ok(())
}
