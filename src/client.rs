use argh::FromArgValue;

use crate::{
    buffer_view::BufferViewHandle,
    connection::ConnectionWithClientHandle,
    editor::Editor,
    editor_event::EditorEvent,
    navigation_history::NavigationHistory,
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
    ui::UiKind,
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TargetClient {
    Local,
    Remote(ConnectionWithClientHandle),
}

impl TargetClient {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => TargetClient::Local,
            _ => TargetClient::Remote(ConnectionWithClientHandle::from_index(index + 1)),
        }
    }

    pub fn into_index(self) -> usize {
        match self {
            TargetClient::Local => 0,
            TargetClient::Remote(handle) => handle.into_index() + 1,
        }
    }
}

impl Default for TargetClient {
    fn default() -> Self {
        Self::Local
    }
}

impl<'de> Serialize<'de> for TargetClient {
    fn serialize<S>(&self, serializer: &mut S)
    where
        S: Serializer,
    {
        match self {
            Self::Local => 0u32.serialize(serializer),
            Self::Remote(handle) => {
                let index = handle.into_index() as u32 + 1;
                index.serialize(serializer);
            }
        }
    }

    fn deserialize<D>(deserializer: &mut D) -> Result<Self, DeserializeError>
    where
        D: Deserializer<'de>,
    {
        let index = u32::deserialize(deserializer)?;
        match index {
            0 => Ok(Self::Local),
            _ => Ok(Self::Remote(ConnectionWithClientHandle::from_index(
                index as usize - 1,
            ))),
        }
    }
}

impl FromArgValue for TargetClient {
    fn from_arg_value(value: &str) -> Result<Self, String> {
        let index = value.parse::<usize>().map_err(|e| e.to_string())?;

        match index {
            0 => Ok(Self::Local),
            _ => Ok(Self::Remote(ConnectionWithClientHandle::from_index(index))),
        }
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
    pub ui: &'a mut UiKind,
    pub target: TargetClient,
    pub client: &'a mut Client,
    pub buffer: &'a mut Vec<u8>,
}

#[derive(Default)]
struct ClientData {
    pub ui: UiKind,
    pub display_buffer: Vec<u8>,
}
impl ClientData {
    pub fn reset(&mut self) {
        match self.ui {
            UiKind::Tui {
                ref mut status_bar_buf,
            } => status_bar_buf.clear(),
            _ => self.ui = UiKind::default(),
        }
        self.display_buffer.clear();
    }
}

#[derive(Default)]
pub struct ClientManager {
    focused_target: TargetClient,
    pub client_map: ClientTargetMap, // TODO: expose through ClientCollection

    local: Client,
    remotes: Vec<Option<Client>>,
    local_data: ClientData,
    remote_data: Vec<ClientData>,
}

impl ClientManager {
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

    pub fn on_client_joined(&mut self, client_handle: ConnectionWithClientHandle) {
        let index = client_handle.into_index();
        let min_len = index + 1;
        if min_len > self.remotes.len() {
            self.remotes.resize_with(min_len, || None);
        }
        self.remotes[index] = Some(Client::default());
        if min_len > self.remote_data.len() {
            self.remote_data.resize_with(min_len, || Default::default());
        }

        self.client_map.on_client_joined(client_handle);
    }

    pub fn on_client_left(&mut self, client_handle: ConnectionWithClientHandle) {
        let index = client_handle.into_index();
        self.remotes[index] = None;
        self.remote_data[index].reset();

        self.client_map.on_client_left(client_handle);
        if self.focused_target == TargetClient::Remote(client_handle) {
            self.focused_target = TargetClient::Local;
        }
    }

    pub fn get(&self, target: TargetClient) -> Option<&Client> {
        match target {
            TargetClient::Local => Some(&self.local),
            TargetClient::Remote(handle) => self.remotes[handle.into_index()].as_ref(),
        }
    }

    pub fn get_mut(&mut self, target: TargetClient) -> Option<&mut Client> {
        match target {
            TargetClient::Local => Some(&mut self.local),
            TargetClient::Remote(handle) => self.remotes[handle.into_index()].as_mut(),
        }
    }

    pub fn get_client_ref(&mut self, target: TargetClient) -> Option<ClientRef> {
        match target {
            TargetClient::Local => Some(ClientRef {
                ui: &mut self.local_data.ui,
                target,
                client: &mut self.local,
                buffer: &mut self.local_data.display_buffer,
            }),
            TargetClient::Remote(handle) => {
                let index = handle.into_index();
                match self.remotes[index] {
                    Some(ref mut c) => {
                        let data = &mut self.remote_data[index];
                        Some(ClientRef {
                            ui: &mut data.ui,
                            target,
                            client: c,
                            buffer: &mut data.display_buffer,
                        })
                    }
                    None => None,
                }
            }
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Client> {
        let remotes = self.remotes.iter_mut().flatten();
        std::iter::once(&mut self.local).chain(remotes)
    }

    pub fn client_refs<'a>(&'a mut self) -> impl Iterator<Item = ClientRef<'a>> {
        let remotes = self
            .remotes
            .iter_mut()
            .enumerate()
            .zip(self.remote_data.iter_mut())
            .flat_map(|((i, c), d)| {
                c.as_mut().map(move |c| ClientRef {
                    ui: &mut d.ui,
                    target: TargetClient::Remote(ConnectionWithClientHandle::from_index(i)),
                    client: c,
                    buffer: &mut d.display_buffer,
                })
            });

        std::iter::once(ClientRef {
            ui: &mut self.local_data.ui,
            target: TargetClient::Local,
            client: &mut self.local,
            buffer: &mut self.local_data.display_buffer,
        })
        .chain(remotes)
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
            TargetClient::Local => self.local_target = to,
            TargetClient::Remote(handle) => self.remote_targets[handle.into_index()] = to,
        }
    }

    pub fn get(&self, target: TargetClient) -> TargetClient {
        match target {
            TargetClient::Local => self.local_target.unwrap_or(target),
            TargetClient::Remote(handle) => {
                self.remote_targets[handle.into_index()].unwrap_or(target)
            }
        }
    }
}
