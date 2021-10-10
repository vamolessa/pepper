use std::{env, fs, io, mem::ManuallyDrop, panic, path::Path, time::Duration};

use crate::{
    client::ClientManager,
    editor::{ApplicationContext, Editor, EditorControlFlow},
    editor_utils::{load_config, MessageKind},
    events::{ClientEvent, ClientEventReceiver, ServerEvent, TargetClient},
    help,
    platform::{Key, Platform, PlatformEvent, PlatformRequest, ProcessTag},
    plugin::{PluginCollection, PluginContext, PluginDefinition},
    serialization::{DeserializeError, Serialize},
    ui, Args, ResourceFile,
};

#[derive(Default, Clone, Copy)]
pub struct OnPanicConfig {
    pub write_info_to_file: bool,
    pub try_attaching_debugger: bool,
}

// TODO: rename bake to ApplicationContext
pub struct ApplicationConfig {
    pub args: Args,
    pub configs: Vec<ResourceFile>,
    pub plugin_definitions: Vec<&'static dyn PluginDefinition>,
    pub on_panic_config: OnPanicConfig,
}
impl Default for ApplicationConfig {
    fn default() -> Self {
        Self {
            args: Args::parse(),
            configs: vec![
                crate::DEFAULT_BINDINGS_CONFIG,
                crate::DEFAULT_SYNTAXES_CONFIG,
            ],
            plugin_definitions: Vec::new(),
            on_panic_config: OnPanicConfig::default(),
        }
    }
}

pub(crate) struct ServerApplication {
    pub ctx: ApplicationContext,
    client_event_receiver: ClientEventReceiver,
}
impl ServerApplication {
    pub const fn connection_buffer_len() -> usize {
        512
    }

    pub const fn idle_duration() -> Duration {
        Duration::from_secs(1)
    }

    pub fn new(config: ApplicationConfig) -> Option<Self> {
        let current_dir = env::current_dir().expect("could not retrieve the current directory");
        let mut ctx = ApplicationContext {
            editor: Editor::new(current_dir),
            platform: Platform::default(),
            clients: ClientManager::default(),
            plugins: PluginCollection::default(),
        };

        for definition in config.plugin_definitions {
            help::add_help_pages(definition.help_pages());

            let plugin_handle = ctx.plugins.next_handle();
            let mut plugin_ctx = PluginContext {
                editor: &mut ctx.editor,
                platform: &mut ctx.platform,
                clients: &mut ctx.clients,
                plugin_handle,
            };
            let plugin = definition.instantiate(&mut plugin_ctx);
            ctx.plugins.add(plugin);
        }

        for config in &config.configs {
            match load_config(
                &mut ctx,
                config.name,
                config.content,
            ) {
                EditorControlFlow::Continue => (),
                _ => return None,
            };
        }

        for config in config.args.configs {
            let path = Path::new(&config.path);
            if config.suppress_file_not_found && !path.exists() {
                continue;
            }
            match fs::read_to_string(path) {
                Ok(source) => match load_config(
                    &mut ctx,
                    &config.path,
                    &source,
                ) {
                    EditorControlFlow::Continue => (),
                    _ => return None,
                },
                Err(_) => ctx.editor
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

    pub fn update<I>(&mut self, events: I)
    where
        I: Iterator<Item = PlatformEvent>,
    {
        for event in events {
            match event {
                PlatformEvent::Idle => {
                    self.ctx.editor.on_idle();
                    self.ctx.trigger_event_handlers();
                }
                PlatformEvent::ConnectionOpen { handle } => self.ctx.clients.on_client_joined(handle),
                PlatformEvent::ConnectionClose { handle } => {
                    self.ctx.clients.on_client_left(handle);
                    if self.ctx.clients.iter().next().is_none() {
                        self.ctx.platform.requests.enqueue(PlatformRequest::Quit);
                        break;
                    }
                }
                PlatformEvent::ConnectionOutput { handle, buf } => {
                    let mut events = self
                        .client_event_receiver
                        .receive_events(handle, buf.as_bytes());
                    self.ctx.platform.buf_pool.release(buf);

                    while let Some(event) = events.next(&self.client_event_receiver) {
                        match Editor::on_client_event(
                            &mut self.ctx,
                            handle,
                            event,
                        ) {
                            EditorControlFlow::Continue => (),
                            EditorControlFlow::Suspend => {
                                let mut buf = self.ctx.platform.buf_pool.acquire();
                                ServerEvent::Suspend.serialize(buf.write());
                                self.ctx.platform
                                    .requests
                                    .enqueue(PlatformRequest::WriteToClient { handle, buf });
                            }
                            EditorControlFlow::Quit => {
                                self.ctx.platform
                                    .requests
                                    .enqueue(PlatformRequest::CloseClient { handle });
                                break;
                            }
                            EditorControlFlow::QuitAll => {
                                self.ctx.platform.requests.enqueue(PlatformRequest::Quit);
                                break;
                            }
                        }
                    }
                    events.finish(&mut self.client_event_receiver);
                }
                PlatformEvent::ProcessSpawned { tag, handle } => {
                    match tag {
                        ProcessTag::Buffer(id) => self.ctx.editor.buffers.on_process_spawned(&mut self.ctx.platform, id, handle),
                        ProcessTag::FindFiles => (),
                        ProcessTag::Plugin(id) => {
                            PluginCollection::on_process_spawned(&mut self.ctx, id, handle)
                        }
                    }
                    self.ctx.trigger_event_handlers();
                }
                PlatformEvent::ProcessOutput { tag, buf } => {
                    let bytes = buf.as_bytes();
                    match tag {
                        ProcessTag::Buffer(id) => self.ctx.editor.buffers.on_process_output(
                            &mut self.ctx.editor.word_database,
                            id,
                            bytes,
                            &mut self.ctx.editor.events,
                        ),
                        ProcessTag::FindFiles => {
                            self.ctx.editor.mode
                                .picker_state
                                .on_process_output(&mut self.ctx.editor.picker, &self.ctx.editor.read_line, bytes)
                        }
                        ProcessTag::Plugin(id) => {
                            PluginCollection::on_process_output(
                                &mut self.ctx,
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
                        ProcessTag::Buffer(id) => self.ctx.editor.buffers.on_process_exit(
                            &mut self.ctx.editor.word_database,
                            id,
                            &mut self.ctx.editor.events,
                        ),
                        ProcessTag::FindFiles => self
                            .ctx
                            .editor
                            .mode
                            .picker_state
                            .on_process_exit(&mut self.ctx.editor.picker, &self.ctx.editor.read_line),
                        ProcessTag::Plugin(id) => PluginCollection::on_process_exit(
                            &mut self.ctx,
                            id,
                        ),
                    }
                    self.ctx.trigger_event_handlers();
                }
            }
        }

        let needs_redraw = self.ctx.editor.on_pre_render(&mut self.ctx.clients);
        if needs_redraw {
            self.ctx.platform.requests.enqueue(PlatformRequest::Redraw);
        }

        let focused_client_handle = self.ctx.clients.focused_client();
        for c in self.ctx.clients.iter() {
            if !c.has_ui() {
                continue;
            }

            let mut buf = self.ctx.platform.buf_pool.acquire();
            let write = buf.write_with_len(ServerEvent::bytes_variant_header_len());
            let ctx = ui::RenderContext {
                editor: &self.ctx.editor,
                clients: &self.ctx.clients,
                viewport_size: c.viewport_size,
                scroll: c.scroll,
                draw_height: c.height,
                has_focus: focused_client_handle == Some(c.handle()),
            };
            ui::render(&ctx, c.buffer_view_handle(), write);
            ServerEvent::Display(&[]).serialize_bytes_variant_header(write);

            let handle = c.handle();
            self.ctx.platform
                .requests
                .enqueue(PlatformRequest::WriteToClient { handle, buf });
        }
    }
}

pub(crate) struct ClientApplication {
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

