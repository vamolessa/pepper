use std::{
    error::Error,
    fmt,
    fs::File,
    path::{Path, PathBuf},
};

use crate::platform::Key;

use crate::{
    buffer::BufferCollection,
    buffer_view::BufferViewCollection,
    client::{ClientManager, TargetClient},
    client_event::{parse_all_keys, ClientEvent},
    command::{CommandManager, CommandOperation},
    config::Config,
    editor_event::{EditorEvent, EditorEventQueue},
    keymap::{KeyMapCollection, MatchResult},
    lsp::{LspClientCollection, LspClientContext, LspClientHandle, LspServerEvent},
    mode::{Mode, ModeKind, ModeOperation},
    picker::Picker,
    register::{RegisterCollection, RegisterKey, KEY_QUEUE_REGISTER},
    syntax::HighlightResult,
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
                // TODO: implement clipboard
                let mut text = String::new();
                //if platform.read_from_clipboard(&mut text) {
                self.input.push_str(&text);
                //}
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
pub enum StatusMessageKind {
    Info,
    Error,
}

pub struct StatusBar {
    kind: StatusMessageKind,
    message: String,
}
impl StatusBar {
    pub fn new() -> Self {
        Self {
            kind: StatusMessageKind::Info,
            message: String::new(),
        }
    }

    pub fn message(&self) -> (StatusMessageKind, &str) {
        (self.kind, &self.message)
    }

    pub fn clear(&mut self) {
        self.message.clear();
    }

    // TODO: replace with 'write'
    pub fn write_str(&mut self, kind: StatusMessageKind, message: &str) {
        self.kind = kind;
        self.message.clear();
        self.message.push_str(message);
    }

    // TODO: replace with 'write'
    pub fn write_fmt(&mut self, kind: StatusMessageKind, args: fmt::Arguments) {
        self.kind = kind;
        self.message.clear();
        let _ = fmt::write(&mut self.message, args);
    }

    pub fn write(&mut self, kind: StatusMessageKind) -> StatusBarWrite {
        self.kind = kind;
        self.message.clear();
        StatusBarWrite(self)
    }

    // TODO: replace with 'write'
    pub fn write_error(&mut self, error: &dyn Error) {
        use fmt::Write;

        self.kind = StatusMessageKind::Error;
        self.message.clear();
        let _ = write!(&mut self.message, "{}", error);
        let mut error = error.source();
        while let Some(e) = error {
            self.message.push('\n');
            let _ = write!(&mut self.message, "{}", e);
            error = e.source();
        }
    }
}
pub struct StatusBarWrite<'a>(&'a mut StatusBar);
impl<'a> StatusBarWrite<'a> {
    pub fn str(&mut self, message: &str) {
        self.0.message.push_str(message);
    }

    pub fn fmt(&mut self, args: fmt::Arguments) {
        let _ = fmt::write(&mut self.0.message, args);
    }
}

pub struct Editor {
    pub current_directory: PathBuf,
    pub config: Config,
    pub mode: Mode,

    pub buffers: BufferCollection,
    pub buffer_views: BufferViewCollection,
    pub word_database: WordDatabase,

    pub buffered_keys: BufferedKeys,
    pub recording_macro: Option<RegisterKey>,
    pub registers: RegisterCollection,
    pub read_line: ReadLine,
    pub picker: Picker,

    pub status_bar: StatusBar,

    pub commands: CommandManager,
    pub lsp: LspClientCollection,
    pub events: EditorEventQueue,

    keymaps: KeyMapCollection,
}
impl Editor {
    pub fn new(current_directory: PathBuf) -> Self {
        Self {
            current_directory,
            config: Config::default(),
            mode: Mode::default(),

            buffers: Default::default(),
            buffer_views: BufferViewCollection::default(),
            word_database: WordDatabase::new(),

            buffered_keys: BufferedKeys::default(),
            recording_macro: None,
            registers: RegisterCollection::default(),
            read_line: ReadLine::default(),
            picker: Picker::default(),

            status_bar: StatusBar::new(),

            commands: CommandManager::new(),
            lsp: LspClientCollection::new(),
            events: EditorEventQueue::default(),

            keymaps: KeyMapCollection::default(),
        }
    }

    pub fn load_config(&mut self, clients: &mut ClientManager, path: &str) -> bool {
        use std::io::Read;

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                self.status_bar
                    .write(StatusMessageKind::Error)
                    .fmt(format_args!("could not open config file '{}'", path));
                return false;
            }
        };

        let mut text = String::new();
        if let Err(_) = file.read_to_string(&mut text) {
            self.status_bar
                .write(StatusMessageKind::Error)
                .fmt(format_args!("could not read config file '{}'", path));
            return false;
        }

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            match CommandManager::eval(self, clients, None, line) {
                Some(CommandOperation::Quit) | Some(CommandOperation::QuitAll) => return true,
                Some(CommandOperation::Error) | None => (),
            }
        }

        true
    }

    pub fn on_pre_render(&mut self, clients: &mut ClientManager) -> bool {
        let picker_height = self.picker.update_scroll_and_unfiltered_entries(
            self.config.values.picker_max_height.get() as _,
            &EmptyWordCollection,
            self.read_line.input(),
        );

        let mut needs_redraw = false;

        let focused_target = clients.focused_target();
        for c in clients.client_refs() {
            let target = c.target;
            let client = c.client;
            let picker_height = if focused_target == target {
                picker_height as _
            } else {
                0
            };

            let buffer_views = &self.buffer_views;
            let buffers = &mut self.buffers;
            if let Some(buffer) = client
                .buffer_view_handle()
                .and_then(|h| buffer_views.get(h))
                .map(|v| v.buffer_handle)
                .and_then(|h| buffers.get_mut(h))
            {
                if let HighlightResult::Pending = buffer.update_highlighting(&self.config.syntaxes)
                {
                    needs_redraw = true;
                }
            }

            client.update_view(self, picker_height);
        }

        needs_redraw
    }

    pub fn on_client_joined(&mut self, clients: &mut ClientManager, index: usize) {
        clients.on_client_joined(index);

        let target = TargetClient(index);
        let buffer_view_handle = clients
            .get(clients.focused_target())
            .and_then(|c| c.buffer_view_handle())
            .and_then(|h| self.buffer_views.get(h))
            .map(|v| v.clone_with_target_client(target))
            .map(|b| self.buffer_views.add(b));

        clients.set_buffer_view_handle(self, target, buffer_view_handle);
    }

    pub fn on_client_left(&mut self, clients: &mut ClientManager, index: usize) {
        clients.on_client_left(index);
    }

    pub fn on_event(
        &mut self,
        clients: &mut ClientManager,
        target: TargetClient,
        event: ClientEvent,
    ) -> EditorLoop {
        let result = match event {
            ClientEvent::AsFocusedClient => {
                clients.client_map.map(target, clients.focused_target());
                EditorLoop::Continue
            }
            ClientEvent::AsClient(as_target) => {
                clients.client_map.map(target, as_target);
                EditorLoop::Continue
            }
            ClientEvent::OpenBuffer(mut path) => {
                let target = clients.client_map.get(target);

                let mut line_index = None;
                if let Some(separator_index) = path.rfind(':') {
                    if let Ok(n) = path[(separator_index + 1)..].parse() {
                        let n: usize = n;
                        line_index = Some(n.saturating_sub(1));
                        path = &path[..separator_index];
                    }
                }

                match self.buffer_views.buffer_view_handle_from_path(
                    target,
                    &mut self.buffers,
                    &mut self.word_database,
                    &self.current_directory,
                    Path::new(path),
                    line_index,
                    &mut self.events,
                ) {
                    Ok(handle) => clients.set_buffer_view_handle(self, target, Some(handle)),
                    Err(error) => self.status_bar.write_error(&error),
                }

                self.trigger_event_handlers(clients, target);
                EditorLoop::Continue
            }
            ClientEvent::Key(key) => {
                self.status_bar.clear();

                let target = clients.client_map.get(target);
                if clients.focus_client(target) {
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

                        match Mode::on_client_keys(self, clients, target, &mut keys) {
                            None => (),
                            Some(ModeOperation::Pending) => {
                                return EditorLoop::Continue;
                            }
                            Some(ModeOperation::Quit) => {
                                Mode::change_to(self, clients, target, ModeKind::default());
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
                                self.registers
                                    .append_fmt(register_key, format_args!("{}", key));
                            }
                        }
                    }

                    match self.recording_macro {
                        Some(KEY_QUEUE_REGISTER) => {
                            self.buffered_keys.0.clear();
                        }
                        _ => {
                            self.parse_and_set_keys_from_register(KEY_QUEUE_REGISTER);
                            self.registers.set(KEY_QUEUE_REGISTER, "");
                        }
                    }
                    if self.buffered_keys.0.is_empty() {
                        break;
                    }
                }

                self.buffered_keys.0.clear();
                self.trigger_event_handlers(clients, target);
                EditorLoop::Continue
            }
            ClientEvent::Resize(width, height) => {
                let target = clients.client_map.get(target);
                if let Some(client) = clients.get_mut(target) {
                    client.viewport_size = (width, height);
                }
                EditorLoop::Continue
            }
        };

        result
    }

    pub fn on_idle(&mut self, clients: &mut ClientManager) {
        self.events.enqueue(EditorEvent::Idle);
        self.trigger_event_handlers(clients, TargetClient::local());
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
                    self.status_bar.write_fmt(
                        StatusMessageKind::Error,
                        format_args!("error parsing keys '{}'\n{}", keys, &error),
                    );
                    self.buffered_keys.0.clear();
                    return;
                }
            }
        }
    }

    fn trigger_event_handlers(&mut self, clients: &mut ClientManager, target: TargetClient) {
        self.events.flip();
        if let None = self.events.iter().next() {
            return;
        }

        Mode::on_editor_events(self, clients, target);

        // TODO: transformar em função static e só passar o editor
        /*
        if let Err(error) = lsp.on_editor_events(&mut lsp_ctx) {
            lsp_ctx.status_bar.write_error(&error);
        }
        */

        self.handle_editor_events(clients);
    }

    fn handle_editor_events(&mut self, clients: &mut ClientManager) {
        for event in self.events.iter() {
            match event {
                EditorEvent::BufferLoad { handle } => {
                    if let Some(buffer) = self.buffers.get_mut(*handle) {
                        buffer.refresh_syntax(&self.config.syntaxes);
                    }
                }
                EditorEvent::BufferSave { handle, new_path } => {
                    if *new_path {
                        if let Some(buffer) = self.buffers.get_mut(*handle) {
                            buffer.refresh_syntax(&self.config.syntaxes);
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

    pub fn on_lsp_event(&mut self, client_handle: LspClientHandle, event: LspServerEvent) {
        let mut ctx = LspClientContext {
            current_directory: &self.current_directory,
            config: &mut self.config,

            buffers: &mut self.buffers,
            buffer_views: &mut self.buffer_views,
            word_database: &mut self.word_database,

            status_bar: &mut self.status_bar,
            events: &mut self.events,
        };

        if let Err(error) = self.lsp.on_server_event(&mut ctx, client_handle, event) {
            self.status_bar.write_error(&error);
        }
    }
}
