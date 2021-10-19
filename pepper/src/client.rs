use std::fmt;

use crate::{
    buffer::{BufferHandle, BufferProperties, CharDisplayDistances},
    buffer_position::BufferPositionIndex,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    editor::Editor,
    editor_utils::ResidualStrBytes,
    navigation_history::{NavigationHistory, NavigationMovement},
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
};

#[derive(Clone, Copy, Eq, PartialEq)]
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

pub enum ViewAnchor {
    Top,
    Center,
    Bottom,
}

pub struct Client {
    active: bool,
    handle: ClientHandle,

    pub viewport_size: (u16, u16),
    pub(crate) scroll: BufferPositionIndex,

    pub(crate) navigation_history: NavigationHistory,

    buffer_view_handle: Option<BufferViewHandle>,
    stdin_buffer_handle: Option<BufferHandle>,
    stdin_residual_bytes: ResidualStrBytes,
}

impl Client {
    pub(crate) fn new() -> Self {
        Self {
            active: false,
            handle: ClientHandle(0),

            viewport_size: (0, 0),
            scroll: 0,

            navigation_history: NavigationHistory::default(),

            buffer_view_handle: None,
            stdin_buffer_handle: None,
            stdin_residual_bytes: ResidualStrBytes::default(),
        }
    }

    fn dispose(&mut self) {
        self.active = false;

        self.viewport_size = (0, 0);
        self.scroll = 0;

        self.navigation_history.clear();

        self.buffer_view_handle = None;
        self.stdin_buffer_handle = None;
        self.stdin_residual_bytes = ResidualStrBytes::default();
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
    ) {
        NavigationHistory::save_snapshot(self, buffer_views);
        self.set_buffer_view_handle_no_history(handle);
    }

    pub(crate) fn set_buffer_view_handle_no_history(&mut self, handle: Option<BufferViewHandle>) {
        self.buffer_view_handle = handle;
    }

    pub fn has_ui(&self) -> bool {
        self.viewport_size.0 != 0 && self.viewport_size.1 != 0
    }

    pub fn set_view_anchor(&mut self, editor: &Editor, anchor: ViewAnchor) {
        if !self.has_ui() {
            return;
        }

        let height = self.viewport_size.1.saturating_sub(1) as usize;
        let height_offset = match anchor {
            ViewAnchor::Top => 0,
            ViewAnchor::Center => height / 2,
            ViewAnchor::Bottom => height.saturating_sub(1),
        };

        let main_cursor_padding_top = self.find_main_cursor_padding_top(editor);
        self.scroll = main_cursor_padding_top.saturating_sub(height_offset) as _;
    }

    pub(crate) fn scroll_to_main_cursor(&mut self, editor: &Editor, margin_bottom: usize) {
        if !self.has_ui() {
            return;
        }

        let height = self.viewport_size.1.saturating_sub(1) as usize;
        let height = height.saturating_sub(margin_bottom);
        let half_height = height / 2;

        let main_cursor_padding_top = self.find_main_cursor_padding_top(editor);

        let scroll = self.scroll as usize;
        if main_cursor_padding_top < scroll.saturating_sub(half_height) {
            self.scroll = main_cursor_padding_top.saturating_sub(half_height) as _;
        } else if main_cursor_padding_top < scroll {
            self.scroll = main_cursor_padding_top as _;
        } else if main_cursor_padding_top >= scroll + height + half_height {
            self.scroll = (main_cursor_padding_top + 1 - half_height) as _;
        } else if main_cursor_padding_top >= scroll + height {
            self.scroll = (main_cursor_padding_top + 1 - height) as _;
        }
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
                self.set_buffer_view_handle(Some(buffer_view_handle), &editor.buffer_views);

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

    fn find_main_cursor_padding_top(&mut self, editor: &Editor) -> usize {
        let buffer_view_handle = match self.buffer_view_handle() {
            Some(handle) => handle,
            None => return 0,
        };

        let tab_size = editor.config.tab_size.get();
        let width = self.viewport_size.0 as usize;

        let buffer_view = editor.buffer_views.get(buffer_view_handle);
        let buffer = editor.buffers.get(buffer_view.buffer_handle).content();
        let position = buffer_view.cursors.main_cursor().position;

        let mut height = position.line_index as usize;
        for display_len in &buffer.line_display_lens()[..position.line_index as usize] {
            height += display_len.total_len(tab_size) / width;
        }

        let cursor_line = buffer.lines()[position.line_index as usize].as_str();
        let cursor_line = &cursor_line[..position.column_byte_index as usize];
        if let Some(d) = CharDisplayDistances::new(cursor_line, tab_size).last() {
            height += d.distance as usize / width;
        }

        height
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
            self.clients.resize_with(min_len, Client::new);
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

