use std::{fmt, str::FromStr};

use crate::{
    buffer::{BufferProperties, BufferHandle, CharDisplayDistances},
    buffer_position::BufferPositionIndex,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    editor::Editor,
    editor_utils::ResidualStrBytes,
    events::{EditorEvent, EditorEventQueue},
    navigation_history::{NavigationHistory, NavigationMovement},
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
};

#[derive(Default, Clone, Copy, Eq, PartialEq)]
pub struct ClientHandle(u8);

impl ClientHandle {
    pub fn into_index(self) -> usize {
        self.0 as _
    }

    pub fn from_index(index: usize) -> Option<ClientHandle> {
        if index <= u8::MAX as usize {
            Some(ClientHandle(index as _))
        } else {
            None
        }
    }
}

impl<'de> Serialize<'de> for ClientHandle {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        self.0.serialize(serializer);
    }

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
    where
        D: Deserializer<'de>,
    {
        Ok(Self(u8::deserialize(deserializer)?))
    }
}

pub struct ClientHandleFromStrError;
impl fmt::Display for ClientHandleFromStrError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("could not parse client index")
    }
}

impl FromStr for ClientHandle {
    type Err = ClientHandleFromStrError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse() {
            Ok(i) => Ok(Self(i)),
            Err(_) => Err(ClientHandleFromStrError),
        }
    }
}

#[derive(Default)]
pub struct Client {
    active: bool,
    handle: ClientHandle,

    pub viewport_size: (u16, u16),
    pub scroll: (BufferPositionIndex, BufferPositionIndex),
    pub height: u16,
    pub(crate) navigation_history: NavigationHistory,

    buffer_view_handle: Option<BufferViewHandle>,
    stdin_buffer_handle: Option<BufferHandle>,
    stdin_residual_bytes: ResidualStrBytes,
}

impl Client {
    fn dispose(&mut self) {
        self.active = false;

        self.viewport_size = (0, 0);
        self.scroll = (0, 0);
        self.height = 0;
        self.navigation_history.clear();

        self.buffer_view_handle = None;
    }

    pub fn handle(&self) -> ClientHandle {
        self.handle
    }

    pub fn buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.buffer_view_handle
    }

    pub fn stdin_buffer_handle(&self) -> Option<BufferHandle> {
        self.stdin_buffer_handle
    }

    pub fn set_buffer_view_handle(
        &mut self,
        handle: Option<BufferViewHandle>,
        buffer_views: &BufferViewCollection,
        events: &mut EditorEventQueue,
    ) {
        NavigationHistory::save_snapshot(self, buffer_views);
        self.set_buffer_view_handle_no_history(handle, events);
    }

    pub fn has_ui(&self) -> bool {
        self.viewport_size.0 != 0 && self.viewport_size.1 != 0
    }

    pub(crate) fn set_buffer_view_handle_no_history(
        &mut self,
        handle: Option<BufferViewHandle>,
        events: &mut EditorEventQueue,
    ) {
        if self.buffer_view_handle != handle {
            if let Some(handle) = self.buffer_view_handle {
                events.enqueue(EditorEvent::BufferViewLostFocus { handle });
            }
            self.buffer_view_handle = handle;
        }
    }

    pub(crate) fn update_view(&mut self, editor: &Editor, picker_height: u16) {
        self.height = self.viewport_size.1.saturating_sub(1 + picker_height);

        let width = self.viewport_size.0 as BufferPositionIndex;
        if width == 0 {
            return;
        }
        let height = self.height as BufferPositionIndex;
        if height == 0 {
            return;
        }
        let buffer_view_handle = match self.buffer_view_handle() {
            Some(handle) => handle,
            None => return,
        };

        let buffer_view = editor.buffer_views.get(buffer_view_handle);
        let buffer = editor.buffers.get(buffer_view.buffer_handle).content();

        let position = buffer_view.cursors.main_cursor().position;

        let line_index = position.line_index;
        let line = buffer.line_at(line_index as _).as_str();
        let column_index = position.column_byte_index;

        let half_height = height / 2;
        let quarter_height = half_height / 2;

        let (mut scroll_x, mut scroll_y) = self.scroll;

        if column_index < scroll_x {
            scroll_x = column_index
        } else {
            let index = column_index as usize;
            let (width, text) = match line[index..].chars().next() {
                Some(c) => (width, &line[..index + c.len_utf8()]),
                None => (width - 1, line),
            };

            if let Some(d) = CharDisplayDistances::new(text, editor.config.tab_size)
                .rev()
                .take_while(|d| d.distance <= width as _)
                .last()
            {
                scroll_x = scroll_x.max(d.char_index as _);
            }
        }

        if line_index < scroll_y.saturating_sub(quarter_height) {
            scroll_y = line_index.saturating_sub(half_height);
        } else if line_index < scroll_y {
            scroll_y = line_index;
        } else if line_index >= scroll_y + height + quarter_height {
            scroll_y = line_index + 1 - half_height;
        } else if line_index >= scroll_y + height {
            scroll_y = line_index + 1 - height;
        }

        self.scroll = (scroll_x, scroll_y);
    }

    pub(crate) fn on_stdin_input(&mut self, editor: &mut Editor, bytes: &[u8]) {
        let mut buf = Default::default();
        let texts = self.stdin_residual_bytes.receive_bytes(&mut buf, bytes);

        let buffer_handle = match self.stdin_buffer_handle() {
            Some(handle) => handle,
            None => {
                use fmt::Write;

                let buffer = editor.buffers.add_new();

                let mut path = editor.string_pool.acquire_with("pipe.");
                let _ = write!(path, "{}", self.handle().into_index());
                buffer.path.clear();
                buffer.path.push(&path);
                editor.string_pool.release(path);

                buffer.properties = BufferProperties::text();
                buffer.properties.is_file = false;

                let buffer_view_handle =
                    editor.buffer_views.add_new(self.handle(), buffer.handle());
                self.set_buffer_view_handle(
                    Some(buffer_view_handle),
                    &editor.buffer_views,
                    &mut editor.events,
                );

                self.stdin_buffer_handle = Some(buffer.handle());
                buffer.handle()
            }
        };

        let buffer = editor.buffers.get_mut(buffer_handle);
        for text in texts {
            let position = buffer.content().end();
            buffer.insert_text(
                &mut editor.word_database,
                position,
                text,
                &mut editor.events,
            );
        }
    }

    pub(crate) fn on_buffer_close(&mut self, editor: &mut Editor, buffer_handle: BufferHandle) {
        self.navigation_history
            .remove_snapshots_with_buffer_handle(buffer_handle);

        if let Some(handle) = self.buffer_view_handle {
            let buffer_view = editor.buffer_views.get(handle);
            if buffer_view.buffer_handle == buffer_handle {
                self.buffer_view_handle = None;
                NavigationHistory::move_in_history(self, editor, NavigationMovement::Backward);
                NavigationHistory::move_in_history(self, editor, NavigationMovement::Forward);
            }
        }

        if self.stdin_buffer_handle == Some(buffer_handle) {
            self.stdin_buffer_handle = None;
        }
    }
}

#[derive(Default)]
pub struct ClientManager {
    focused_client: Option<ClientHandle>,
    previous_focused_client: Option<ClientHandle>,
    clients: Vec<Client>,
}

impl ClientManager {
    pub fn focused_client(&self) -> Option<ClientHandle> {
        self.focused_client
    }

    pub fn previous_focused_client(&self) -> Option<ClientHandle> {
        self.previous_focused_client
    }

    pub fn focus_client(&mut self, handle: ClientHandle) -> bool {
        let client = self.get(handle);
        if !client.has_ui() {
            return false;
        }

        let changed = Some(handle) != self.focused_client;
        if changed {
            self.previous_focused_client = self.focused_client;
        }
        self.focused_client = Some(handle);
        changed
    }

    pub fn get(&self, handle: ClientHandle) -> &Client {
        &self.clients[handle.into_index()]
    }

    pub fn get_mut(&mut self, handle: ClientHandle) -> &mut Client {
        &mut self.clients[handle.into_index()]
    }

    pub fn iter(&self) -> impl Clone + Iterator<Item = &Client> {
        self.clients.iter().filter(|c| c.active)
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Client> {
        self.clients.iter_mut().filter(|c| c.active)
    }

    pub(crate) fn on_client_joined(&mut self, handle: ClientHandle) {
        let min_len = handle.into_index() + 1;
        if min_len > self.clients.len() {
            self.clients.resize_with(min_len, Client::default);
        }

        let client = &mut self.clients[handle.into_index()];
        client.active = true;
        client.handle = handle;
    }

    pub(crate) fn on_client_left(&mut self, handle: ClientHandle) {
        self.clients[handle.into_index()].dispose();
        if self.focused_client == Some(handle) {
            self.focused_client = None;
        }
    }
}

