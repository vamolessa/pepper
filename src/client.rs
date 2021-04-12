use std::{fmt, str::FromStr};

use crate::{
    buffer_view::BufferViewHandle,
    editor::Editor,
    events::{EditorEvent, EditorEventQueue},
    navigation_history::NavigationHistory,
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
    pub scroll: usize,
    pub height: u16,
    pub navigation_history: NavigationHistory,

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

        self.current_buffer_view_handle = None;
        self.previous_buffer_view_handle = None;
    }

    pub fn handle(&self) -> ClientHandle {
        self.handle
    }

    pub fn buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.current_buffer_view_handle
    }

    pub fn previous_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.previous_buffer_view_handle
    }

    pub fn set_buffer_view_handle(
        &mut self,
        handle: Option<BufferViewHandle>,
        events: &mut EditorEventQueue,
    ) {
        if self.current_buffer_view_handle != handle {
            self.previous_buffer_view_handle = self.current_buffer_view_handle;
            self.current_buffer_view_handle = handle;

            events.enqueue(EditorEvent::ClientChangeBufferView {
                handle: self.handle,
            })
        }
    }

    pub fn has_ui(&self) -> bool {
        self.viewport_size.0 != 0 && self.viewport_size.0 != 0
    }

    pub fn update_view(&mut self, editor: &Editor, picker_height: u16) {
        fn calculate_scroll(this: &Client, editor: &Editor) -> Option<usize> {
            if this.viewport_size.0 == 0 {
                return None;
            }

            let buffer_view = editor.buffer_views.get(this.current_buffer_view_handle?)?;
            let buffer = editor.buffers.get(buffer_view.buffer_handle)?;
            let focused_line_index = buffer_view.cursors.main_cursor().position.line_index;

            let height = this.height as usize;
            let half_height = height / 2;

            let mut scroll = this.scroll;

            if focused_line_index < this.scroll.saturating_sub(half_height) {
                scroll = focused_line_index.saturating_sub(half_height);
            } else if focused_line_index < this.scroll {
                scroll = focused_line_index;
            } else if focused_line_index >= this.scroll + height + half_height {
                scroll = focused_line_index + 1 - half_height;
            } else if focused_line_index >= this.scroll + height {
                scroll = focused_line_index + 1 - height;
            }

            let mut extra_line_count = 0;
            for line in buffer
                .content()
                .lines()
                .skip(scroll)
                .take(focused_line_index - scroll)
            {
                extra_line_count += line.char_count() / this.viewport_size.0 as usize;
            }

            let focused_line_index = focused_line_index + extra_line_count;
            if focused_line_index >= scroll + height + half_height {
                scroll = focused_line_index + 1 - half_height;
            } else if focused_line_index >= scroll + height {
                scroll = focused_line_index + 1 - height;
            }

            Some(scroll)
        }

        self.height = self.viewport_size.1.saturating_sub(1 + picker_height);
        if let Some(scroll) = calculate_scroll(self, editor) {
            self.scroll = scroll;
        }
    }
}

pub struct ClientManager {
    focused_client: Option<ClientHandle>,
    previous_focused_client: Option<ClientHandle>,
    clients: Vec<Client>,
}

impl ClientManager {
    pub fn new() -> Self {
        Self {
            focused_client: None,
            previous_focused_client: None,
            clients: Vec::new(),
        }
    }

    pub fn focused_client(&self) -> Option<ClientHandle> {
        self.focused_client
    }

    pub fn previous_focused_client(&self) -> Option<ClientHandle> {
        self.previous_focused_client
    }

    pub fn focus_client(&mut self, handle: ClientHandle) -> bool {
        let changed = Some(handle) != self.focused_client;
        if changed {
            self.previous_focused_client = self.focused_client;
        }
        self.focused_client = Some(handle);
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
        if self.focused_client == Some(handle) {
            self.focused_client = None;
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

    pub fn iter(&self) -> impl Iterator<Item = &Client> {
        self.clients.iter().filter(|c| c.active)
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Client> {
        self.clients.iter_mut().filter(|c| c.active)
    }
}
