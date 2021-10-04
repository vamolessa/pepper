use std::{
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    buffer::{BufferCapabilities, BufferCollection, BufferReadError},
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::{Client, ClientHandle, ClientManager},
    command::CommandManager,
    config::Config,
    editor_utils::{ReadLine, StatusBar, StringPool},
    events::{
        ClientEvent, EditorEvent, EditorEventIter, EditorEventQueue, KeyParseAllError, KeyParser,
        ServerEvent, TargetClient,
    },
    keymap::{KeyMapCollection, MatchResult},
    lsp,
    mode::{Mode, ModeContext, ModeKind},
    pattern::Pattern,
    picker::Picker,
    platform::{Key, Platform, PlatformRequest, ProcessHandle, ProcessTag},
    plugin::PluginCollection,
    register::{RegisterCollection, RegisterKey},
    syntax::{HighlightResult, SyntaxCollection},
    theme::Theme,
    word_database::WordDatabase,
};

#[derive(Clone, Copy)]
pub enum EditorControlFlow {
    Continue,
    Suspend,
    Quit,
    QuitAll,
}

pub struct KeysIterator {
    pub index: usize,
}
impl KeysIterator {
    pub fn next(&mut self, keys: &BufferedKeys) -> Key {
        if self.index < keys.0.len() {
            let next = keys.0[self.index];
            self.index += 1;
            next
        } else {
            Key::None
        }
    }
}

pub struct BufferedKeysParseError<'a> {
    pub keys: &'a str,
    pub error: KeyParseAllError,
}
impl<'a> fmt::Display for BufferedKeysParseError<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "error parsing keys '{}'\n{}", self.keys, self.error)
    }
}

#[derive(Default)]
pub struct BufferedKeys(Vec<Key>);
impl BufferedKeys {
    pub fn as_slice(&self) -> &[Key] {
        &self.0
    }

    pub fn parse<'a>(&mut self, keys: &'a str) -> Result<KeysIterator, BufferedKeysParseError<'a>> {
        let index = self.as_slice().len();
        for key in KeyParser::new(keys) {
            match key {
                Ok(key) => self.0.push(key),
                Err(error) => {
                    self.0.truncate(index);
                    return Err(BufferedKeysParseError { keys, error });
                }
            }
        }

        Ok(KeysIterator { index })
    }
}

pub struct Editor {
    pub current_directory: PathBuf,
    pub config: Config,
    pub theme: Theme,
    pub syntaxes: SyntaxCollection,
    pub keymaps: KeyMapCollection,

    pub mode: Mode,
    pub buffers: BufferCollection,
    pub buffer_views: BufferViewCollection,
    pub word_database: WordDatabase,

    pub buffered_keys: BufferedKeys,
    pub recording_macro: Option<RegisterKey>,
    pub registers: RegisterCollection,
    pub read_line: ReadLine,
    pub picker: Picker,
    pub string_pool: StringPool,

    pub status_bar: StatusBar,
    pub aux_pattern: Pattern,

    pub commands: CommandManager,
    pub plugins: PluginCollection,
    pub lsp: lsp::ClientManager,
    pub events: EditorEventQueue,
}
impl Editor {
    pub fn new(current_directory: PathBuf) -> Self {
        Self {
            current_directory,
            config: Config::default(),
            theme: Theme::default(),
            syntaxes: SyntaxCollection::new(),
            keymaps: KeyMapCollection::default(),

            mode: Mode::default(),

            buffers: BufferCollection::default(),
            buffer_views: BufferViewCollection::default(),
            word_database: WordDatabase::new(),

            buffered_keys: BufferedKeys::default(),
            recording_macro: None,
            registers: RegisterCollection::new(),
            read_line: ReadLine::default(),
            picker: Picker::default(),
            string_pool: StringPool::default(),

            status_bar: StatusBar::new(),
            aux_pattern: Pattern::new(),

            commands: CommandManager::new(),
            plugins: PluginCollection::default(),
            lsp: lsp::ClientManager::new(),
            events: EditorEventQueue::default(),
        }
    }

    pub fn buffer_view_handle_from_path(
        &mut self,
        client_handle: ClientHandle,
        path: &Path,
        capabilities: BufferCapabilities,
    ) -> Result<BufferViewHandle, BufferReadError> {
        if let Some(buffer_handle) = self.buffers.find_with_path(&self.current_directory, path) {
            let handle = self
                .buffer_views
                .buffer_view_handle_from_buffer_handle(client_handle, buffer_handle);
            Ok(handle)
        } else {
            let path = path.strip_prefix(&self.current_directory).unwrap_or(path);
            let buffer = self.buffers.add_new();
            buffer.path.clear();
            buffer.path.push(path);
            buffer.capabilities = capabilities;

            match buffer.read_from_file(&mut self.word_database, &mut self.events) {
                Ok(()) => {
                    let handle = self.buffer_views.add_new(client_handle, buffer.handle());
                    Ok(handle)
                }
                Err(error) => {
                    let handle = buffer.handle();
                    self.buffers.defer_remove(handle, &mut self.events);
                    Err(error)
                }
            }
        }
    }

    pub fn execute_keys(
        &mut self,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        mut keys: KeysIterator,
    ) -> EditorControlFlow {
        let start_index = keys.index;

        match self
            .keymaps
            .matches(self.mode.kind(), &self.buffered_keys.0[start_index..])
        {
            MatchResult::None => (),
            MatchResult::Prefix => return EditorControlFlow::Continue,
            MatchResult::ReplaceWith(replaced_keys) => {
                self.buffered_keys.0.truncate(start_index);
                self.buffered_keys.0.extend_from_slice(replaced_keys);
            }
        }

        loop {
            if keys.index == self.buffered_keys.0.len() {
                break;
            }
            let from_index = self.recording_macro.map(|_| keys.index);

            let mut ctx = ModeContext {
                editor: self,
                platform,
                clients,
                client_handle,
            };
            match Mode::on_client_keys(&mut ctx, &mut keys) {
                None => return EditorControlFlow::Continue,
                Some(EditorControlFlow::Continue) => (),
                Some(flow) => {
                    Mode::change_to(&mut ctx, ModeKind::default());
                    self.buffered_keys.0.truncate(start_index);
                    return flow;
                }
            }

            if let (Some(from_index), Some(register_key)) = (from_index, self.recording_macro) {
                for key in &self.buffered_keys.0[from_index..keys.index] {
                    use fmt::Write;
                    let register = self.registers.get_mut(register_key);
                    let _ = write!(register, "{}", key);
                }
            }

            self.trigger_event_handlers(platform, clients);
        }

        self.buffered_keys.0.truncate(start_index);
        EditorControlFlow::Continue
    }

    pub fn on_pre_render(&mut self, clients: &mut ClientManager) -> bool {
        let picker_height = self
            .picker
            .update_scroll(self.config.picker_max_height as _);

        let mut needs_redraw = false;
        let focused_handle = clients.focused_client();

        for c in clients.iter_mut() {
            let picker_height = if focused_handle == Some(c.handle()) {
                picker_height as _
            } else {
                0
            };

            if let Some(handle) = c.buffer_view_handle() {
                let buffer_view = self.buffer_views.get(handle);
                let buffer = self.buffers.get_mut(buffer_view.buffer_handle);
                if let HighlightResult::Pending = buffer.update_highlighting(&self.syntaxes) {
                    needs_redraw = true;
                }
            }

            c.update_view(self, picker_height);
        }

        needs_redraw
    }

    pub fn on_client_event(
        &mut self,
        platform: &mut Platform,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        event: ClientEvent,
    ) -> EditorControlFlow {
        match event {
            ClientEvent::Key(target, key) => {
                let client_handle = match target {
                    TargetClient::Sender => client_handle,
                    TargetClient::Focused => match clients.focused_client() {
                        Some(handle) => handle,
                        None => return EditorControlFlow::Continue,
                    },
                };

                if clients.focus_client(client_handle) {
                    self.recording_macro = None;
                    self.buffered_keys.0.clear();

                    if self.mode.kind() == ModeKind::Insert {
                        let mut ctx = ModeContext {
                            editor: self,
                            platform,
                            clients,
                            client_handle,
                        };
                        Mode::change_to(&mut ctx, ModeKind::default());
                    }
                }

                if key != Key::None {
                    self.status_bar.clear();
                }
                self.buffered_keys.0.push(key);
                self.execute_keys(platform, clients, client_handle, KeysIterator { index: 0 })
            }
            ClientEvent::Resize(width, height) => {
                let client = clients.get_mut(client_handle);
                client.viewport_size = (width, height);
                EditorControlFlow::Continue
            }
            ClientEvent::Command(target, command) => {
                let client_handle = match target {
                    TargetClient::Sender => client_handle,
                    TargetClient::Focused => match clients.focused_client() {
                        Some(handle) => handle,
                        None => return EditorControlFlow::Continue,
                    },
                };

                let mut command = self.string_pool.acquire_with(command);
                let flow = CommandManager::eval_and_write_error(
                    self,
                    platform,
                    clients,
                    Some(client_handle),
                    &mut command,
                );
                self.string_pool.release(command);
                flow
            }
            ClientEvent::StdinInput(target, bytes) => {
                let client_handle = match target {
                    TargetClient::Sender => client_handle,
                    TargetClient::Focused => match clients.focused_client() {
                        Some(handle) => handle,
                        None => return EditorControlFlow::Continue,
                    },
                };

                clients.get_mut(client_handle).on_stdin_input(self, bytes);
                self.trigger_event_handlers(platform, clients);
                EditorControlFlow::Continue
            }
        }
    }

    pub fn on_idle(&mut self, clients: &mut ClientManager, platform: &mut Platform) {
        self.events.enqueue(EditorEvent::Idle);
        self.trigger_event_handlers(platform, clients);
    }

    pub fn on_process_spawned(
        &mut self,
        platform: &mut Platform,
        clients: &mut ClientManager,
        tag: ProcessTag,
        handle: ProcessHandle,
    ) {
        match tag {
            ProcessTag::Buffer(index) => self.buffers.on_process_spawned(platform, index, handle),
            ProcessTag::FindFiles => (),
            ProcessTag::Plugin { plugin_handle, index } => PluginCollection::on_process_spawned(
                self,
                platform,
                clients,
                plugin_handle,
                index,
                handle,
            ),
            ProcessTag::Lsp(client_handle) => {
                lsp::ClientManager::on_process_spawned(self, platform, client_handle, handle)
            }
        }
    }

    pub fn on_process_output(
        &mut self,
        platform: &mut Platform,
        clients: &mut ClientManager,
        tag: ProcessTag,
        bytes: &[u8],
    ) {
        match tag {
            ProcessTag::Buffer(index) => self.buffers.on_process_output(
                &mut self.word_database,
                index,
                bytes,
                &mut self.events,
            ),
            ProcessTag::FindFiles => {
                self.mode
                    .picker_state
                    .on_process_output(&mut self.picker, &self.read_line, bytes)
            }
            ProcessTag::Plugin { plugin_handle, index } => PluginCollection::on_process_output(
                self,
                platform,
                clients,
                plugin_handle,
                index,
                bytes,
            ),
            ProcessTag::Lsp(client_handle) => {
                lsp::ClientManager::on_process_output(self, platform, clients, client_handle, bytes)
            }
        }

        self.trigger_event_handlers(platform, clients);
    }

    pub fn on_process_exit(
        &mut self,
        platform: &mut Platform,
        clients: &mut ClientManager,
        tag: ProcessTag,
    ) {
        match tag {
            ProcessTag::Buffer(index) => {
                self.buffers
                    .on_process_exit(&mut self.word_database, index, &mut self.events)
            }
            ProcessTag::FindFiles => self
                .mode
                .picker_state
                .on_process_exit(&mut self.picker, &self.read_line),
            ProcessTag::Plugin { plugin_handle, index } => {
                PluginCollection::on_process_exit(self, platform, clients, plugin_handle, index)
            }
            ProcessTag::Lsp(client_handle) => {
                lsp::ClientManager::on_process_exit(self, client_handle)
            }
        }

        self.trigger_event_handlers(platform, clients);
    }

    pub fn trigger_event_handlers(&mut self, platform: &mut Platform, clients: &mut ClientManager) {
        loop {
            self.events.flip();
            let mut events = EditorEventIter::new();
            if events.next(&self.events).is_none() {
                return;
            }

            lsp::ClientManager::on_editor_events(self, platform);

            let mut events = EditorEventIter::new();
            while let Some(event) = events.next(&self.events) {
                match *event {
                    EditorEvent::Idle => (),
                    EditorEvent::BufferRead { handle } => {
                        let buffer = self.buffers.get_mut(handle);
                        buffer.refresh_syntax(&self.syntaxes);
                        self.buffer_views.on_buffer_load(buffer);
                    }
                    EditorEvent::BufferInsertText { handle, range, .. } => {
                        self.buffer_views.on_buffer_insert_text(handle, range);
                    }
                    EditorEvent::BufferDeleteText { handle, range } => {
                        self.buffer_views.on_buffer_delete_text(handle, range);
                    }
                    EditorEvent::BufferWrite { handle, new_path } => {
                        let buffer = self.buffers.get_mut(handle);
                        if new_path {
                            buffer.refresh_syntax(&self.syntaxes);
                        }

                        for client in clients.iter() {
                            if client.stdin_buffer_handle() == Some(buffer.handle()) {
                                let mut buf = platform.buf_pool.acquire();
                                let write =
                                    buf.write_with_len(ServerEvent::bytes_variant_header_len());
                                let content = buffer.content();
                                let range =
                                    BufferRange::between(BufferPosition::zero(), content.end());
                                for text in content.text_range(range) {
                                    write.extend_from_slice(text.as_bytes());
                                }
                                ServerEvent::StdoutOutput(&[])
                                    .serialize_bytes_variant_header(write);

                                let handle = client.handle();
                                platform
                                    .requests
                                    .enqueue(PlatformRequest::WriteToClient { handle, buf });
                                break;
                            }
                        }
                    }
                    EditorEvent::BufferClose { handle } => {
                        self.buffers
                            .remove_from_editor_event_handler(handle, &mut self.word_database);
                        for client in clients.iter_mut() {
                            client.on_buffer_close(self, handle);
                        }
                        self.buffer_views.remove_buffer_views(handle);
                    }
                    EditorEvent::FixCursors { handle, cursors } => {
                        let mut view_cursors =
                            self.buffer_views.get_mut(handle).cursors.mut_guard();
                        view_cursors.clear();
                        for &cursor in cursors.as_cursors(&self.events) {
                            view_cursors.add(cursor);
                        }
                    }
                    EditorEvent::BufferViewLostFocus { handle } => {
                        let buffer_view = self.buffer_views.get(handle);
                        let buffer_handle = buffer_view.buffer_handle;
                        let buffer = self.buffers.get(buffer_handle);
                        let should_close = buffer.capabilities.auto_close && !buffer.needs_save();
                        let any_view = !clients
                            .iter()
                            .filter_map(Client::buffer_view_handle)
                            .map(|h| self.buffer_views.get(h))
                            .any(|v| v.buffer_handle == buffer_handle);

                        if should_close && !any_view {
                            self.buffers.defer_remove(buffer_handle, &mut self.events);
                        }
                    }
                }
            }
        }
    }
}

