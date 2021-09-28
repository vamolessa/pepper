use std::{env, fs, io, mem::ManuallyDrop, panic, path::Path, time::Duration};

use crate::{
    client::ClientManager,
    editor::{Editor, EditorControlFlow},
    editor_utils::{load_config, MessageKind},
    events::{ClientEvent, ClientEventReceiver, ServerEvent, TargetClient},
    platform::{Key, Platform, PlatformEvent, PlatformRequest},
    serialization::{DeserializeError, Serialize},
    ui, Args,
};

pub struct ServerApplication {
    editor: Editor,
    pub platform: Platform,
    clients: ClientManager,
    client_event_receiver: ClientEventReceiver,
}
impl ServerApplication {
    pub const fn connection_buffer_len() -> usize {
        512
    }

    pub const fn idle_duration() -> Duration {
        Duration::from_secs(1)
    }

    pub fn new(args: Args) -> Option<Self> {
        let current_dir = env::current_dir().expect("could not retrieve the current directory");
        let mut editor = Editor::new(current_dir);
        let mut platform = Platform::default();
        let mut clients = ClientManager::default();

        if !args.no_default_config {
            let source = include_str!("../rc/default_config.pp");
            load_config(
                &mut editor,
                &mut platform,
                &mut clients,
                "default_config.pp",
                source,
            );
        }

        for config in args.configs {
            let path = Path::new(&config.path);
            if config.suppress_file_not_found && !path.exists() {
                continue;
            }
            match fs::read_to_string(path) {
                Ok(source) => match load_config(
                    &mut editor,
                    &mut platform,
                    &mut clients,
                    &config.path,
                    &source,
                ) {
                    EditorControlFlow::Continue => (),
                    _ => return None,
                },
                Err(_) => editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("could not load config '{}'", config.path)),
            }
        }

        Some(Self {
            editor,
            platform,
            clients,
            client_event_receiver: ClientEventReceiver::default(),
        })
    }

    pub fn update<I>(&mut self, events: I)
    where
        I: Iterator<Item = PlatformEvent>,
    {
        for event in events {
            match event {
                PlatformEvent::Idle => self.editor.on_idle(&mut self.clients, &mut self.platform),
                PlatformEvent::ConnectionOpen { handle } => self.clients.on_client_joined(handle),
                PlatformEvent::ConnectionClose { handle } => {
                    self.clients.on_client_left(handle);
                    if self.clients.iter().next().is_none() {
                        self.platform.requests.enqueue(PlatformRequest::Quit);
                        break;
                    }
                }
                PlatformEvent::ConnectionOutput { handle, buf } => {
                    let mut events = self
                        .client_event_receiver
                        .receive_events(handle, buf.as_bytes());
                    self.platform.buf_pool.release(buf);

                    while let Some(event) = events.next(&self.client_event_receiver) {
                        match self.editor.on_client_event(
                            &mut self.platform,
                            &mut self.clients,
                            handle,
                            event,
                        ) {
                            EditorControlFlow::Continue => (),
                            EditorControlFlow::Suspend => {
                                let mut buf = self.platform.buf_pool.acquire();
                                ServerEvent::Suspend.serialize(buf.write());
                                self.platform
                                    .requests
                                    .enqueue(PlatformRequest::WriteToClient { handle, buf });
                            }
                            EditorControlFlow::Quit => {
                                self.platform
                                    .requests
                                    .enqueue(PlatformRequest::CloseClient { handle });
                                break;
                            }
                            EditorControlFlow::QuitAll => {
                                self.platform.requests.enqueue(PlatformRequest::Quit);
                                break;
                            }
                        }
                    }
                    events.finish(&mut self.client_event_receiver);
                }
                PlatformEvent::ProcessSpawned { tag, handle } => {
                    self.editor
                        .on_process_spawned(&mut self.platform, tag, handle)
                }
                PlatformEvent::ProcessOutput { tag, buf } => {
                    self.editor.on_process_output(
                        &mut self.platform,
                        &mut self.clients,
                        tag,
                        buf.as_bytes(),
                    );
                    self.platform.buf_pool.release(buf);
                }
                PlatformEvent::ProcessExit { tag } => {
                    self.editor
                        .on_process_exit(&mut self.platform, &mut self.clients, tag)
                }
            }
        }

        let needs_redraw = self.editor.on_pre_render(&mut self.clients);
        if needs_redraw {
            self.platform.requests.enqueue(PlatformRequest::Redraw);
        }

        let focused_client_handle = self.clients.focused_client();
        for c in self.clients.iter() {
            if !c.has_ui() {
                continue;
            }

            let mut buf = self.platform.buf_pool.acquire();
            let write = buf.write_with_len(ServerEvent::bytes_variant_header_len());
            let ctx = ui::RenderContext {
                editor: &self.editor,
                clients: &self.clients,
                viewport_size: c.viewport_size,
                scroll: c.scroll,
                draw_height: c.height,
                has_focus: focused_client_handle == Some(c.handle()),
            };
            ui::render(&ctx, c.buffer_view_handle(), write);
            ServerEvent::Display(&[]).serialize_bytes_variant_header(write);

            let handle = c.handle();
            self.platform
                .requests
                .enqueue(PlatformRequest::WriteToClient { handle, buf });
        }
    }
}

pub struct ClientApplication {
    target_client: TargetClient,
    server_read_buf: Vec<u8>,
    server_write_buf: Vec<u8>,
    output: Option<ManuallyDrop<fs::File>>,
    stdout_buf: Vec<u8>,
}
impl ClientApplication {
    pub const fn stdin_buffer_len() -> usize {
        4 * 1024
    }

    pub const fn connection_buffer_len() -> usize {
        48 * 1024
    }

    pub fn new(output: Option<ManuallyDrop<fs::File>>) -> Self {
        Self {
            target_client: TargetClient::Sender,
            server_read_buf: Vec::new(),
            server_write_buf: Vec::new(),
            output,
            stdout_buf: Vec::new(),
        }
    }

    pub fn init(&mut self, args: Args) -> &[u8] {
        if args.as_focused_client {
            self.target_client = TargetClient::Focused;
        }

        self.server_write_buf.clear();

        self.reinit_screen();
        if !args.quit && !args.as_focused_client {
            ClientEvent::Key(self.target_client, Key::None).serialize(&mut self.server_write_buf);
        }

        let mut commands = String::new();
        for path in &args.files {
            commands.clear();
            commands.push_str("open \"");
            commands.push_str(path);
            commands.push('"');
            ClientEvent::Command(self.target_client, &commands)
                .serialize(&mut self.server_write_buf);
        }

        if args.quit {
            ClientEvent::Command(TargetClient::Sender, "quit")
                .serialize(&mut self.server_write_buf);
        }

        self.server_write_buf.as_slice()
    }

    pub fn reinit_screen(&mut self) {
        use io::Write;
        if let Some(output) = &mut self.output {
            let _ = output.write_all(ui::ENTER_ALTERNATE_BUFFER_CODE);
            let _ = output.write_all(ui::HIDE_CURSOR_CODE);
            let _ = output.write_all(ui::MODE_256_COLORS_CODE);
            output.flush().unwrap();
        }
    }

    pub fn restore_screen(&mut self) {
        use io::Write;
        if let Some(output) = &mut self.output {
            let _ = output.write_all(ui::EXIT_ALTERNATE_BUFFER_CODE);
            let _ = output.write_all(ui::SHOW_CURSOR_CODE);
            let _ = output.write_all(ui::RESET_STYLE_CODE);
            let _ = output.flush();
        }
    }

    pub fn update<'a>(
        &'a mut self,
        resize: Option<(usize, usize)>,
        keys: &[Key],
        stdin_bytes: Option<&[u8]>,
        server_bytes: &[u8],
    ) -> (bool, &'a [u8]) {
        use io::Write;

        self.server_write_buf.clear();

        if let Some((width, height)) = resize {
            ClientEvent::Resize(width as _, height as _).serialize(&mut self.server_write_buf);
        }

        for key in keys {
            ClientEvent::Key(self.target_client, *key).serialize(&mut self.server_write_buf);
        }

        if let Some(bytes) = stdin_bytes {
            ClientEvent::StdinInput(self.target_client, bytes)
                .serialize(&mut self.server_write_buf);
        }

        let mut suspend = false;
        if !server_bytes.is_empty() {
            self.server_read_buf.extend_from_slice(server_bytes);
            let mut read_slice = &self.server_read_buf[..];

            loop {
                let previous_slice = read_slice;
                match ServerEvent::deserialize(&mut read_slice) {
                    Ok(ServerEvent::Display(display)) => {
                        if let Some(output) = &mut self.output {
                            output.write_all(display).unwrap();
                        }
                    }
                    Ok(ServerEvent::Suspend) => suspend = true,
                    Ok(ServerEvent::StdoutOutput(bytes)) => {
                        self.stdout_buf.clear();
                        self.stdout_buf.extend_from_slice(bytes);
                    }
                    Err(DeserializeError::InsufficientData) => {
                        let read_len = self.server_read_buf.len() - previous_slice.len();
                        self.server_read_buf.drain(..read_len);
                        break;
                    }
                    Err(DeserializeError::InvalidData) => {
                        panic!("client received invalid data from server")
                    }
                }
            }

            if let Some(output) = &mut self.output {
                output.flush().unwrap();
            }
        }

        (suspend, self.server_write_buf.as_slice())
    }

    pub fn get_stdout_bytes(&self) -> &[u8] {
        &self.stdout_buf
    }
}
impl Drop for ClientApplication {
    fn drop(&mut self) {
        self.restore_screen();
    }
}
