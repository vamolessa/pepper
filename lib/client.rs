use argh::FromArgValue;

use crate::{
    buffer_view::BufferViewHandle,
    editor::Editor,
    navigation_history::NavigationHistory,
    platform::PlatformConnectionHandle,
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
};

#[derive(Default, Clone, Copy, Eq, PartialEq)]
pub struct ClientHandle(u16);

impl ClientHandle {
    pub fn into_index(self) -> usize {
        self.0 as _
    }

    pub fn from_index(index: usize) -> Option<ClientHandle> {
        if index <= u16::MAX as usize {
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
        Ok(Self(u16::deserialize(deserializer)?))
    }
}

impl FromArgValue for ClientHandle {
    fn from_arg_value(value: &str) -> Result<Self, String> {
        let index = value
            .parse::<u16>()
            .map_err(|e| format!("could not parse client index: {}", e))?;
        Ok(Self(index))
    }
}

#[derive(Default)]
pub struct Client {
    active: bool,
    handle: ClientHandle,

    pub viewport_size: (u16, u16),
    pub scroll: usize,
    pub height: u16,
    pub navigation_history: NavigationHistory,

    pub status_bar_buffer: String, // TODO: try to remove this

    current_buffer_view_handle: Option<BufferViewHandle>,
    previous_buffer_view_handle: Option<BufferViewHandle>,
}

impl Client {
    fn dispose(&mut self) {
        self.active = false;

        self.viewport_size = (0, 0);
        self.scroll = 0;
        self.height = 0;
        self.navigation_history.clear();

        self.status_bar_buffer.clear();

        self.current_buffer_view_handle = None;
        self.previous_buffer_view_handle = None;
    }

    pub fn handle(&self) -> ClientHandle {
        self.handle
    }

    pub fn connection_handle(&self) -> PlatformConnectionHandle {
        PlatformConnectionHandle(self.handle.into_index())
    }

    pub fn buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.current_buffer_view_handle
    }

    pub fn previous_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.previous_buffer_view_handle
    }

    pub fn set_buffer_view_handle(&mut self, handle: Option<BufferViewHandle>) {
        if self.current_buffer_view_handle != handle {
            self.previous_buffer_view_handle = self.current_buffer_view_handle;
            self.current_buffer_view_handle = handle;
        }
    }

    pub fn update_view(&mut self, editor: &Editor, picker_height: u16) {
        self.height = self.viewport_size.1.saturating_sub(1 + picker_height);
        if let Some(scroll) = self.calculate_scroll(editor) {
            self.scroll = scroll;
        }
    }

    fn calculate_scroll(&self, editor: &Editor) -> Option<usize> {
        if self.viewport_size.0 == 0 {
            return None;
        }

        let buffer_view = editor.buffer_views.get(self.current_buffer_view_handle?)?;
        let buffer = editor.buffers.get(buffer_view.buffer_handle)?;
        let focused_line_index = buffer_view.cursors.main_cursor().position.line_index;

        let height = self.height as usize;

        let mut scroll = self.scroll;

        if focused_line_index < self.scroll {
            scroll = focused_line_index;
        } else if focused_line_index >= self.scroll + height {
            scroll = focused_line_index + 1 - height;
        }

        let mut extra_line_count = 0;
        for line in buffer
            .content()
            .lines()
            .skip(scroll)
            .take(focused_line_index - scroll)
        {
            extra_line_count += line.char_count() / self.viewport_size.0 as usize;
        }

        if focused_line_index + extra_line_count >= scroll + height {
            scroll = focused_line_index + extra_line_count + 1 - height;
        }

        Some(scroll)
    }
}

pub struct ClientManager {
    focused_handle: Option<ClientHandle>,
    clients: Vec<Client>,
}

impl ClientManager {
    pub fn new() -> Self {
        Self {
            focused_handle: None,
            clients: Vec::new(),
        }
    }

    pub fn focused_handle(&self) -> Option<ClientHandle> {
        self.focused_handle
    }

    pub fn focus_client(&mut self, handle: ClientHandle) -> bool {
        let changed = Some(handle) != self.focused_handle;
        self.focused_handle = Some(handle);
        changed
    }

    pub fn on_client_joined(&mut self, handle: ClientHandle) {
        let min_len = handle.into_index() + 1;
        if min_len > self.clients.len() {
            self.clients.resize_with(min_len, Default::default);
        }

        let client = &mut self.clients[handle.into_index()];
        client.active = true;
        client.handle = handle;
    }

    pub fn on_client_left(&mut self, handle: ClientHandle) {
        self.clients[handle.into_index()].dispose();
        if self.focused_handle == Some(handle) {
            self.focused_handle = None;
        }
    }

    pub fn get(&self, handle: ClientHandle) -> Option<&Client> {
        let client = &self.clients[handle.into_index()];
        if client.active {
            Some(client)
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, handle: ClientHandle) -> Option<&mut Client> {
        let client = &mut self.clients[handle.into_index()];
        if client.active {
            Some(client)
        } else {
            None
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Client> {
        self.clients
            .iter_mut()
            .filter_map(|c| if c.active { Some(c) } else { None })
    }
}
