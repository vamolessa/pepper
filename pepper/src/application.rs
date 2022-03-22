use std::{
    env, fs, io, panic,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    client::ClientManager,
    editor::{Editor, EditorContext, EditorFlow},
    editor_utils::{load_config, MessageKind},
    events::{ClientEvent, ClientEventReceiver, ServerEvent, TargetClient},
    platform::{drop_event, Key, Platform, PlatformEvent, PlatformRequest, ProcessTag},
    plugin::{PluginCollection, PluginDefinition},
    serialization::{DeserializeError, Serialize},
    ui, Args, ResourceFile,
};

#[derive(Default, Clone, Copy)]
pub struct OnPanicConfig {
    pub write_info_to_file: Option<&'static Path>,
    pub try_attaching_debugger: bool,
}

pub struct ApplicationConfig {
    pub args: Args,
    pub plugin_definitions: Vec<PluginDefinition>,
    pub static_configs: Vec<ResourceFile>,
    pub on_panic_config: OnPanicConfig,
}
impl Default for ApplicationConfig {
    fn default() -> Self {
        Self {
            args: Args::parse(),
            static_configs: vec![
                crate::DEFAULT_BINDINGS_CONFIG,
                crate::DEFAULT_ALIASES_CONFIG,
                crate::DEFAULT_SYNTAXES_CONFIG,
            ],
            plugin_definitions: Vec::new(),
            on_panic_config: OnPanicConfig::default(),
        }
    }
}

pub const SERVER_CONNECTION_BUFFER_LEN: usize = 4 * 1024;
pub const SERVER_IDLE_DURATION: Duration = Duration::from_secs(1);

pub struct ServerApplication {
    pub ctx: EditorContext,
    client_event_receiver: ClientEventReceiver,
}
impl ServerApplication {
    pub fn new(config: ApplicationConfig) -> Option<Self> {
        let current_dir = env::current_dir().unwrap_or(PathBuf::new());
        let mut ctx = EditorContext {
            editor: Editor::new(current_dir),
            platform: Platform::default(),
            clients: ClientManager::default(),
            plugins: PluginCollection::default(),
        };

        for definition in config.plugin_definitions {
            PluginCollection::add(&mut ctx, definition);
        }

        for config in &config.static_configs {
            match load_config(&mut ctx, config.name, config.content) {
                EditorFlow::Continue => (),
                _ => return None,
            };
        }

        for config in config.args.configs {
            let path = Path::new(&config.path);
            if config.suppress_file_not_found && !path.exists() {
                continue;
            }
            match fs::read_to_string(path) {
                Ok(source) => match load_config(&mut ctx, &config.path, &source) {
                    EditorFlow::Continue => (),
                    _ => return None,
                },
                Err(_) => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("could not load config '{}'", config.path)),
            }
        }

        Some(Self {
            ctx,
            client_event_receiver: ClientEventReceiver::default(),
        })
    }

    pub fn update<I>(&mut self, mut events: I)
    where
        I: Iterator<Item = PlatformEvent>,
    {
        while let Some(event) = events.next() {
            match event {
                PlatformEvent::Idle => {
                    self.ctx.editor.on_idle();
                    self.ctx.trigger_event_handlers();
                }
                PlatformEvent::ConnectionOpen { handle } => {
                    self.ctx.clients.on_client_joined(handle)
                }
                PlatformEvent::ConnectionClose { handle } => {
                    self.ctx.clients.on_client_left(handle);
                    if self.ctx.clients.iter().next().is_none() {
                        self.ctx.platform.requests.enqueue(PlatformRequest::Quit);

                        for event in events {
                            drop_event(&mut self.ctx.platform.buf_pool, event);
                        }
                        break;
                    }
                }
                PlatformEvent::ConnectionOutput { handle, buf } => {
                    let mut events = self
                        .client_event_receiver
                        .receive_events(handle, buf.as_bytes());
                    self.ctx.platform.buf_pool.release(buf);

                    while let Some(event) = events.next(&self.client_event_receiver) {
                        match Editor::on_client_event(&mut self.ctx, handle, event) {
                            EditorFlow::Continue => (),
                            EditorFlow::Suspend => {
                                let mut buf = self.ctx.platform.buf_pool.acquire();
                                ServerEvent::Suspend.serialize(buf.write());
                                self.ctx
                                    .platform
                                    .requests
                                    .enqueue(PlatformRequest::WriteToClient { handle, buf });
                            }
                            EditorFlow::Quit => {
                                self.ctx
                                    .platform
                                    .requests
                                    .enqueue(PlatformRequest::CloseClient { handle });
                                break;
                            }
                            EditorFlow::QuitAll => {
                                self.ctx.platform.requests.enqueue(PlatformRequest::Quit);
                                break;
                            }
                        }
                    }
                    events.finish(&mut self.client_event_receiver);
                }
                PlatformEvent::ProcessSpawned { tag, handle } => {
                    match tag {
                        ProcessTag::Ignored => (),
                        ProcessTag::Buffer(index) => self.ctx.editor.buffers.on_process_spawned(
                            &mut self.ctx.platform,
                            index,
                            handle,
                        ),
                        ProcessTag::FindFiles => (),
                        ProcessTag::FindPattern => (),
                        ProcessTag::Plugin { plugin_handle, id } => {
                            PluginCollection::on_process_spawned(
                                &mut self.ctx,
                                plugin_handle,
                                id,
                                handle,
                            )
                        }
                    }
                    self.ctx.trigger_event_handlers();
                }
                PlatformEvent::ProcessOutput { tag, buf } => {
                    let bytes = buf.as_bytes();
                    match tag {
                        ProcessTag::Ignored => (),
                        ProcessTag::Buffer(index) => self.ctx.editor.buffers.on_process_output(
                            &mut self.ctx.editor.word_database,
                            index,
                            bytes,
                            &mut self.ctx.editor.events,
                        ),
                        ProcessTag::FindFiles => {
                            self.ctx.editor.mode.picker_state.on_process_output(
                                &mut self.ctx.editor.picker,
                                &self.ctx.editor.read_line,
                                bytes,
                            )
                        }
                        ProcessTag::FindPattern => {
                            self.ctx.editor.mode.read_line_state.on_process_output(
                                &mut self.ctx.editor.buffers,
                                &mut self.ctx.editor.word_database,
                                bytes,
                                &mut self.ctx.editor.events,
                            )
                        }
                        ProcessTag::Plugin { plugin_handle, id } => {
                            PluginCollection::on_process_output(
                                &mut self.ctx,
                                plugin_handle,
                                id,
                                bytes,
                            )
                        }
                    }
                    self.ctx.trigger_event_handlers();
                    self.ctx.platform.buf_pool.release(buf);
                }
                PlatformEvent::ProcessExit { tag } => {
                    match tag {
                        ProcessTag::Ignored => (),
                        ProcessTag::Buffer(index) => self.ctx.editor.buffers.on_process_exit(
                            &mut self.ctx.editor.word_database,
                            index,
                            &mut self.ctx.editor.events,
                        ),
                        ProcessTag::FindFiles => self.ctx.editor.mode.picker_state.on_process_exit(
                            &mut self.ctx.editor.picker,
                            &self.ctx.editor.read_line,
                        ),
                        ProcessTag::FindPattern => {
                            self.ctx.editor.mode.read_line_state.on_process_exit(
                                &mut self.ctx.editor.buffers,
                                &mut self.ctx.editor.word_database,
                                &mut self.ctx.editor.events,
                            )
                        }
                        ProcessTag::Plugin { plugin_handle, id } => {
                            PluginCollection::on_process_exit(&mut self.ctx, plugin_handle, id)
                        }
                    }
                    self.ctx.trigger_event_handlers();
                }
            }
        }

        self.ctx.render();
    }
}

pub const CLIENT_STDIN_BUFFER_LEN: usize = 4 * 1024;
pub const CLIENT_CONNECTION_BUFFER_LEN: usize = 4 * 1024;

pub struct ClientApplication<O>
where
    O: io::Write,
{
    target_client: TargetClient,
    server_read_buf: Vec<u8>,
    server_write_buf: Vec<u8>,
    pub output: Option<O>,
    stdout_buf: Vec<u8>,
}
impl<O> ClientApplication<O>
where
    O: io::Write,
{
    pub fn new() -> Self {
        Self {
            target_client: TargetClient::Sender,
            server_read_buf: Vec::new(),
            server_write_buf: Vec::new(),
            output: None,
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
            ClientEvent::Key(self.target_client, Key::default())
                .serialize(&mut self.server_write_buf);
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
        if let Some(output) = &mut self.output {
            let _ = output.write_all(ui::ENTER_ALTERNATE_BUFFER_CODE);
            let _ = output.write_all(ui::HIDE_CURSOR_CODE);
            let _ = output.write_all(ui::MODE_256_COLORS_CODE);
            let _ = output.flush();
        }
    }

    pub fn restore_screen(&mut self) {
        if let Some(output) = &mut self.output {
            let _ = output.write_all(ui::EXIT_ALTERNATE_BUFFER_CODE);
            let _ = output.write_all(ui::SHOW_CURSOR_CODE);
            let _ = output.write_all(ui::RESET_STYLE_CODE);
            let _ = output.flush();
        }
    }

    pub fn update(
        &mut self,
        resize: Option<(u16, u16)>,
        keys: &[Key],
        stdin_bytes: Option<&[u8]>,
        server_bytes: &[u8],
    ) -> (bool, &'_ [u8]) {
        self.server_write_buf.clear();

        if let Some((width, height)) = resize {
            ClientEvent::Resize(width, height).serialize(&mut self.server_write_buf);
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
impl<O> Drop for ClientApplication<O>
where
    O: io::Write,
{
    fn drop(&mut self) {
        self.restore_screen();
    }
}
