use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    buffer::BufferCollection,
    buffer_view::BufferViewCollection,
    client::{ClientCollection, ClientTargetMap, TargetClient},
    client_event::{ClientEvent, Key},
    config::Config,
    connection::ConnectionWithClientHandle,
    editor_event::{EditorEvent, EditorEventDoubleQueue, EditorEventsIter},
    keymap::{KeyMapCollection, MatchResult},
    lsp::{LspClientCollection, LspClientContext, LspClientHandle, LspServerEvent},
    mode::{Mode, ModeContext, ModeOperation},
    picker::Picker,
    register::{RegisterCollection, RegisterKey, KEY_QUEUE_REGISTER},
    script::ScriptEngine,
    task::{TaskHandle, TaskManager, TaskResult},
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

pub struct KeysIterator<'a> {
    keys: &'a [Key],
    index: usize,
}
impl<'a> KeysIterator<'a> {
    fn new(keys: &'a [Key]) -> Self {
        Self { keys, index: 0 }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn keys(&self) -> &'a [Key] {
        self.keys
    }

    pub fn next(&mut self) -> Key {
        if self.index < self.keys.len() {
            let next = self.keys[self.index];
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

    pub fn reset(&mut self, prompt: &str) {
        self.prompt.clear();
        self.prompt.push_str(prompt);
        self.input.clear();
    }

    pub fn set_input(&mut self, input: &str) {
        self.input.clear();
        self.input.push_str(input);
    }

    pub fn poll(&mut self, keys: &mut KeysIterator) -> ReadLinePoll {
        match keys.next() {
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
                use copypasta::{ClipboardContext, ClipboardProvider};
                if let Ok(text) = ClipboardContext::new().and_then(|mut c| c.get_contents()) {
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
pub enum StatusMessageKind {
    Info,
    Error,
}

pub struct StatusMessage {
    kind: StatusMessageKind,
    message: String,
}
impl StatusMessage {
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

    pub fn write_str(&mut self, kind: StatusMessageKind, message: &str) {
        self.kind = kind;
        self.message.clear();
        self.message.push_str(message);
    }

    pub fn write_fmt(&mut self, kind: StatusMessageKind, args: fmt::Arguments) {
        self.kind = kind;
        self.message.clear();
        let _ = fmt::write(&mut self.message, args);
    }

    pub fn write_error(&mut self, error: &dyn Error) {
        use std::fmt::Write;

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

pub struct Editor {
    pub current_directory: PathBuf,
    pub config: Config,
    pub mode: Mode,

    pub buffers: BufferCollection,
    pub buffer_views: BufferViewCollection,
    pub word_database: WordDatabase,

    pub buffered_keys: Vec<Key>,
    pub recording_macro: Option<RegisterKey>,
    pub registers: RegisterCollection,
    pub read_line: ReadLine,
    pub picker: Picker,

    pub focused_client: TargetClient,
    pub status_message: StatusMessage,
    pub tasks: TaskManager,
    pub lsp: LspClientCollection,

    events: EditorEventDoubleQueue,
    keymaps: KeyMapCollection,
    scripts: ScriptEngine,
    client_target_map: ClientTargetMap,
}
impl Editor {
    pub fn new(current_directory: PathBuf, tasks: TaskManager, lsp: LspClientCollection) -> Self {
        Self {
            current_directory,
            config: Config::default(),
            mode: Mode::default(),

            buffers: Default::default(),
            buffer_views: BufferViewCollection::default(),
            word_database: WordDatabase::new(),

            buffered_keys: Vec::new(),
            recording_macro: None,
            registers: RegisterCollection::default(),
            read_line: ReadLine::default(),
            picker: Picker::default(),

            focused_client: TargetClient::Local,
            status_message: StatusMessage::new(),
            tasks,
            lsp,

            events: EditorEventDoubleQueue::default(),
            keymaps: KeyMapCollection::default(),
            scripts: ScriptEngine::new(),
            client_target_map: ClientTargetMap::default(),
        }
    }

    pub fn load_config(&mut self, clients: &mut ClientCollection, path: &Path) {
        let (mode, _, _, mut mode_ctx) = self.mode_context(clients, TargetClient::Local);
        let (scripts, _, mut script_ctx) = mode_ctx.script_context();

        if let Err(e) = scripts.eval_entry_file(&mut script_ctx, path) {
            script_ctx.status_message.write_error(&e);
        }

        let next_mode = script_ctx.next_mode;
        mode.change_to(&mut mode_ctx, next_mode);
    }

    pub fn on_pre_render(&mut self, clients: &mut ClientCollection) {
        let picker_height = self.picker.update_scroll_and_unfiltered_entries(
            self.config.values.picker_max_height.get() as _,
            &EmptyWordCollection,
            self.read_line.input(),
        );

        for c in clients.client_refs() {
            let target = c.target;
            let client = c.client;
            let picker_height = if self.focused_client == target {
                picker_height as _
            } else {
                0
            };
            client.update_view(self, picker_height);
        }
    }

    pub fn on_client_joined(
        &mut self,
        clients: &mut ClientCollection,
        client_handle: ConnectionWithClientHandle,
    ) {
        clients.on_client_joined(client_handle);
        self.client_target_map.on_client_joined(client_handle);

        let target_client = TargetClient::Remote(client_handle);
        let buffer_view_handle = clients
            .get(self.focused_client)
            .and_then(|c| c.current_buffer_view_handle())
            .and_then(|h| self.buffer_views.get(h))
            .map(|v| v.clone_with_target_client(target_client))
            .map(|b| self.buffer_views.add(b));

        if let Some(client) = clients.get_mut(target_client) {
            client.set_current_buffer_view_handle(buffer_view_handle);
        }
    }

    pub fn on_client_left(
        &mut self,
        clients: &mut ClientCollection,
        client_handle: ConnectionWithClientHandle,
    ) {
        clients.on_client_left(client_handle);
        self.client_target_map.on_client_left(client_handle);

        if self.focused_client == TargetClient::Remote(client_handle) {
            self.focused_client = TargetClient::Local;
        }
    }

    pub fn on_event(
        &mut self,
        clients: &mut ClientCollection,
        target_client: TargetClient,
        event: ClientEvent,
    ) -> EditorLoop {
        let result = match event {
            ClientEvent::Ui(ui) => {
                let target_client = self.client_target_map.get(target_client);
                if let Some(client) = clients.get_client_ref(target_client) {
                    *client.ui = ui;
                }
                EditorLoop::Continue
            }
            ClientEvent::AsFocusedClient => {
                self.client_target_map
                    .map(target_client, self.focused_client);
                EditorLoop::Continue
            }
            ClientEvent::AsClient(target) => {
                self.client_target_map.map(target_client, target);
                EditorLoop::Continue
            }
            ClientEvent::OpenBuffer(mut path) => {
                let target_client = self.client_target_map.get(target_client);

                let mut line_index = None;
                if let Some(separator_index) = path.rfind(':') {
                    if let Ok(n) = path[(separator_index + 1)..].parse() {
                        let n: usize = n;
                        line_index = Some(n.saturating_sub(1));
                        path = &path[..separator_index];
                    }
                }

                let write_events = self.events.get_stream_and_sink().1;

                match self.buffer_views.buffer_view_handle_from_path(
                    &mut self.buffers,
                    &mut self.word_database,
                    &self.config.syntaxes,
                    target_client,
                    &self.current_directory,
                    Path::new(path),
                    line_index,
                    write_events,
                ) {
                    Ok(handle) => {
                        if let Some(client) = clients.get_mut(target_client) {
                            client.set_current_buffer_view_handle(Some(handle));
                        }
                    }
                    Err(error) => self.status_message.write_error(&error),
                }

                self.trigger_event_handlers(clients, target_client);
                EditorLoop::Continue
            }
            ClientEvent::Key(key) => {
                let target_client = self.client_target_map.get(target_client);

                if target_client != self.focused_client {
                    self.focused_client = target_client;
                    self.recording_macro = None;
                    self.buffered_keys.clear();
                }

                self.buffered_keys.push(key);

                match self
                    .keymaps
                    .matches(self.mode.discriminant(), &self.buffered_keys)
                {
                    MatchResult::None => (),
                    MatchResult::Prefix => return EditorLoop::Continue,
                    MatchResult::ReplaceWith(replaced_keys) => {
                        self.buffered_keys.clear();
                        self.buffered_keys.extend_from_slice(replaced_keys);
                    }
                }

                'key_queue_loop: loop {
                    let (mode, buffered_keys, _, mut mode_ctx) =
                        self.mode_context(clients, target_client);
                    let mut keys = KeysIterator::new(&buffered_keys);
                    loop {
                        if keys.index == buffered_keys.len() {
                            break;
                        }
                        let keys_from_index = mode_ctx.recording_macro.map(|_| keys.index);

                        match mode.on_client_keys(&mut mode_ctx, &mut keys) {
                            ModeOperation::Pending => {
                                return EditorLoop::Continue;
                            }
                            ModeOperation::None => (),
                            ModeOperation::Quit => {
                                mode.change_to(&mut mode_ctx, Mode::default());
                                self.buffered_keys.clear();
                                return EditorLoop::Quit;
                            }
                            ModeOperation::QuitAll => {
                                self.buffered_keys.clear();
                                return EditorLoop::QuitAll;
                            }
                            ModeOperation::EnterMode(next_mode) => {
                                mode.change_to(&mut mode_ctx, next_mode);
                            }
                            ModeOperation::ExecuteMacro(key) => {
                                self.parse_and_set_keys_in_register(key);
                                continue 'key_queue_loop;
                            }
                        }

                        if let Some((from_index, register_key)) =
                            keys_from_index.zip(mode_ctx.recording_macro.clone())
                        {
                            for key in &buffered_keys[from_index..keys.index] {
                                mode_ctx
                                    .registers
                                    .append_fmt(register_key, format_args!("{}", key));
                            }
                        }
                    }

                    match self.recording_macro {
                        Some(KEY_QUEUE_REGISTER) => {
                            self.buffered_keys.clear();
                        }
                        _ => {
                            self.parse_and_set_keys_in_register(KEY_QUEUE_REGISTER);
                            self.registers.set(KEY_QUEUE_REGISTER, "");
                        }
                    }
                    if self.buffered_keys.is_empty() {
                        break;
                    }
                }

                self.buffered_keys.clear();
                self.trigger_event_handlers(clients, target_client);
                EditorLoop::Continue
            }
            ClientEvent::Resize(width, height) => {
                let target_client = self.client_target_map.get(target_client);
                if let Some(client) = clients.get_mut(target_client) {
                    client.viewport_size = (width, height);
                }
                EditorLoop::Continue
            }
        };

        result
    }

    fn mode_context<'a>(
        &'a mut self,
        clients: &'a mut ClientCollection,
        target_client: TargetClient,
    ) -> (
        &'a mut Mode,
        &'a [Key],
        EditorEventsIter<'a>,
        ModeContext<'a>,
    ) {
        let (read_events, write_events) = self.events.get_stream_and_sink();
        let mode_context = ModeContext {
            target_client,
            clients,

            current_directory: &self.current_directory,
            config: &mut self.config,

            buffers: &mut self.buffers,
            buffer_views: &mut self.buffer_views,
            word_database: &mut self.word_database,

            recording_macro: &mut self.recording_macro,
            registers: &mut self.registers,
            read_line: &mut self.read_line,
            picker: &mut self.picker,

            status_message: &mut self.status_message,

            events: write_events,
            keymaps: &mut self.keymaps,
            scripts: &mut self.scripts,
            tasks: &mut self.tasks,
            lsp: &mut self.lsp,
        };
        (
            &mut self.mode,
            &self.buffered_keys,
            read_events,
            mode_context,
        )
    }

    fn parse_and_set_keys_in_register(&mut self, register_key: RegisterKey) {
        self.buffered_keys.clear();

        let keys = self.registers.get(register_key);
        if keys.is_empty() {
            return;
        }

        for key in Key::parse_all(keys) {
            match key {
                Ok(key) => self.buffered_keys.push(key),
                Err(error) => {
                    self.status_message.write_fmt(
                        StatusMessageKind::Error,
                        format_args!("error parsing keys '{}'\n{}", keys, &error),
                    );
                    self.buffered_keys.clear();
                    return;
                }
            }
        }
    }

    fn trigger_event_handlers(
        &mut self,
        clients: &mut ClientCollection,
        target_client: TargetClient,
    ) {
        self.events.flip();
        let (mode, _, events, mut mode_ctx) = self.mode_context(clients, target_client);

        if let None = events.into_iter().next() {
            return;
        }

        mode.on_editor_events(&mut mode_ctx, events);

        let (scripts, _, mut script_ctx) = mode_ctx.script_context();
        if let Err(error) = scripts.on_editor_event(&mut script_ctx, events) {
            script_ctx.status_message.write_error(&error);
        }

        let (lsp, mut lsp_ctx) = script_ctx.lsp_context();
        if let Err(error) = lsp.on_editor_events(&mut lsp_ctx, events) {
            lsp_ctx.status_message.write_error(&error);
        }

        Self::handle_editor_events(&mut mode_ctx, events);
    }

    fn handle_editor_events(ctx: &mut ModeContext, events: EditorEventsIter) {
        for event in events {
            match event {
                EditorEvent::BufferSave { handle, new_path } => {
                    if *new_path {
                        if let Some(buffer) = ctx.buffers.get_mut(*handle) {
                            buffer.refresh_syntax(&ctx.config.syntaxes);
                        }
                    }
                }
                EditorEvent::BufferClose { handle } => {
                    ctx.buffers.remove(*handle, ctx.clients, ctx.word_database);
                }
                EditorEvent::BufferInsertText { handle, range, .. } => {
                    if let Some(buffer) = ctx.buffers.get_mut(*handle) {
                        buffer.highlight_insert(&ctx.config.syntaxes, *range);
                    }
                }
                EditorEvent::BufferDeleteText { handle, range } => {
                    if let Some(buffer) = ctx.buffers.get_mut(*handle) {
                        buffer.highlight_delete(&ctx.config.syntaxes, *range);
                    }
                }
                _ => (),
            }
        }
    }

    pub fn on_task_event(
        &mut self,
        clients: &mut ClientCollection,
        target_client: TargetClient,
        handle: TaskHandle,
        result: TaskResult,
    ) {
        let (_, _, _, mut mode_ctx) = self.mode_context(clients, target_client);
        let (scripts, _, mut script_ctx) = mode_ctx.script_context();
        if let Err(error) = scripts.on_task_event(&mut script_ctx, handle, &result) {
            script_ctx.status_message.write_error(&error);
        }
    }

    pub fn on_lsp_event(&mut self, client_handle: LspClientHandle, event: LspServerEvent) {
        let (_, write_events) = self.events.get_stream_and_sink();
        let mut ctx = LspClientContext {
            current_directory: &self.current_directory,
            config: &mut self.config,

            buffers: &mut self.buffers,
            buffer_views: &mut self.buffer_views,
            word_database: &mut self.word_database,

            status_message: &mut self.status_message,
            events: write_events,
        };

        if let Err(error) = self.lsp.on_server_event(&mut ctx, client_handle, event) {
            self.status_message.write_error(&error);
        }
    }
}
