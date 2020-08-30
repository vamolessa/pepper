use crate::{
    buffer_view::BufferViewHandle, connection::ConnectionWithClientHandle, cursor::Cursor,
    select::SelectEntryCollection,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TargetClient {
    Local,
    Remote(ConnectionWithClientHandle),
}

impl TargetClient {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => TargetClient::Local,
            _ => TargetClient::Remote(ConnectionWithClientHandle::from_index(index - 1)),
        }
    }

    pub fn into_index(self) -> usize {
        match self {
            TargetClient::Local => 0,
            TargetClient::Remote(handle) => handle.into_index() + 1,
        }
    }
}

pub struct Client {
    pub current_buffer_view_handle: Option<BufferViewHandle>,
    pub width: u16,
    pub height: u16,
    pub text_scroll: usize,
    pub select_scroll: usize,
    pub text_height: u16,
    pub select_height: u16,
}

impl Client {
    pub fn scroll(
        &mut self,
        has_focus: bool,
        main_cursor: Cursor,
        select_entries: &SelectEntryCollection,
    ) {
        self.text_height = self.height - 1;

        let select_entry_count = if has_focus {
            select_entries.len() as u16
        } else {
            0
        };

        self.select_height = select_entry_count.min(self.text_height / 2);
        self.text_height -= self.select_height;

        let cursor_position = main_cursor.position;
        if cursor_position.line_index < self.text_scroll {
            self.text_scroll = cursor_position.line_index;
        } else if cursor_position.line_index >= self.text_scroll + self.text_height as usize {
            self.text_scroll = cursor_position.line_index + 1 - self.text_height as usize;
        }

        let selected_index = select_entries.selected_index;
        if selected_index < self.select_scroll {
            self.select_scroll = selected_index;
        } else if selected_index >= self.select_scroll + self.select_height as usize {
            self.select_scroll = selected_index + 1 - self.select_height as usize;
        }
    }
}

pub struct ClientCollection {
    pub local: Client,
    pub remotes: Vec<Option<Client>>,
}

impl ClientCollection {
    pub fn on_client_joined(&mut self, client_handle: ConnectionWithClientHandle) {
        let min_len = client_handle.into_index() + 1;
        if min_len > self.remotes.len() {
            self.remotes.resize_with(min_len, || None);
        }
    }

    pub fn on_client_left(&mut self, client_handle: ConnectionWithClientHandle) {
        self.remotes[client_handle.into_index()] = None;
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
