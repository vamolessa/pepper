use std::{env, io};

use crate::{
    client::{ClientManager, TargetClient},
    client_event::ClientEvent,
    command::CommandOperation,
    connection::ClientEventDeserializationBufCollection,
    editor::{Editor, EditorLoop},
    platform::{ClientPlatform, ClientPlatformEvent, ServerPlatform, ServerPlatformEvent},
    serialization::{SerializationBuf, Serialize},
    ui, Args,
};

impl Args {
    pub fn parse() -> Option<Self> {
        let args: Args = argh::from_env();
        if args.version {
            let name = env!("CARGO_PKG_NAME");
            let version = env!("CARGO_PKG_VERSION");
            println!("{} version {}", name, version);
            return None;
        }

        if let Some(ref session) = args.session {
            if !session.chars().all(char::is_alphanumeric) {
                panic!(
                    "invalid session name '{}'. it can only contain alphanumeric characters",
                    session
                );
            }
        }

        Some(args)
    }
}

pub struct ServerApplication {
    editor: Editor,
    clients: ClientManager,
    event_deserialization_bufs: ClientEventDeserializationBufCollection,
    connections_with_error: Vec<usize>,
}
impl ServerApplication {
    pub fn connection_buffer_len() -> usize {
        512
    }

    pub fn new<P>(args: Args, _: &mut P) -> Option<Self>
    where
        P: ServerPlatform,
    {
        let current_dir = env::current_dir().expect("could not retrieve the current directory");
        let mut editor = Editor::new(current_dir);
        let mut clients = ClientManager::new();

        for config in &args.config {
            if let Some(CommandOperation::Quit) | Some(CommandOperation::QuitAll) =
                editor.load_config(&mut clients, config)
            {
                return None;
            }
        }

        let event_deserialization_bufs = ClientEventDeserializationBufCollection::default();

        Some(Self {
            editor,
            clients,
            event_deserialization_bufs,
            connections_with_error: Vec::new(),
        })
    }

    pub fn on_event<P>(&mut self, platform: &mut P, event: ServerPlatformEvent) -> bool
    where
        P: ServerPlatform,
    {
        match event {
            ServerPlatformEvent::Redraw => (),
            ServerPlatformEvent::Idle => self.editor.on_idle(&mut self.clients),
            ServerPlatformEvent::ConnectionOpen { index } => {
                if let Some(handle) = TargetClient::from_index(index) {
                    self.clients.on_client_joined(handle)
                }
            }
            ServerPlatformEvent::ConnectionClose { index } => {
                if let Some(handle) = TargetClient::from_index(index) {
                    self.clients.on_client_left(handle);
                    if self.clients.iter_mut().next().is_none() {
                        return false;
                    }
                }
            }
            ServerPlatformEvent::ConnectionMessage { index, len } => {
                let handle = match TargetClient::from_index(index) {
                    Some(handle) => handle,
                    None => return true,
                };

                let bytes = platform.read_from_connection(index, len);
                let editor = &mut self.editor;
                let clients = &mut self.clients;

                let editor_loop =
                    self.event_deserialization_bufs
                        .receive_events(index, bytes, |event| {
                            editor.on_event(clients, handle, event)
                        });
                match editor_loop {
                    EditorLoop::Continue => (),
                    EditorLoop::Quit => platform.close_connection(index),
                    EditorLoop::QuitAll => return false,
                }
            }
            ServerPlatformEvent::ProcessStdout { index, len } => {
                let _bytes = platform.read_from_process_stdout(index, len);
                //
            }
            ServerPlatformEvent::ProcessStderr { index, len } => {
                let _bytes = platform.read_from_process_stderr(index, len);
                //
            }
            ServerPlatformEvent::ProcessExit { index, success } => {
                //
            }
        }

        let needs_redraw = self.editor.on_pre_render(&mut self.clients);
        if needs_redraw {
            platform.request_redraw();
        }

        let focused_handle = self.clients.focused_handle();
        for c in self.clients.iter_mut() {
            let has_focus = focused_handle == c.handle();
            c.display_buffer.clear();
            c.display_buffer.extend_from_slice(&[0; 4]);
            ui::render(
                &self.editor,
                c.buffer_view_handle(),
                c.viewport_size.0 as _,
                c.viewport_size.1 as _,
                c.scroll,
                has_focus,
                &mut c.display_buffer,
                &mut c.status_bar_buffer,
            );

            let len = c.display_buffer.len() as u32 - 4;
            let len_bytes = len.to_le_bytes();
            c.display_buffer[..4].copy_from_slice(&len_bytes);

            let connection_index = c.handle().into_index();
            if !platform.write_to_connection(connection_index, &c.display_buffer) {
                self.connections_with_error.push(connection_index);
            }
        }

        for index in self.connections_with_error.drain(..) {
            platform.close_connection(index);
            if let Some(handle) = TargetClient::from_index(index) {
                self.clients.on_client_left(handle);
                if self.clients.iter_mut().next().is_none() {
                    return false;
                }
            }
        }

        true
    }
}

pub struct ClientApplication {
    as_client_handle: Option<TargetClient>,
    read_buf: Vec<u8>,
    write_buf: SerializationBuf,
    stdout: io::StdoutLock<'static>,
}
impl ClientApplication {
    pub fn connection_buffer_len() -> usize {
        2 * 1024
    }

    pub fn new<P>(args: Args, platform: &mut P) -> Self
    where
        P: ClientPlatform,
    {
        static mut STDOUT: Option<io::Stdout> = None;
        let mut stdout = unsafe {
            STDOUT = Some(io::stdout());
            STDOUT.as_ref().unwrap().lock()
        };

        let mut command = String::new();
        let mut write_buf = SerializationBuf::default();
        for path in &args.files {
            command.clear();
            command.push_str("open '");
            command.push_str(path);
            command.push_str("'");
            ClientEvent::Command(args.as_client, &command).serialize(&mut write_buf);
        }
        let bytes = write_buf.as_slice();
        if !bytes.is_empty() {
            platform.write(bytes);
        }

        use io::Write;
        let _ = stdout.write_all(ui::ENTER_ALTERNATE_BUFFER_CODE);
        let _ = stdout.write_all(ui::HIDE_CURSOR_CODE);
        let _ = stdout.write_all(ui::MODE_256_COLORS_CODE);
        let _ = stdout.flush();

        Self {
            as_client_handle: args.as_client,
            read_buf: Vec::new(),
            write_buf,
            stdout,
        }
    }

    pub fn on_events<P>(&mut self, platform: &mut P, events: &[ClientPlatformEvent]) -> bool
    where
        P: ClientPlatform,
    {
        use io::Write;

        self.write_buf.clear();
        for event in events {
            match event {
                ClientPlatformEvent::Key(key) => {
                    ClientEvent::Key(self.as_client_handle, *key).serialize(&mut self.write_buf);
                }
                ClientPlatformEvent::Resize(width, height) => {
                    ClientEvent::Resize(self.as_client_handle, *width as _, *height as _)
                        .serialize(&mut self.write_buf);
                }
                ClientPlatformEvent::Message(len) => {
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

                    self.read_buf.extend_from_slice(ui::RESET_STYLE_CODE);
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
impl Drop for ClientApplication {
    fn drop(&mut self) {
        use io::Write;

        let _ = self.stdout.write_all(ui::EXIT_ALTERNATE_BUFFER_CODE);
        let _ = self.stdout.write_all(ui::SHOW_CURSOR_CODE);
        let _ = self.stdout.write_all(ui::RESET_STYLE_CODE);
        let _ = self.stdout.flush();
    }
}

// TODO: delete old code
/*
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
