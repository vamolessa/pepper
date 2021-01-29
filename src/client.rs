use argh::FromArgValue;

use crate::{
    buffer_view::BufferViewHandle,
    editor::Editor,
    editor_event::EditorEvent,
    navigation_history::NavigationHistory,
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
};

// TODO: remove this and keep only `client_index`
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct TargetClient(pub usize);

impl TargetClient {
    // TODO: remove this
    pub fn local() -> Self {
        Self(0)
    }

    pub fn into_index(self) -> usize {
        self.0
    }

    pub fn from_index(index: usize) -> TargetClient {
        TargetClient(index)
    }
}

impl<'de> Serialize<'de> for TargetClient {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        let index = self.0 as u32;
        index.serialize(serializer);
    }

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
    where
        D: Deserializer<'de>,
    {
        let index = u32::deserialize(deserializer)?;
        Ok(Self(index as _))
    }
}

impl FromArgValue for TargetClient {
    fn from_arg_value(value: &str) -> Result<Self, String> {
        let index = value.parse::<usize>().map_err(|e| e.to_string())?;
        Ok(Self(index))
    }
}

#[derive(Default)]
pub struct Client {
    pub viewport_size: (u16, u16),
    pub scroll: usize,
    pub height: u16,
    pub navigation_history: NavigationHistory,

    current_buffer_view_handle: Option<BufferViewHandle>,
    previous_buffer_view_handle: Option<BufferViewHandle>,
}

impl Client {
    pub fn buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.current_buffer_view_handle
    }

    pub fn previous_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.previous_buffer_view_handle
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

pub struct ClientRef<'a> {
    pub target: TargetClient,
    pub client: &'a mut Client,
    pub display_buffer: &'a mut Vec<u8>,
    pub status_bar_buffer: &'a mut String,
}

#[derive(Default)]
struct ClientData {
    pub display_buffer: Vec<u8>,
    pub status_bar_buffer: String,
}
impl ClientData {
    pub fn reset(&mut self) {
        self.display_buffer.clear();
        self.status_bar_buffer.clear();
    }
}

pub struct ClientManager {
    focused_target: TargetClient,    // TODO: make it Option<TargetClient>
    pub client_map: ClientTargetMap, // TODO: expose through ClientCollection
    clients: Vec<Option<Client>>,
    data: Vec<ClientData>,
}

impl ClientManager {
    pub fn new() -> Self {
        Self {
            focused_target: TargetClient::local(),
            client_map: ClientTargetMap::default(),
            clients: Vec::new(),
            data: Vec::new(),
        }
    }

    pub fn focused_target(&self) -> TargetClient {
        self.focused_target
    }

    // TODO: maybe change it to handle it from client_events
    pub fn focus_client(&mut self, target: TargetClient) -> bool {
        let changed = target != self.focused_target;
        self.focused_target = target;
        changed
    }

    // TODO: maybe move it to Editor
    pub fn set_buffer_view_handle(
        &mut self,
        editor: &mut Editor,
        target: TargetClient,
        handle: Option<BufferViewHandle>,
    ) {
        if let Some(client) = self.get_mut(target) {
            if client.current_buffer_view_handle != handle {
                client.previous_buffer_view_handle = client.current_buffer_view_handle;
                client.current_buffer_view_handle = handle;
            }

            if let Some(handle) = handle
                .and_then(|h| editor.buffer_views.get(h))
                .map(|v| v.buffer_handle)
            {
                editor.events.enqueue(EditorEvent::BufferOpen { handle });
            }
        }
    }

    pub fn on_client_joined(&mut self, index: usize) {
        let min_len = index + 1;
        if min_len > self.clients.len() {
            self.clients.resize_with(min_len, || None);
        }
        self.clients[index] = Some(Client::default());
        if min_len > self.data.len() {
            self.data.resize_with(min_len, || Default::default());
        }

        self.client_map.on_client_joined(index);
    }

    pub fn on_client_left(&mut self, index: usize) {
        self.clients[index] = None;
        self.data[index].reset();

        self.client_map.on_client_left(index);
        if self.focused_target == TargetClient(index) {
            self.focused_target = TargetClient::local();
        }
    }

    pub fn get(&self, target: TargetClient) -> Option<&Client> {
        self.clients[target.0].as_ref()
    }

    pub fn get_mut(&mut self, target: TargetClient) -> Option<&mut Client> {
        self.clients[target.0].as_mut()
    }

    pub fn get_client_ref(&mut self, target: TargetClient) -> Option<ClientRef> {
        let index = target.0;
        match self.clients[index] {
            Some(ref mut c) => {
                let data = &mut self.data[index];
                Some(ClientRef {
                    target,
                    client: c,
                    display_buffer: &mut data.display_buffer,
                    status_bar_buffer: &mut data.status_bar_buffer,
                })
            }
            None => None,
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Client> {
        self.clients.iter_mut().flatten()
    }

    pub fn client_refs<'a>(&'a mut self) -> impl Iterator<Item = ClientRef<'a>> {
        self.clients
            .iter_mut()
            .enumerate()
            .zip(self.data.iter_mut())
            .flat_map(|((i, c), d)| {
                c.as_mut().map(move |c| ClientRef {
                    target: TargetClient(i),
                    client: c,
                    display_buffer: &mut d.display_buffer,
                    status_bar_buffer: &mut d.status_bar_buffer,
                })
            })
    }
}

#[derive(Default)]
pub struct ClientTargetMap {
    targets: Vec<Option<TargetClient>>,
}

impl ClientTargetMap {
    pub fn on_client_joined(&mut self, index: usize) {
        let min_len = index + 1;
        if min_len > self.targets.len() {
            self.targets.resize_with(min_len, || None);
        }
    }

    pub fn on_client_left(&mut self, index: usize) {
        self.targets[index] = None;
        for target in &mut self.targets {
            if *target == Some(TargetClient(index)) {
                *target = None;
            }
        }
    }

    pub fn map(&mut self, from: TargetClient, to: TargetClient) {
        self.targets[from.0] = if to.0 < self.targets.len() {
            Some(to)
        } else {
            None
        };
    }

    pub fn get(&self, target: TargetClient) -> TargetClient {
        self.targets[target.0].unwrap_or(target)
    }
}
