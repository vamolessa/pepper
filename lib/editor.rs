use std::{fmt, fs::File, path::PathBuf};

use crate::{
    buffer::BufferCollection,
    buffer_view::BufferViewCollection,
    client::{ClientHandle, ClientManager},
    command::{CommandIter, CommandManager, CommandOperation},
    config::Config,
    editor_utils::{MessageKind, ReadLine, StatusBar, StringPool},
    events::{ClientEvent, EditorEvent, EditorEventQueue, KeyParser},
    keymap::{KeyMapCollection, MatchResult},
    lsp,
    mode::{Mode, ModeContext, ModeKind, ModeOperation},
    picker::Picker,
    platform::{Key, Platform},
    register::{RegisterCollection, RegisterKey, KEY_QUEUE_REGISTER},
    syntax::{HighlightResult, SyntaxCollection},
    theme::Theme,
    word_database::{WordDatabase, WordIndicesIter},
};

#[derive(Clone, Copy)]
pub enum EditorControlFlow {
    Quit,
    QuitAll,
    Continue,
}
impl EditorControlFlow {
    pub fn is_quit(self) -> bool {
        matches!(self, EditorControlFlow::Quit | EditorControlFlow::QuitAll)
    }
}

#[derive(Default)]
pub struct BufferedKeys(Vec<Key>);
impl BufferedKeys {
    pub fn as_slice(&self) -> &[Key] {
        &self.0
    }
}

pub struct KeysIterator {
    index: usize,
}
impl KeysIterator {
    fn new() -> Self {
        Self { index: 0 }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn next(&mut self, keys: &BufferedKeys) -> Key {
        if self.index < keys.0.len() {
            let next = keys.0[self.index];
            self.index += 1;
            next
        } else {
            Key::None
        }
    }

    pub fn put_back(&mut self) {
        self.index = self.index.saturating_sub(1);
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

    pub commands: CommandManager,
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

            buffers: Default::default(),
            buffer_views: BufferViewCollection::default(),
            word_database: WordDatabase::new(),

            buffered_keys: BufferedKeys::default(),
            recording_macro: None,
            registers: RegisterCollection::default(),
            read_line: ReadLine::default(),
            picker: Picker::default(),
            string_pool: StringPool::default(),

            status_bar: StatusBar::new(),

            commands: CommandManager::new(),
            lsp: lsp::ClientManager::new(),
            events: EditorEventQueue::default(),
        }
    }

    pub fn load_config(
        &mut self,
        platform: &mut Platform,
        clients: &mut ClientManager,
        path: &str,
    ) -> Option<CommandOperation> {
        use std::io::Read;

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                self.status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("could not open config file '{}'", path));
                return None;
            }
        };

        let mut source = String::new();
        match file.read_to_string(&mut source) {
            Ok(_) => CommandManager::eval_body_and_print(self, platform, clients, None, &source),
            Err(_) => {
                self.status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("could not read config file '{}'", path));
                None
            }
        }
    }

    pub fn on_pre_render(&mut self, clients: &mut ClientManager) -> bool {
        let picker_height = self.picker.update_scroll_and_unfiltered_entries(
            self.config.picker_max_height.get() as _,
            WordIndicesIter::empty(),
            self.read_line.input(),
        );

        let mut needs_redraw = false;
        let focused_handle = clients.focused_handle();

        for c in clients.iter_mut() {
            let picker_height = if focused_handle == Some(c.handle()) {
                picker_height as _
            } else {
                0
            };

            let buffer_views = &self.buffer_views;
            let buffers = &mut self.buffers;
            if let Some(buffer) = c
                .buffer_view_handle()
                .and_then(|h| buffer_views.get(h))
                .map(|v| v.buffer_handle)
                .and_then(|h| buffers.get_mut(h))
            {
                if let HighlightResult::Pending = buffer.update_highlighting(&self.syntaxes) {
                    needs_redraw = true;
                }
            }

            c.update_view(self, picker_height);
        }

        needs_redraw
    }

    pub fn on_client_joined(&mut self, clients: &mut ClientManager, handle: ClientHandle) {
        clients.on_client_joined(handle);

        let buffer_view_handle = clients
            .focused_handle()
            .and_then(|h| clients.get(h))
            .and_then(|c| c.buffer_view_handle())
            .and_then(|h| self.buffer_views.get(h))
            .map(|v| v.clone_with_client_handle(handle))
            .map(|b| self.buffer_views.add(b));

        if let Some(client) = clients.get_mut(handle) {
            client.set_buffer_view_handle(buffer_view_handle);
        }
    }

    pub fn on_client_left(&mut self, clients: &mut ClientManager, handle: ClientHandle) {
        clients.on_client_left(handle);
    }

    pub fn on_client_event(
        &mut self,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        platform: &mut Platform,
        event: ClientEvent,
    ) -> EditorControlFlow {
        match event {
            ClientEvent::Key(client_handle, key) => {
                self.status_bar.clear();

                if clients.focus_client(client_handle) {
                    self.recording_macro = None;
                    self.buffered_keys.0.clear();
                }

                self.buffered_keys.0.push(key);

                match self
                    .keymaps
                    .matches(self.mode.kind(), self.buffered_keys.as_slice())
                {
                    MatchResult::None => (),
                    MatchResult::Prefix => return EditorControlFlow::Continue,
                    MatchResult::ReplaceWith(replaced_keys) => {
                        self.buffered_keys.0.clear();
                        self.buffered_keys.0.extend_from_slice(replaced_keys);
                    }
                }

                'key_queue_loop: loop {
                    let mut keys = KeysIterator::new();
                    loop {
                        if keys.index == self.buffered_keys.0.len() {
                            break;
                        }
                        let keys_from_index = self.recording_macro.map(|_| keys.index);

                        let mut ctx = ModeContext {
                            editor: self,
                            platform,
                            clients,
                            client_handle,
                        };
                        match Mode::on_client_keys(&mut ctx, &mut keys) {
                            None => (),
                            Some(ModeOperation::Pending) => {
                                return EditorControlFlow::Continue;
                            }
                            Some(ModeOperation::Quit) => {
                                Mode::change_to(&mut ctx, ModeKind::default());
                                self.buffered_keys.0.clear();
                                return EditorControlFlow::Quit;
                            }
                            Some(ModeOperation::QuitAll) => {
                                self.buffered_keys.0.clear();
                                return EditorControlFlow::QuitAll;
                            }
                            Some(ModeOperation::ExecuteMacro(key)) => {
                                self.parse_and_set_keys_from_register(key);
                                continue 'key_queue_loop;
                            }
                        }

                        if let (Some(from_index), Some(register_key)) =
                            (keys_from_index, self.recording_macro.clone())
                        {
                            for key in &self.buffered_keys.0[from_index..keys.index] {
                                use fmt::Write;
                                let register = self.registers.get_mut(register_key);
                                let _ = write!(register, "{}", key);
                            }
                        }
                    }

                    match self.recording_macro {
                        Some(KEY_QUEUE_REGISTER) => {
                            self.buffered_keys.0.clear();
                        }
                        _ => {
                            self.parse_and_set_keys_from_register(KEY_QUEUE_REGISTER);
                            self.registers.get_mut(KEY_QUEUE_REGISTER).clear();
                        }
                    }
                    if self.buffered_keys.0.is_empty() {
                        break;
                    }
                }

                self.buffered_keys.0.clear();
                self.trigger_event_handlers(clients, platform);
                EditorControlFlow::Continue
            }
            ClientEvent::Resize(client_handle, width, height) => {
                if let Some(client) = clients.get_mut(client_handle) {
                    client.viewport_size = (width, height);
                }
                EditorControlFlow::Continue
            }
            ClientEvent::Command(client_handle, commands) => {
                match CommandManager::eval_body_and_print(
                    self,
                    platform,
                    clients,
                    Some(client_handle),
                    commands,
                ) {
                    None => EditorControlFlow::Continue,
                    Some(CommandOperation::Quit) => EditorControlFlow::Quit,
                    Some(CommandOperation::QuitAll) => EditorControlFlow::QuitAll,
                }
            }
        }
    }

    pub fn on_idle(&mut self, clients: &mut ClientManager, platform: &mut Platform) {
        self.events.enqueue(EditorEvent::Idle);
        self.trigger_event_handlers(clients, platform);
    }

    fn parse_and_set_keys_from_register(&mut self, register_key: RegisterKey) {
        self.buffered_keys.0.clear();

        let keys = self.registers.get(register_key);
        if keys.is_empty() {
            return;
        }

        for key in KeyParser::new(keys) {
            match key {
                Ok(key) => self.buffered_keys.0.push(key),
                Err(error) => {
                    self.status_bar
                        .write(MessageKind::Error)
                        .fmt(format_args!("error parsing keys '{}'\n{}", keys, &error));
                    self.buffered_keys.0.clear();
                    return;
                }
            }
        }
    }

    fn trigger_event_handlers(&mut self, clients: &mut ClientManager, platform: &mut Platform) {
        self.events.flip();
        if let None = self.events.iter().next() {
            return;
        }

        lsp::ClientManager::on_editor_events(self, platform);
        self.handle_editor_events(clients);
    }

    fn handle_editor_events(&mut self, clients: &mut ClientManager) {
        for event in self.events.iter() {
            match event {
                EditorEvent::BufferOpen { handle } => {
                    if let Some(buffer) = self.buffers.get_mut(*handle) {
                        buffer.refresh_syntax(&self.syntaxes);
                    }
                }
                EditorEvent::BufferSave { handle, new_path } => {
                    if *new_path {
                        if let Some(buffer) = self.buffers.get_mut(*handle) {
                            buffer.refresh_syntax(&self.syntaxes);
                        }
                    }
                }
                EditorEvent::BufferClose { handle } => {
                    self.buffers
                        .remove(*handle, clients, &mut self.word_database);
                }
                _ => (),
            }
        }
    }
}
