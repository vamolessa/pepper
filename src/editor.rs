use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    command::CommandCollection,
    config::Config,
    connection::ConnectionWithClientHandle,
    connection::TargetClient,
    editor_operation::{EditorOperation, EditorOperationSerializer, StatusMessageKind},
    event::Key,
    keymap::{KeyMapCollection, MatchResult},
    mode::{Mode, ModeContext, ModeOperation},
};

pub enum EditorLoop {
    Quit,
    Continue,
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

pub struct Editor {
    mode: Mode,
    pub keymaps: KeyMapCollection,
    buffered_keys: Vec<Key>,
    input: String,
    pub commands: CommandCollection,

    pub buffers: BufferCollection,
    pub buffer_views: BufferViewCollection,
    local_client_current_buffer_view_handle: Option<BufferViewHandle>,
    remote_client_current_buffer_view_handles: Vec<Option<BufferViewHandle>>,

    focused_client: TargetClient,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            mode: Mode::default(),
            keymaps: KeyMapCollection::default(),
            buffered_keys: Vec::new(),
            input: String::new(),
            commands: CommandCollection::default(),

            buffers: Default::default(),
            buffer_views: BufferViewCollection::default(),
            local_client_current_buffer_view_handle: None,
            remote_client_current_buffer_view_handles: Vec::new(),

            focused_client: TargetClient::Local,
        }
    }

    pub fn on_client_joined(
        &mut self,
        client_handle: ConnectionWithClientHandle,
        config: &Config,
        operations: &mut EditorOperationSerializer,
    ) {
        operations.on_client_joined(client_handle);
        let target_client = TargetClient::Remote(client_handle);

        let buffer_view_handle = match self.focused_client {
            TargetClient::All => None,
            TargetClient::Local => self.local_client_current_buffer_view_handle.as_ref(),
            TargetClient::Remote(handle) => {
                self.remote_client_current_buffer_view_handles[handle.into_index()].as_ref()
            }
        };
        if let Some(buffer) = buffer_view_handle
            .map(|h| self.buffer_views.get(h).buffer_handle)
            .map(|h| self.buffers.get(h))
            .flatten()
        {
            operations.serialize_buffer(target_client, &buffer.content);
            operations.serialize(target_client, &EditorOperation::Path(buffer.path.as_ref()));
        }

        operations.serialize(self.focused_client, &EditorOperation::Focused(false));
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
                .as_ref()
                .map(|h| {
                    self.buffer_views
                        .get(h)
                        .clone_with_target_client(target_client)
                }),
            TargetClient::Remote(handle) => self.remote_client_current_buffer_view_handles
                [handle.into_index()]
            .as_ref()
            .map(|h| {
                self.buffer_views
                    .get(h)
                    .clone_with_target_client(target_client)
            }),
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

        self.focused_client = target_client;
        self.buffered_keys.clear();
    }

    pub fn on_client_left(
        &mut self,
        client_handle: ConnectionWithClientHandle,
        operations: &mut EditorOperationSerializer,
    ) {
        operations.on_client_left(client_handle);
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

    pub fn on_key(
        &mut self,
        config: &Config,
        key: Key,
        target_client: TargetClient,
        operations: &mut EditorOperationSerializer,
    ) -> EditorLoop {
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

            let current_buffer_view_handle = match target_client {
                TargetClient::All => unreachable!(),
                TargetClient::Local => &mut self.local_client_current_buffer_view_handle,
                TargetClient::Remote(handle) => {
                    &mut self.remote_client_current_buffer_view_handles[handle.into_index()]
                }
            };

            let mut mode_context = ModeContext {
                target_client,
                operations,

                config,
                keymaps: &mut self.keymaps,
                commands: &mut self.commands,
                buffers: &mut self.buffers,
                buffer_views: &mut self.buffer_views,
                current_buffer_view_handle,
                input: &mut self.input,
            };

            match self.mode.on_event(&mut mode_context, &mut keys) {
                ModeOperation::Pending => return EditorLoop::Continue,
                ModeOperation::None => (),
                ModeOperation::Quit => {
                    self.buffered_keys.clear();
                    return EditorLoop::Quit;
                }
                ModeOperation::EnterMode(next_mode) => {
                    self.mode = next_mode.clone();
                    self.mode.on_enter(&mut mode_context);
                    operations.serialize(TargetClient::All, &EditorOperation::Mode(next_mode));
                }
                ModeOperation::Error(error) => {
                    self.mode = Mode::default();
                    self.mode.on_enter(&mut mode_context);
                    operations
                        .serialize(TargetClient::All, &EditorOperation::Mode(Mode::default()));
                    operations.serialize(
                        self.focused_client,
                        &EditorOperation::StatusMessage(StatusMessageKind::Error, &error[..]),
                    );

                    break EditorLoop::Continue;
                }
            }
        };

        self.buffered_keys.clear();
        result
    }
}
