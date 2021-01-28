use std::{
    collections::hash_map::DefaultHasher,
    env,
    error::Error,
    fmt, fs,
    hash::{Hash, Hasher},
    io,
    path::Path,
    sync::mpsc,
    time::Instant,
};

use crate::platform::{
    ClientApplication, ClientPlatform, Key, PlatformApplication, PlatformClientEvent,
    PlatformServerEvent, ServerApplication, ServerPlatform,
};

use crate::{
    client::{ClientManager, TargetClient},
    client_event::{ClientEvent, ClientEventSerializer, LocalEvent},
    connection::ClientEventDeserializationBufCollection,
    editor::{Editor, EditorLoop},
    event_manager::{ConnectionEvent, EventManager},
    lsp::LspClientCollection,
    serialization::{SerializationBuf, Serialize},
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
    editor: Editor,
    clients: ClientManager,
    event_deserialization_bufs: ClientEventDeserializationBufCollection,
    connections_with_error: Vec<usize>,
}
impl PlatformApplication for Server {
    type Args = Args;
    fn parse_args() -> Option<Self::Args> {
        let args: Args = argh::from_env();
        if args.version {
            print_version();
            return None;
        }

        Some(args)
    }
}
impl ServerApplication for Server {
    fn new<P>(args: Self::Args, platform: &mut P) -> Self
    where
        P: ServerPlatform,
    {
        let current_dir = env::current_dir().expect("could not retrieve the current directory");
        let tasks = TaskManager::new();
        let lsp = LspClientCollection::new();
        let mut editor = Editor::new(current_dir, tasks, lsp);
        let mut clients = ClientManager::new();

        for config in &args.config {
            editor.load_config(&mut clients, config);
        }

        let event_deserialization_bufs = ClientEventDeserializationBufCollection::default();

        Self {
            editor,
            clients,
            event_deserialization_bufs,
            connections_with_error: Vec::new(),
        }
    }

    fn on_event<P>(&mut self, platform: &mut P, event: PlatformServerEvent) -> bool
    where
        P: ServerPlatform,
    {
        match event {
            PlatformServerEvent::Idle => (),
            PlatformServerEvent::ConnectionOpen { index } => self.clients.on_client_joined(index),
            PlatformServerEvent::ConnectionClose { index } => {
                self.clients.on_client_left(index);
                if self.clients.iter_mut().next().is_none() {
                    return false;
                }
            }
            PlatformServerEvent::ConnectionMessage { index, len } => {
                let bytes = platform.read_from_connection(index, len);
                let editor = &mut self.editor;
                let clients = &mut self.clients;
                let target = TargetClient::from_index(index);
                let editor_loop =
                    self.event_deserialization_bufs
                        .receive_events(index, bytes, |event| {
                            editor.on_event(clients, target, event)
                        });
                match editor_loop {
                    EditorLoop::Continue => (),
                    EditorLoop::Quit => platform.close_connection(index),
                    EditorLoop::QuitAll => return false,
                }
            }
            PlatformServerEvent::ProcessStdout { index, len } => {
                let _bytes = platform.read_from_process_stdout(index, len);
                //
            }
            PlatformServerEvent::ProcessStderr { index, len } => {
                let _bytes = platform.read_from_process_stderr(index, len);
                //
            }
            PlatformServerEvent::ProcessExit { index, success } => {
                //
            }
        }

        let needs_redraw = self.editor.on_pre_render(&mut self.clients);
        if needs_redraw {
            // TODO: notify platform for redraw
        }

        let focused_target = self.clients.focused_target();
        for c in self.clients.client_refs() {
            let has_focus = focused_target == c.target;
            c.buffer.clear();
            c.buffer.extend_from_slice(&[0; 4]);
            c.ui.render(&self.editor, c.client, has_focus, c.buffer);

            let len = c.buffer.len() as u32 - 4;
            let len_bytes = len.to_le_bytes();
            c.buffer[..4].copy_from_slice(&len_bytes);

            let connection_index = c.target.0;
            if !platform.write_to_connection(connection_index, c.buffer) {
                self.connections_with_error.push(connection_index);
            }
        }

        for handle in self.connections_with_error.drain(..) {
            platform.close_connection(handle);
            self.clients.on_client_left(handle);
            if self.clients.iter_mut().next().is_none() {
                return false;
            }
        }

        true
    }
}

pub struct Client {
    read_buf: Vec<u8>,
    write_buf: SerializationBuf,
    stdout: io::StdoutLock<'static>,
}
impl PlatformApplication for Client {
    type Args = Args;
    fn parse_args() -> Option<Self::Args> {
        let args: Args = argh::from_env();
        if args.version {
            print_version();
            return None;
        }

        Some(args)
    }
}
impl ClientApplication for Client {
    fn new<P>(args: Self::Args, platform: &mut P) -> Self
    where
        P: ClientPlatform,
    {
        static mut STDOUT: Option<io::Stdout> = None;
        let mut stdout = unsafe {
            STDOUT = Some(io::stdout());
            STDOUT.as_ref().unwrap().lock()
        };

        let mut write_buf = SerializationBuf::default();
        for path in &args.files {
            ClientEvent::OpenBuffer(path).serialize(&mut write_buf);
        }
        let bytes = write_buf.as_slice();
        if !bytes.is_empty() {
            platform.write(bytes);
        }

        use io::Write;
        let _ = stdout.write_all(crate::ui::tui::ENTER_ALTERNATE_BUFFER_CODE);
        let _ = stdout.write_all(crate::ui::tui::HIDE_CURSOR_CODE);
        let _ = stdout.flush();

        Self {
            read_buf: Vec::new(),
            write_buf,
            stdout,
        }
    }

    fn on_events<P>(&mut self, platform: &mut P, events: &[PlatformClientEvent]) -> bool
    where
        P: ClientPlatform,
    {
        use io::Write;

        self.write_buf.clear();
        for event in events {
            match event {
                PlatformClientEvent::Key(key) => {
                    ClientEvent::Key(*key).serialize(&mut self.write_buf);
                }
                PlatformClientEvent::Resize(width, height) => {
                    ClientEvent::Resize(*width as _, *height as _).serialize(&mut self.write_buf);
                }
                PlatformClientEvent::Message(len) => {
                    let buf = platform.read(*len);
                    self.read_buf.extend_from_slice(buf);
                    let mut len_bytes = [0; 4];
                    if self.read_buf.len() < len_bytes.len() {
                        continue;
                    }

                    len_bytes.copy_from_slice(&self.read_buf[..4]);
                    let message_len = u32::from_le_bytes(len_bytes) as usize;
                    if self.read_buf.len() < message_len + 4 {
                        continue;
                    }

                    self.stdout.write_all(&self.read_buf[4..]).unwrap();
                    self.read_buf.clear();
                }
            }
        }

        self.stdout.flush().unwrap();
        let bytes = self.write_buf.as_slice();
        bytes.is_empty() || platform.write(bytes)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        use io::Write;

        let _ = self
            .stdout
            .write_all(crate::ui::tui::EXIT_ALTERNATE_BUFFER_CODE);
        let _ = self.stdout.write_all(crate::ui::tui::SHOW_CURSOR_CODE);
        let _ = self.stdout.flush();
    }
}

// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------
// --------------------------------------------------------------------------------------

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

    /*
    if args.as_focused_client || args.as_client.is_some() {
        run_with_ui(args, ui::none_ui::NoneUi, &session_socket_path)
    } else {
        let stdout = std::io::stdout();
        let stdout = stdout.lock();
        let ui = ui::tui::Tui::new(stdout);
        run_with_ui(args, ui, &session_socket_path)
    }
    */

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

fn render_clients<I>(editor: &mut Editor, clients: &mut ClientManager, ui: &mut I) -> UiResult<bool>
where
    I: Ui,
{
    let needs_redraw = editor.on_pre_render(clients);

    let focused_target = clients.focused_target();
    for c in clients.client_refs() {
        let has_focus = focused_target == c.target;
        c.ui.render(editor, c.client, has_focus, c.buffer);
        //connections.send_serialized_display(c.target.connection_handle(), c.buffer);
    }

    Ok(needs_redraw)
}

/*
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
    let mut clients = ClientManager::new();

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
    //let _ = self.stream.set_nonblocking(false);
    //let _ = self.read_buf.read_from(&mut self.stream);
    //let _ = self.stream.write(&[0]);
    //let _ = self.stream.shutdown(Shutdown::Read);

    ui.shutdown()?;
    Ok(())
}
*/
