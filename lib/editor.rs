use std::{fmt, fs::File, path::PathBuf};

use crate::{
    buffer::BufferCollection,
    buffer_view::BufferViewCollection,
    client::{ClientHandle, ClientManager},
    client_event::{parse_all_keys, ClientEvent, ClientEventSource},
    command::{CommandIter, CommandManager, CommandOperation},
    config::Config,
    editor_event::{EditorEvent, EditorEventQueue},
    keymap::{KeyMapCollection, MatchResult},
    lsp::{LspClientCollection, LspClientHandle, LspServerEvent},
    mode::{Mode, ModeContext, ModeKind, ModeOperation},
    picker::Picker,
    platform::{Key, Platform},
    register::{RegisterCollection, RegisterKey, KEY_QUEUE_REGISTER},
    syntax::{HighlightResult, SyntaxCollection},
    theme::Theme,
    word_database::{EmptyWordCollection, WordDatabase},
};

#[derive(Clone, Copy)]
pub enum EditorLoop {
    Quit,
    QuitAll,
    Continue,
}
impl EditorLoop {
    pub fn is_quit(self) -> bool {
        matches!(self, EditorLoop::Quit | EditorLoop::QuitAll)
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

#[derive(Clone, Copy)]
pub enum ReadLinePoll {
    Pending,
    Submitted,
    Canceled,
}

#[derive(Default)]
pub struct ReadLine {
    prompt: String,
    input: String,
}
impl ReadLine {
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn set_prompt(&mut self, prompt: &str) {
        self.prompt.clear();
        self.prompt.push_str(prompt);
    }

    pub fn set_input(&mut self, input: &str) {
        self.input.clear();
        self.input.push_str(input);
    }

    pub fn poll(
        &mut self,
        platform: &mut dyn Platform,
        buffered_keys: &BufferedKeys,
        keys_iter: &mut KeysIterator,
    ) -> ReadLinePoll {
        match keys_iter.next(buffered_keys) {
            Key::Esc => ReadLinePoll::Canceled,
            Key::Enter => ReadLinePoll::Submitted,
            Key::Home | Key::Ctrl('u') => {
                self.input.clear();
                ReadLinePoll::Pending
            }
            Key::Ctrl('w') => {
                let mut found_space = false;
                let mut end_index = 0;
                for (i, c) in self.input.char_indices().rev() {
                    if found_space {
                        if c != ' ' {
                            break;
                        }
                    } else if c == ' ' {
                        found_space = true;
                    }
                    end_index = i;
                }

                self.input.truncate(end_index);
                ReadLinePoll::Pending
            }
            Key::Backspace | Key::Ctrl('h') => {
                if let Some((last_char_index, _)) = self.input.char_indices().rev().next() {
                    self.input.truncate(last_char_index);
                }
                ReadLinePoll::Pending
            }
            Key::Ctrl('y') => {
                let mut text = String::new();
                if platform.read_from_clipboard(&mut text) {
                    self.input.push_str(&text);
                }
                ReadLinePoll::Pending
            }
            Key::Char(c) => {
                self.input.push(c);
                ReadLinePoll::Pending
            }
            _ => ReadLinePoll::Pending,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EditorOutputKind {
    Info,
    Error,
}

pub struct EditorOutput {
    kind: EditorOutputKind,
    message: String,
}
impl EditorOutput {
    pub fn new() -> Self {
        Self {
            kind: EditorOutputKind::Info,
            message: String::new(),
        }
    }

    pub fn message(&self) -> (EditorOutputKind, &str) {
        (self.kind, &self.message)
    }

    pub fn clear(&mut self) {
        self.message.clear();
    }

    pub fn write(&mut self, kind: EditorOutputKind) -> EditorOutputWrite {
        self.kind = kind;
        self.message.clear();
        EditorOutputWrite(&mut self.message)
    }
}
pub struct EditorOutputWrite<'a>(&'a mut String);
impl<'a> EditorOutputWrite<'a> {
    pub fn str(&mut self, message: &str) {
        self.0.push_str(message);
    }

    pub fn fmt(&mut self, args: fmt::Arguments) {
        let _ = fmt::write(&mut self.0, args);
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

    pub output: EditorOutput,

    pub commands: CommandManager,
    pub lsp: LspClientCollection,
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

            output: EditorOutput::new(),

            commands: CommandManager::new(),
            lsp: LspClientCollection::new(),
            events: EditorEventQueue::default(),
        }
    }

    pub fn load_config(
        &mut self,
        platform: &mut dyn Platform,
        clients: &mut ClientManager,
        path: &str,
    ) -> Option<CommandOperation> {
        use std::io::Read;

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                self.output
                    .write(EditorOutputKind::Error)
                    .fmt(format_args!("could not open config file '{}'", path));
                return None;
            }
        };

        let mut text = String::new();
        if let Err(_) = file.read_to_string(&mut text) {
            self.output
                .write(EditorOutputKind::Error)
                .fmt(format_args!("could not read config file '{}'", path));
            return None;
        }

        for command in CommandIter::new(&text) {
            match CommandManager::eval(self, platform, clients, None, command) {
                Some(CommandOperation::Quit) | Some(CommandOperation::QuitAll) => break,
                None => (),
            }
        }

        None
    }

    pub fn on_pre_render(&mut self, clients: &mut ClientManager) -> bool {
        let picker_height = self.picker.update_scroll_and_unfiltered_entries(
            self.config.picker_max_height.get() as _,
            &EmptyWordCollection,
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
        platform: &mut dyn Platform,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        event: ClientEvent,
    ) -> EditorLoop {
        let result = match event {
            ClientEvent::Key(source, key) => {
                self.output.clear();

                let client_handle = match source {
                    ClientEventSource::ConnectionClient => client_handle,
                    ClientEventSource::FocusedClient => match clients.focused_handle() {
                        Some(handle) => handle,
                        None => return EditorLoop::Continue,
                    },
                    ClientEventSource::ClientHandle(handle) => handle,
                };

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
                    MatchResult::Prefix => return EditorLoop::Continue,
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
                                return EditorLoop::Continue;
                            }
                            Some(ModeOperation::Quit) => {
                                Mode::change_to(&mut ctx, ModeKind::default());
                                self.buffered_keys.0.clear();
                                return EditorLoop::Quit;
                            }
                            Some(ModeOperation::QuitAll) => {
                                self.buffered_keys.0.clear();
                                return EditorLoop::QuitAll;
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
                self.trigger_event_handlers(clients);
                EditorLoop::Continue
            }
            ClientEvent::Resize(source, width, height) => {
                let client_handle = match source {
                    ClientEventSource::ConnectionClient => client_handle,
                    ClientEventSource::FocusedClient => match clients.focused_handle() {
                        Some(handle) => handle,
                        None => return EditorLoop::Continue,
                    },
                    ClientEventSource::ClientHandle(handle) => handle,
                };

                if let Some(client) = clients.get_mut(client_handle) {
                    client.viewport_size = (width, height);
                }
                EditorLoop::Continue
            }
            ClientEvent::Command(source, command) => {
                let client_handle = match source {
                    ClientEventSource::ConnectionClient => client_handle,
                    ClientEventSource::FocusedClient => match clients.focused_handle() {
                        Some(handle) => handle,
                        None => return EditorLoop::Continue,
                    },
                    ClientEventSource::ClientHandle(handle) => handle,
                };

                match CommandManager::eval(self, platform, clients, Some(client_handle), command) {
                    Some(CommandOperation::Quit) => EditorLoop::Quit,
                    Some(CommandOperation::QuitAll) => EditorLoop::QuitAll,
                    None => EditorLoop::Continue,
                }
            }
        };

        result
    }

    pub fn on_idle(&mut self, clients: &mut ClientManager) {
        self.events.enqueue(EditorEvent::Idle);
        self.trigger_event_handlers(clients);
    }

    fn parse_and_set_keys_from_register(&mut self, register_key: RegisterKey) {
        self.buffered_keys.0.clear();

        let keys = self.registers.get(register_key);
        if keys.is_empty() {
            return;
        }

        for key in parse_all_keys(keys) {
            match key {
                Ok(key) => self.buffered_keys.0.push(key),
                Err(error) => {
                    self.output
                        .write(EditorOutputKind::Error)
                        .fmt(format_args!("error parsing keys '{}'\n{}", keys, &error));
                    self.buffered_keys.0.clear();
                    return;
                }
            }
        }
    }

    fn trigger_event_handlers(&mut self, clients: &mut ClientManager) {
        self.events.flip();
        if let None = self.events.iter().next() {
            return;
        }

        if let Err(error) = LspClientCollection::on_editor_events(self) {
            self.output
                .write(EditorOutputKind::Error)
                .fmt(format_args!("{}", error));
        }
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

    pub fn on_process_stdout(&mut self, platform: &mut dyn Platform, process_index: usize, bytes: &[u8]) {
        //
    }

    pub fn on_process_stderr(&mut self, platform: &mut dyn Platform, process_index: usize, bytes: &[u8]) {
        //
    }

    pub fn on_process_exit(&mut self, platform: &mut dyn Platform, process_index: usize, success: bool) {
        //
    }

    pub fn on_lsp_event(&mut self, client_handle: LspClientHandle, event: LspServerEvent) {
        if let Err(error) = LspClientCollection::on_server_event(self, client_handle, event) {
            self.output
                .write(EditorOutputKind::Error)
                .fmt(format_args!("{}", error));
        }
    }
}
