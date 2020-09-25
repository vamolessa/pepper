use argh::FromArgValue;

use crate::{
    buffer_view::BufferViewHandle,
    connection::ConnectionWithClientHandle,
    editor::Editor,
    serialization::{DeserializeError, Deserializer, Serialize, Serializer},
    ui::UiKind,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TargetClient {
    Local,
    Remote(ConnectionWithClientHandle),
}

impl TargetClient {
    pub fn into_index(self) -> usize {
        match self {
            TargetClient::Local => 0,
            TargetClient::Remote(handle) => handle.into_index() + 1,
        }
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
    pub ui: UiKind,
    pub current_buffer_view_handle: Option<BufferViewHandle>,
    pub viewport_size: (u16, u16),
    pub scroll: usize,
    pub height: u16,
}

impl Client {
    pub fn update_view(&mut self, editor: &Editor, has_focus: bool) {
        let focused_line_index = self
            .current_buffer_view_handle
            .and_then(|h| editor.buffer_views.get(h))
            .map(|v| v.cursors.main_cursor().position.line_index)
            .unwrap_or(0);

        self.height = self.viewport_size.1.saturating_sub(1);

        let picker_height = if has_focus {
            editor
                .picker
                .height(editor.config.values.picker_max_height.get()) as u16
        } else {
            0
        };

        self.height = self.height.saturating_sub(picker_height);

        if focused_line_index < self.scroll {
            self.scroll = focused_line_index;
        } else if focused_line_index >= self.scroll + self.height as usize {
            self.scroll = focused_line_index + 1 - self.height as usize;
        }
    }
}

pub struct ClientRef<'a> {
    pub target: TargetClient,
    pub client: &'a mut Client,
    pub buffer: &'a mut Vec<u8>,
}

#[derive(Default)]
pub struct ClientCollection {
    local: Client,
    remotes: Vec<Option<Client>>,
    local_buf: Vec<u8>,
    remote_bufs: Vec<Vec<u8>>,
}

impl ClientCollection {
    pub fn on_client_joined(&mut self, client_handle: ConnectionWithClientHandle) {
        let index = client_handle.into_index();
        let min_len = index + 1;
        if min_len > self.remotes.len() {
            self.remotes.resize_with(min_len, || None);
        }
        self.remotes[index] = Some(Client::default());
        if min_len > self.remote_bufs.len() {
            self.remote_bufs.resize_with(min_len, || Vec::new());
        }
    }

    pub fn on_client_left(&mut self, client_handle: ConnectionWithClientHandle) {
        let index = client_handle.into_index();
        self.remotes[index] = None;
        self.remote_bufs[index].clear();
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

    pub fn client_refs<'a>(&'a mut self) -> impl Iterator<Item = ClientRef<'a>> {
        let remotes = self
            .remotes
            .iter_mut()
            .enumerate()
            .zip(self.remote_bufs.iter_mut())
            .flat_map(|((i, c), b)| {
                c.as_mut().map(move |c| ClientRef {
                    target: TargetClient::Remote(ConnectionWithClientHandle::from_index(i)),
                    client: c,
                    buffer: b,
                })
            });

        std::iter::once(ClientRef {
            target: TargetClient::Local,
            client: &mut self.local,
            buffer: &mut self.local_buf,
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
