use std::{
    collections::hash_map::DefaultHasher,
    env,
    error::Error,
    fmt, fs,
    hash::{Hash, Hasher},
    path::Path,
    sync::mpsc,
    time::Instant,
};

use crate::platform::{
    ClientApplication, ConnectionHandle, Platform, ProcessHandle, ServerApplication, ServerEvent,
};

use crate::{
    client::{ClientManager, TargetClient},
    client_event::{ClientEvent, ClientEventSerializer, Key, LocalEvent},
    connection::{ConnectionWithClientCollection, ConnectionWithServer},
    editor::{Editor, EditorLoop},
    event_manager::{ConnectionEvent, EventManager},
    lsp::LspClientCollection,
    task::TaskManager,
    ui::{self, Ui, UiKind, UiResult},
    Args,
};

fn print_version() {
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");
    println!("{} version {}", name, version);
}

pub struct Server {
    args: Args,
    connections: Vec<ConnectionHandle>,
}
impl ServerApplication for Server {
    fn new<P>(platform: &mut P) -> Option<Self>
    where
        P: Platform,
    {
        let args: Args = argh::from_env();
        if args.version {
            print_version();
            return None;
        }

        let (event_sender, event_receiver) = mpsc::channel();
        let current_dir = env::current_dir().expect("could not retrieve the current directory");
        let tasks = TaskManager::new(event_sender.clone());
        let lsp = LspClientCollection::new(event_sender.clone());
        let mut editor = Editor::new(current_dir, tasks, lsp);
        let mut clients = ClientManager::default();

        for config in &args.config {
            editor.load_config(&mut clients, config);
        }

        Some(Self {
            args,
            connections: Vec::new(),
        })
    }

    fn on_event<P>(&mut self, platform: &mut P, event: ServerEvent) -> bool
    where
        P: Platform,
    {
        match event {
            ServerEvent::ConnectionOpen(handle) => self.connections.push(handle),
            ServerEvent::ConnectionClose(handle) => {
                if let Some(index) = self.connections.iter().position(|c| *c == handle) {
                    self.connections.remove(index);
                }

                if self.connections.is_empty() {
                    return false;
                }
            }
            //ServerEvent::ConnectionMessage(handle) => {
            //
            //}
            _ => (),
        }

        true
    }
}

pub struct Client {
    //
}
impl ClientApplication for Client {
    fn new() -> Option<Self> {
        let args: Args = argh::from_env();
        if args.version {
            print_version();
            return None;
        }

        Some(Self {})
    }

    fn on_event(&mut self, event: crate::platform::ClientEvent) -> &[u8] {
        &[]
    }
}

// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------

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

fn u64_to_str(buf: &mut [u8], value: u64) -> &str {
    use std::fmt::Write;
    struct Formatter<'a> {
        buf: &'a mut [u8],
        len: usize,
    }
    impl<'a> Write for Formatter<'a> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let bytes = s.as_bytes();
            let len = self.len + bytes.len();
            self.buf[self.len..len].copy_from_slice(bytes);
            self.len = len;
            Ok(())
        }
    }
    let mut formatter = Formatter { buf, len: 0 };
    let _ = write!(formatter, "{}", value);
    let formatted = &formatter.buf[..formatter.len];
    unsafe { std::str::from_utf8_unchecked(formatted) }
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

    match args.session.as_ref() {
        Some(session) => {
            if !session.chars().all(char::is_alphanumeric) {
                return Err(Box::new(ApplicationError(format!(
                    "invalid session name '{}'. it can only contain alphanumeric characters",
                    session
                ))));
            }
            session_socket_path.push(session);
        }
        None => {
            let current_dir = env::current_dir().map_err(|e| Box::new(e))?;
            let mut hasher = DefaultHasher::new();
            current_dir.hash(&mut hasher);
            let hash = hasher.finish();
            let mut buf = [0; 32];
            let hash = u64_to_str(&mut buf, hash);
            session_socket_path.push(hash);
        }
    }

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
    clients: &mut ClientManager,
    ui: &mut I,
    connections: &mut ConnectionWithClientCollection,
) -> UiResult<bool>
where
    I: Ui,
{
    let needs_redraw = editor.on_pre_render(clients);

    let focused_target = clients.focused_target();
    for c in clients.client_refs() {
        let has_focus = focused_target == c.target;
        c.ui.render(editor, c.client, has_focus, c.buffer);
        match c.target {
            TargetClient::Local => ui.display(c.buffer)?,
            TargetClient::Remote(handle) => connections.send_serialized_display(handle, c.buffer),
        }
    }

    Ok(needs_redraw)
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
    let (event_sender, event_receiver) = mpsc::channel();

    let current_dir = env::current_dir().map_err(Box::new)?;
    let tasks = TaskManager::new(event_sender.clone());
    let lsp = LspClientCollection::new(event_sender.clone());
    let mut editor = Editor::new(current_dir, tasks, lsp);
    let mut clients = ClientManager::default();

    for config in &args.config {
        editor.load_config(&mut clients, config);
    }

    client_events_from_args(&args, |event| {
        editor.on_event(&mut clients, TargetClient::Local, event);
    });

    let event_manager = EventManager::new()?;
    let event_registry = event_manager.registry();
    let event_manager_loop = event_manager.run_event_loop_in_background(event_sender.clone());
    let ui_event_loop = ui.run_event_loop_in_background(event_sender.clone());

    connections.register_listener(&event_registry)?;

    ui.init()?;

    for event in event_receiver.iter() {
        profiler.begin_frame();

        match event {
            LocalEvent::None => continue,
            LocalEvent::EndOfInput => break,
            LocalEvent::Idle => editor.on_idle(&mut clients),
            LocalEvent::Repaint => (),
            LocalEvent::Key(key) => {
                editor.status_bar.clear();
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
                        editor.status_bar.clear();
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
            LocalEvent::TaskEvent(client, handle, result) => {
                editor.on_task_event(&mut clients, client, handle, result);
            }
            LocalEvent::Lsp(handle, event) => {
                editor.on_lsp_event(handle, event);
            }
        }

        let needs_redraw = render_clients(&mut editor, &mut clients, &mut ui, &mut connections)?;
        if needs_redraw {
            event_sender.send(LocalEvent::Repaint)?;
        }

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
            LocalEvent::None | LocalEvent::Idle | LocalEvent::Repaint => continue,
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
            _ => unreachable!(),
        }
    }

    drop(event_manager_loop);
    drop(ui_event_loop);

    connection.close();
    ui.shutdown()?;
    Ok(())
}
