use std::path::Path;

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::Client,
    client_event::{ClientEvent, Key},
    config::Config,
    connection::{ConnectionWithClientHandle, TargetClient},
    editor_operation::{
        EditorOperation, EditorOperationDeserializeResult, EditorOperationDeserializer,
        EditorOperationSerializer,
    },
    keymap::{KeyMapCollection, MatchResult},
    mode::{Mode, ModeContext, ModeOperation},
    script::{ScriptContext, ScriptEngine},
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
        if self.index > 0 {
            self.index -= 1;
        }
    }
}

#[derive(Default)]
pub struct ClientTargetMap {
    local_target: Option<TargetClient>,
    remote_targets: Vec<Option<TargetClient>>,
}

impl ClientTargetMap {
    pub fn on_client_joined(&mut self, client_handle: ConnectionWithClientHandle) {
        let min_len = client_handle.into_index() + 1;
        if min_len > self.remote_targets.len() {
            self.remote_targets.resize_with(min_len, || None);
        }
    }

    pub fn on_client_left(&mut self, client_handle: ConnectionWithClientHandle) {
        if self.local_target == Some(TargetClient::Remote(client_handle)) {
            self.local_target = None;
        }

        self.remote_targets[client_handle.into_index()] = None;
        for target in &mut self.remote_targets {
            if *target == Some(TargetClient::Remote(client_handle)) {
                *target = None;
            }
        }
    }

    pub fn map(&mut self, from: TargetClient, to: TargetClient) {
        let to = match to {
            TargetClient::All => unreachable!(),
            TargetClient::Local => Some(to),
            TargetClient::Remote(handle) => {
                if handle.into_index() < self.remote_targets.len() {
                    Some(to)
                } else {
                    None
                }
            }
        };

        match from {
            TargetClient::All => unreachable!(),
            TargetClient::Local => self.local_target = to,
            TargetClient::Remote(handle) => self.remote_targets[handle.into_index()] = to,
        }
    }

    pub fn get(&self, target: TargetClient) -> TargetClient {
        match target {
            TargetClient::All => target,
            TargetClient::Local => self.local_target.unwrap_or(target),
            TargetClient::Remote(handle) => {
                self.remote_targets[handle.into_index()].unwrap_or(target)
            }
        }
    }
}

pub struct Editor {
    mode: Mode,
    keymaps: KeyMapCollection,
    buffered_keys: Vec<Key>,
    input: String,
    scripts: ScriptEngine,

    buffers: BufferCollection,
    buffer_views: BufferViewCollection,
    local_client_current_buffer_view_handle: Option<BufferViewHandle>,
    remote_client_current_buffer_view_handles: Vec<Option<BufferViewHandle>>,

    focused_client: TargetClient,
    client_target_map: ClientTargetMap,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            mode: Mode::default(),
            keymaps: KeyMapCollection::default(),
            buffered_keys: Vec::new(),
            input: String::new(),
            scripts: ScriptEngine::new(),

            buffers: Default::default(),
            buffer_views: BufferViewCollection::default(),
            local_client_current_buffer_view_handle: None,
            remote_client_current_buffer_view_handles: Vec::new(),

            focused_client: TargetClient::Local,
            client_target_map: ClientTargetMap::default(),
        }
    }

    pub fn load_config(
        &mut self,
        config: &mut Config,
        client: &mut Client,
        operations: &mut EditorOperationSerializer,
        path: &Path,
    ) {
        let mut editor_loop = EditorLoop::Continue;
        let ctx = ScriptContext {
            editor_loop: &mut editor_loop,
            target_client: TargetClient::Local,
            operations,

            config,
            keymaps: &mut self.keymaps,
            buffers: &mut self.buffers,
            buffer_views: &mut self.buffer_views,
            local_client_current_buffer_view_handle: &mut self
                .local_client_current_buffer_view_handle,
            remote_client_current_buffer_view_handles: &mut self
                .remote_client_current_buffer_view_handles,
        };

        if let Err(e) = self.scripts.eval_entry_file(ctx, path) {
            let message = e.to_string();
            operations.serialize_error(&message);
        }

        let mut deserializer = EditorOperationDeserializer::from_slice(operations.local_bytes());
        while let EditorOperationDeserializeResult::Some(op) = deserializer.deserialize_next() {
            let _ = client.on_editor_operation(config, &op);
        }
    }

    pub fn on_client_joined(
        &mut self,
        client_handle: ConnectionWithClientHandle,
        config: &Config,
        operations: &mut EditorOperationSerializer,
    ) {
        operations.on_client_joined(client_handle);
        self.client_target_map.on_client_joined(client_handle);

        let target_client = TargetClient::Remote(client_handle);

        let buffer_view_handle = match self.focused_client {
            TargetClient::All => unreachable!(),
            TargetClient::Local => self.local_client_current_buffer_view_handle,
            TargetClient::Remote(handle) => {
                self.remote_client_current_buffer_view_handles[handle.into_index()]
            }
        };
        if let Some(buffer) = buffer_view_handle
            .and_then(|h| self.buffer_views.get(h))
            .and_then(|v| self.buffers.get(v.buffer_handle))
        {
            operations.serialize_buffer(target_client, &buffer.content);
            operations.serialize(target_client, &EditorOperation::Path(buffer.path()));
        }

        operations.serialize(target_client, &EditorOperation::Mode(self.mode.clone()));
        for c in self.input.chars() {
            operations.serialize(target_client, &EditorOperation::InputAppend(c));
        }
        operations.serialize_config_values(target_client, &config.values);
        operations.serialize_theme(target_client, &config.theme);
        for syntax in config.syntaxes.iter() {
            operations.serialize_syntax(target_client, syntax);
        }

        let buffer_view = match self.focused_client {
            TargetClient::All => unreachable!(),
            TargetClient::Local => self
                .local_client_current_buffer_view_handle
                .and_then(|h| self.buffer_views.get(h))
                .map(|v| v.clone_with_target_client(target_client)),
            TargetClient::Remote(handle) => self.remote_client_current_buffer_view_handles
                [handle.into_index()]
            .and_then(|h| self.buffer_views.get(h))
            .map(|v| v.clone_with_target_client(target_client)),
        };

        if let Some(buffer_view) = &buffer_view {
            operations.serialize_cursors(target_client, &buffer_view.cursors);
        }

        match target_client {
            TargetClient::All => unreachable!(),
            TargetClient::Local => {
                self.local_client_current_buffer_view_handle =
                    buffer_view.map(|v| self.buffer_views.add(v));
            }
            TargetClient::Remote(handle) => {
                let min_len = handle.into_index() + 1;
                if min_len > self.remote_client_current_buffer_view_handles.len() {
                    self.remote_client_current_buffer_view_handles
                        .resize_with(min_len, || None);
                }

                self.remote_client_current_buffer_view_handles[handle.into_index()] =
                    buffer_view.map(|v| self.buffer_views.add(v));
            }
        }

        self.buffered_keys.clear();
    }

    pub fn on_client_left(
        &mut self,
        client_handle: ConnectionWithClientHandle,
        operations: &mut EditorOperationSerializer,
    ) {
        operations.on_client_left(client_handle);
        self.client_target_map.on_client_left(client_handle);

        if self.focused_client == TargetClient::Remote(client_handle) {
            self.focused_client = TargetClient::Local;
            operations.serialize(self.focused_client, &EditorOperation::Focused(true));
            operations.serialize(TargetClient::All, &EditorOperation::InputKeep(0));
            operations.serialize(TargetClient::All, &EditorOperation::Mode(Mode::default()));

            self.mode = Mode::default();
            self.buffered_keys.clear();
            self.input.clear();

            self.remote_client_current_buffer_view_handles[client_handle.into_index()] = None;
        }
    }

    pub fn on_event(
        &mut self,
        config: &Config,
        event: ClientEvent,
        target_client: TargetClient,
        operations: &mut EditorOperationSerializer,
    ) -> EditorLoop {
        match event {
            ClientEvent::AsFocusedClient => {
                self.client_target_map
                    .map(target_client, self.focused_client);
                EditorLoop::Continue
            }
            ClientEvent::AsClient(index) => {
                self.client_target_map
                    .map(target_client, TargetClient::from_index(index));
                EditorLoop::Continue
            }
            ClientEvent::OpenFile(path) => {
                let target_client = self.client_target_map.get(target_client);

                let path = Path::new(path);
                match self.buffer_views.new_buffer_from_file(
                    &mut self.buffers,
                    target_client,
                    operations,
                    path,
                ) {
                    Ok(buffer_view_handle) => match target_client {
                        TargetClient::All => unreachable!(),
                        TargetClient::Local => {
                            self.local_client_current_buffer_view_handle = Some(buffer_view_handle)
                        }
                        TargetClient::Remote(handle) => {
                            self.remote_client_current_buffer_view_handles[handle.into_index()] =
                                Some(buffer_view_handle)
                        }
                    },
                    Err(error) => operations.serialize_error(&error),
                }

                EditorLoop::Continue
            }
            ClientEvent::Key(key) => {
                let target_client = self.client_target_map.get(target_client);

                if target_client != self.focused_client {
                    operations.serialize(self.focused_client, &EditorOperation::Focused(false));
                    operations.serialize(target_client, &EditorOperation::Focused(true));

                    self.focused_client = target_client;
                    self.buffered_keys.clear();
                }

                self.buffered_keys.push(key);

                match self
                    .keymaps
                    .matches(self.mode.discriminant(), &self.buffered_keys[..])
                {
                    MatchResult::None => (),
                    MatchResult::Prefix => return EditorLoop::Continue,
                    MatchResult::ReplaceWith(replaced_keys) => {
                        self.buffered_keys.clear();
                        self.buffered_keys.extend_from_slice(replaced_keys);
                    }
                }

                let mut keys = KeysIterator::new(&self.buffered_keys);
                let result = loop {
                    if keys.index >= self.buffered_keys.len() {
                        break EditorLoop::Continue;
                    }

                    let mut mode_context = ModeContext {
                        target_client,
                        operations,

                        config,
                        keymaps: &mut self.keymaps,
                        scripts: &mut self.scripts,
                        buffers: &mut self.buffers,
                        buffer_views: &mut self.buffer_views,
                        local_client_current_buffer_view_handle: &mut self
                            .local_client_current_buffer_view_handle,
                        remote_client_current_buffer_view_handles: &mut self
                            .remote_client_current_buffer_view_handles,
                        input: &mut self.input,
                    };

                    match self.mode.on_event(&mut mode_context, &mut keys) {
                        ModeOperation::Pending => return EditorLoop::Continue,
                        ModeOperation::None => (),
                        ModeOperation::Quit => {
                            self.buffered_keys.clear();
                            return EditorLoop::Quit;
                        }
                        ModeOperation::QuitAll => {
                            self.buffered_keys.clear();
                            return EditorLoop::QuitAll;
                        }
                        ModeOperation::EnterMode(next_mode) => {
                            self.mode = next_mode.clone();
                            self.mode.on_enter(&mut mode_context);
                            operations
                                .serialize(TargetClient::All, &EditorOperation::Mode(next_mode));
                        }
                    }
                };

                self.buffered_keys.clear();
                result
            }
        }
    }
}
