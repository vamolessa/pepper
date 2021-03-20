use std::{env, fmt, io, path::Path, sync::mpsc};

use crate::{
    client::{ClientHandle, ClientManager},
    command::{CommandManager, CommandOperation},
    editor::{Editor, EditorControlFlow},
    events::{ClientEvent, ClientEventReceiver, ServerEvent},
    lsp,
    platform::{Key, Platform, PlatformRequest, ProcessHandle, SharedBuf},
    serialization::{DeserializationSlice, DeserializeError, SerializationBuf, Serialize},
    ui, Args,
};

pub struct AnyError;
impl<T> From<T> for AnyError
where
    T: std::error::Error,
{
    fn from(_: T) -> Self {
        Self
    }
}

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

#[derive(Clone, Copy)]
pub enum ProcessTag {
    Command(usize),
    Lsp(lsp::ClientHandle),
}

pub enum ApplicationEvent {
    Idle,
    Redraw,
    ConnectionOpen {
        handle: ClientHandle,
    },
    ConnectionClose {
        handle: ClientHandle,
    },
    ConnectionMessage {
        handle: ClientHandle,
        buf: SharedBuf,
    },
    ProcessSpawned {
        tag: ProcessTag,
        handle: ProcessHandle,
    },
    ProcessStdout {
        tag: ProcessTag,
        buf: SharedBuf,
    },
    ProcessStderr {
        tag: ProcessTag,
        buf: SharedBuf,
    },
    ProcessExit {
        tag: ProcessTag,
        success: bool,
    },
}

pub struct ServerApplication;
impl ServerApplication {
    pub const fn connection_buffer_len() -> usize {
        512
    }

    pub fn run(args: Args, mut platform: Platform) -> Option<mpsc::Sender<ApplicationEvent>> {
        let current_dir = env::current_dir().expect("could not retrieve the current directory");
        let mut editor = Editor::new(current_dir);
        let mut clients = ClientManager::new();

        for config in &args.config {
            let config_path = Path::new(config);
            if let Some(CommandOperation::Quit) | Some(CommandOperation::QuitAll) =
                editor.load_config(&mut platform, &mut clients, config_path)
            {
                return None;
            }
        }

        let (event_sender, event_receiver) = mpsc::channel();

        let event_sender_clone = event_sender.clone();
        std::thread::spawn(move || {
            let _ = Self::run_application(
                editor,
                clients,
                &mut platform,
                event_sender_clone,
                event_receiver,
            );
            platform.enqueue_request(PlatformRequest::Exit);
            platform.flush_requests();
        });

        Some(event_sender)
    }

    fn run_application(
        mut editor: Editor,
        mut clients: ClientManager,
        platform: &mut Platform,
        event_sender: mpsc::Sender<ApplicationEvent>,
        event_receiver: mpsc::Receiver<ApplicationEvent>,
    ) -> Result<(), AnyError> {
        let mut client_event_receiver = ClientEventReceiver::default();

        'event_loop: loop {
            let mut event = event_receiver.recv()?;
            loop {
                match event {
                    ApplicationEvent::Idle => editor.on_idle(&mut clients, platform),
                    ApplicationEvent::Redraw => (),
                    ApplicationEvent::ConnectionOpen { handle } => {
                        clients.on_client_joined(handle);
                        let mut buf = platform.buf_pool.acquire();
                        let write = buf.write();
                        write.push(handle.into_index() as _);
                        let buf = buf.share();
                        platform.buf_pool.release(buf.clone());
                        platform.enqueue_request(PlatformRequest::WriteToClient { handle, buf });
                    }
                    ApplicationEvent::ConnectionClose { handle } => {
                        clients.on_client_left(handle);
                        if clients.iter_mut().next().is_none() {
                            break 'event_loop;
                        }
                    }
                    ApplicationEvent::ConnectionMessage { handle, buf } => {
                        let mut events =
                            client_event_receiver.receive_events(handle, buf.as_bytes());
                        while let Some(event) = events.next(&client_event_receiver) {
                            match editor.on_client_event(&mut clients, platform, event) {
                                EditorControlFlow::Continue => (),
                                EditorControlFlow::Quit => {
                                    platform
                                        .enqueue_request(PlatformRequest::CloseClient { handle });
                                    break;
                                }
                                EditorControlFlow::QuitAll => break 'event_loop,
                            }
                        }
                        events.finish(&mut client_event_receiver);
                    }
                    ApplicationEvent::ProcessSpawned { tag, handle } => match tag {
                        ProcessTag::Lsp(client_handle) => lsp::ClientManager::on_process_spawned(
                            &mut editor,
                            platform,
                            client_handle,
                            handle,
                        ),
                        ProcessTag::Command(index) => {
                            editor.commands.on_process_spawned(platform, index, handle)
                        }
                    },
                    ApplicationEvent::ProcessStdout { tag, buf } => {
                        let bytes = buf.as_bytes();
                        match tag {
                            ProcessTag::Lsp(client_handle) => {
                                lsp::ClientManager::on_process_stdout(
                                    &mut editor,
                                    platform,
                                    client_handle,
                                    bytes,
                                )
                            }
                            ProcessTag::Command(index) => CommandManager::on_process_stdout(
                                &mut editor,
                                platform,
                                &mut clients,
                                index,
                                bytes,
                            ),
                        }
                    }
                    ApplicationEvent::ProcessStderr { tag, buf } => {
                        let bytes = buf.as_bytes();
                        match tag {
                            _ => (),
                        }
                    }
                    ApplicationEvent::ProcessExit { tag, success } => match tag {
                        ProcessTag::Lsp(client_handle) => {
                            lsp::ClientManager::on_process_exit(&mut editor, client_handle)
                        }
                        ProcessTag::Command(index) => CommandManager::on_process_exit(
                            &mut editor,
                            platform,
                            &mut clients,
                            index,
                            success,
                        ),
                    },
                }

                event = match event_receiver.try_recv() {
                    Ok(event) => event,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return Err(AnyError),
                };
            }

            let needs_redraw = editor.on_pre_render(&mut clients);
            if needs_redraw {
                event_sender.send(ApplicationEvent::Redraw)?;
            }

            let focused_client_handle = clients.focused_handle();
            for c in clients.iter_mut() {
                if !c.has_ui() {
                    continue;
                }

                let has_focus = focused_client_handle == Some(c.handle());

                let mut buf = platform.buf_pool.acquire();
                let write = buf.write_with_len(5);
                ui::render(
                    &editor,
                    c.buffer_view_handle(),
                    c.viewport_size.0 as _,
                    c.viewport_size.1 as _,
                    c.scroll,
                    has_focus,
                    write,
                    &mut c.status_bar_buffer,
                );
                ServerEvent::serialize_display_header(write);

                let handle = c.handle();
                let buf = buf.share();
                platform.buf_pool.release(buf.clone());
                platform.enqueue_request(PlatformRequest::WriteToClient { handle, buf });
            }

            platform.flush_requests();
        }

        Ok(())
    }
}

pub struct ClientApplication {
    handle: ClientHandle,
    is_pipped: bool,
    read_buf: Vec<u8>,
    write_buf: SerializationBuf,
    stdout: io::StdoutLock<'static>,
}
impl ClientApplication {
    pub const fn connection_buffer_len() -> usize {
        2 * 1024
    }

    pub fn new(handle: ClientHandle, is_pipped: bool) -> Self {
        static mut STDOUT: Option<io::Stdout> = None;
        let stdout = unsafe {
            STDOUT = Some(io::stdout());
            STDOUT.as_ref().unwrap().lock()
        };

        Self {
            handle,
            is_pipped,
            read_buf: Vec::new(),
            write_buf: SerializationBuf::default(),
            stdout,
        }
    }

    pub fn init<'a>(&'a mut self, args: Args) -> &'a [u8] {
        self.write_buf.clear();

        if let Some(handle) = args.as_client {
            self.handle = handle;
        }

        let mut commands = String::new();
        for path in &args.files {
            let (path, line) = match path.rfind(':') {
                Some(i) => match path[(i + 1)..].parse::<u32>() {
                    Ok(line) => (&path[..i], Some(line.saturating_sub(1))),
                    Err(_) => (&path[..], None),
                },
                None => (&path[..], None),
            };

            use fmt::Write;
            match line {
                Some(line) => writeln!(commands, "open '{}' -line={}", path, line).unwrap(),
                None => writeln!(commands, "open '{}'", path).unwrap(),
            }
        }

        if self.is_pipped {
            use fmt::Write;
            use io::Read;

            let mut buf = Vec::new();
            match std::io::stdin().lock().read_to_end(&mut buf) {
                Ok(_) => match std::str::from_utf8(&buf) {
                    Ok(text) => commands.push_str(text),
                    Err(error) => write!(commands, "print -error {{{}}}", error).unwrap(),
                },
                Err(error) => write!(commands, "print -error {{{}}}", error).unwrap(),
            }
        } else {
            use io::Write;
            self.stdout
                .write_all(ui::ENTER_ALTERNATE_BUFFER_CODE)
                .unwrap();
            self.stdout.write_all(ui::HIDE_CURSOR_CODE).unwrap();
            self.stdout.write_all(ui::MODE_256_COLORS_CODE).unwrap();
            self.stdout.flush().unwrap();

            if args.as_client.is_none() {
                ClientEvent::Key(self.handle, Key::None).serialize(&mut self.write_buf);
            }
        }

        if !commands.is_empty() {
            ClientEvent::Command(self.handle, &commands).serialize(&mut self.write_buf);
        }
        self.write_buf.as_slice()
    }

    pub fn update_with_ui<'a>(
        &'a mut self,
        resize: Option<(usize, usize)>,
        keys: &[Key],
        message: &[u8],
    ) -> &'a [u8] {
        use io::Write;

        self.write_buf.clear();

        if let Some((width, height)) = resize {
            ClientEvent::Resize(self.handle, width as _, height as _)
                .serialize(&mut self.write_buf);
        }

        for key in keys {
            ClientEvent::Key(self.handle, *key).serialize(&mut self.write_buf);
        }

        if !message.is_empty() {
            self.read_buf.extend_from_slice(message);
            let mut deserializer = DeserializationSlice(&self.read_buf);

            loop {
                match ServerEvent::deserialize(&mut deserializer) {
                    Ok(ServerEvent::Display(display)) => {
                        self.stdout.write_all(display).unwrap();
                    }
                    Ok(ServerEvent::CommandOutput(output)) => {
                        // TODO: server command output
                    }
                    Ok(ServerEvent::Request(request)) => {
                        // TODO: server request
                    }
                    Err(DeserializeError::InsufficientData) => {
                        let rest_len = deserializer.0.len();
                        let rest_index = self.read_buf.len() - rest_len;
                        self.read_buf.copy_within(rest_index.., 0);
                        self.read_buf.truncate(rest_len);
                        break;
                    }
                    Err(DeserializeError::InvalidData) => panic!("invalid data received"),
                }
            }

            self.stdout.flush().unwrap();
        }

        self.write_buf.as_slice()
    }
}
impl Drop for ClientApplication {
    fn drop(&mut self) {
        if !self.is_pipped {
            use io::Write;
            let _ = self.stdout.write_all(ui::EXIT_ALTERNATE_BUFFER_CODE);
            let _ = self.stdout.write_all(ui::SHOW_CURSOR_CODE);
            let _ = self.stdout.write_all(ui::RESET_STYLE_CODE);
            let _ = self.stdout.flush();
        }
    }
}
