use std::{env, io, sync::mpsc};

use crate::{
    client::{ClientHandle, ClientManager},
    client_event::{ClientEvent, ClientEventSource},
    command::CommandOperation,
    connection::ClientEventDeserializationBufCollection,
    editor::{Editor, EditorLoop},
    lsp,
    platform::{ConnectionHandle, Key, Platform, PlatformRequest, ProcessHandle, SharedBuf},
    serialization::{SerializationBuf, Serialize},
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
        handle: ConnectionHandle,
    },
    ConnectionClose {
        handle: ConnectionHandle,
    },
    ConnectionMessage {
        handle: ConnectionHandle,
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
            if let Some(CommandOperation::Quit) | Some(CommandOperation::QuitAll) =
                editor.load_config(&mut platform, &mut clients, config)
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
        let mut event_deserialization_bufs = ClientEventDeserializationBufCollection::default();

        'event_loop: loop {
            let mut event = event_receiver.recv()?;
            loop {
                match event {
                    ApplicationEvent::Idle => editor.on_idle(&mut clients),
                    ApplicationEvent::Redraw => (),
                    ApplicationEvent::ConnectionOpen { handle } => {
                        if let Some(client_handle) = ClientHandle::from_index(handle.0) {
                            clients.on_client_joined(client_handle);
                        }
                    }
                    ApplicationEvent::ConnectionClose { handle } => {
                        if let Some(client_handle) = ClientHandle::from_index(handle.0) {
                            clients.on_client_left(client_handle);
                            if clients.iter_mut().next().is_none() {
                                break 'event_loop;
                            }
                        }
                    }
                    ApplicationEvent::ConnectionMessage { handle, buf } => {
                        let client_handle = match ClientHandle::from_index(handle.0) {
                            Some(handle) => handle,
                            None => break 'event_loop,
                        };

                        let mut events = event_deserialization_bufs
                            .receive_events(client_handle, buf.as_bytes());
                        while let Some(event) = events.next() {
                            match editor.on_client_event(
                                platform,
                                &mut clients,
                                client_handle,
                                event,
                            ) {
                                EditorLoop::Continue => (),
                                EditorLoop::Quit => {
                                    platform.enqueue_request(PlatformRequest::CloseConnection {
                                        handle,
                                    });
                                    break;
                                }
                                EditorLoop::QuitAll => break 'event_loop,
                            }
                        }
                    }
                    ApplicationEvent::ProcessSpawned { tag, handle } => match tag {
                        ProcessTag::Lsp(client_handle) => {
                            editor.on_lsp_process_spawned(client_handle, handle)
                        }
                        _ => (),
                    },
                    ApplicationEvent::ProcessStdout { tag, buf } => {
                        let bytes = buf.as_bytes();
                        match tag {
                            ProcessTag::Lsp(client_handle) => {
                                editor.on_lsp_process_stdout(client_handle, bytes)
                            }
                            _ => (),
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
                            editor.on_lsp_process_exit(client_handle, success)
                        }
                        _ => (),
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
                let has_focus = focused_client_handle == Some(c.handle());

                let mut buf = platform.buf_pool.acquire();
                let render_buf = buf.write();
                render_buf.clear();
                render_buf.extend_from_slice(&[0; 4]);
                ui::render(
                    &editor,
                    c.buffer_view_handle(),
                    c.viewport_size.0 as _,
                    c.viewport_size.1 as _,
                    c.scroll,
                    has_focus,
                    render_buf,
                    &mut c.status_bar_buffer,
                );

                let len = render_buf.len() as u32 - 4;
                let len_bytes = len.to_le_bytes();
                render_buf[..4].copy_from_slice(&len_bytes);

                let handle = c.connection_handle();
                let buf = buf.share();
                platform.buf_pool.release(buf.clone());
                platform.enqueue_request(PlatformRequest::WriteToConnection { handle, buf });
            }

            platform.flush_requests();
        }

        Ok(())
    }
}

pub struct ClientApplication {
    client_event_source: ClientEventSource,
    read_buf: Vec<u8>,
    write_buf: SerializationBuf,
    stdout: io::StdoutLock<'static>,
}
impl ClientApplication {
    pub const fn connection_buffer_len() -> usize {
        2 * 1024
    }

    pub fn new() -> Self {
        static mut STDOUT: Option<io::Stdout> = None;
        let mut stdout = unsafe {
            STDOUT = Some(io::stdout());
            STDOUT.as_ref().unwrap().lock()
        };

        use io::Write;
        let _ = stdout.write_all(ui::ENTER_ALTERNATE_BUFFER_CODE);
        let _ = stdout.write_all(ui::HIDE_CURSOR_CODE);
        let _ = stdout.write_all(ui::MODE_256_COLORS_CODE);
        let _ = stdout.flush();

        Self {
            client_event_source: ClientEventSource::ConnectionClient,
            read_buf: Vec::new(),
            write_buf: SerializationBuf::default(),
            stdout,
        }
    }

    pub fn init<'a>(&'a mut self, args: Args) -> &'a [u8] {
        self.client_event_source = if args.as_focused_client {
            ClientEventSource::FocusedClient
        } else if let Some(handle) = args.as_client {
            ClientEventSource::ClientHandle(handle)
        } else {
            ClientEventSource::ConnectionClient
        };

        self.write_buf.clear();

        if !args.files.is_empty() {
            let mut open_buffers_command = String::new();
            open_buffers_command.push_str("open");

            for path in &args.files {
                open_buffers_command.push_str(" '");
                open_buffers_command.push_str(path);
                open_buffers_command.push_str("'");
            }

            ClientEvent::Command(self.client_event_source, &open_buffers_command)
                .serialize(&mut self.write_buf);
        }

        self.write_buf.as_slice()
    }

    pub fn update<'a>(
        &'a mut self,
        resize: Option<(usize, usize)>,
        keys: &[Key],
        message: &[u8],
    ) -> &'a [u8] {
        use io::Write;

        self.write_buf.clear();

        if let Some((width, height)) = resize {
            ClientEvent::Resize(self.client_event_source, width as _, height as _)
                .serialize(&mut self.write_buf);
        }

        for key in keys {
            ClientEvent::Key(self.client_event_source, *key).serialize(&mut self.write_buf);
        }

        if !message.is_empty() {
            self.read_buf.extend_from_slice(message);
            let mut len_bytes = [0; 4];

            if self.read_buf.len() >= len_bytes.len() {
                len_bytes.copy_from_slice(&self.read_buf[..4]);
                let message_len = u32::from_le_bytes(len_bytes) as usize;

                if self.read_buf.len() >= message_len + 4 {
                    self.read_buf.extend_from_slice(ui::RESET_STYLE_CODE);
                    self.stdout.write_all(&self.read_buf[4..]).unwrap();
                    self.read_buf.clear();
                }
            }
        }

        self.stdout.flush().unwrap();
        self.write_buf.as_slice()
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
