use std::ops::Range;

use crate::{
    buffer::BufferHandle,
    buffer_view::{BufferView, BufferViewCollection},
    client::{ClientHandle, ClientManager},
    cursor::Cursor,
    editor::Editor,
};

pub enum NavigationDirection {
    Forward,
    Backward,
}

#[derive(Clone)]
struct NavigationHistorySnapshot {
    buffer_handle: BufferHandle,
    cursor_range: Range<usize>,
}

enum NavigationState {
    IterIndex(usize),
    Insert,
}

pub struct NavigationHistory {
    cursors: Vec<Cursor>,
    snapshots: Vec<NavigationHistorySnapshot>,
    state: NavigationState,
}

impl NavigationHistory {
    pub fn clear(&mut self) {
        self.cursors.clear();
        self.snapshots.clear();
        self.state = NavigationState::IterIndex(0);
    }

    pub fn save_client_snapshot(
        clients: &mut ClientManager,
        handle: ClientHandle,
        buffer_views: &BufferViewCollection,
    ) {
        let client = match clients.get_mut(handle) {
            Some(client) => client,
            None => return,
        };
        let view_handle = match client.buffer_view_handle() {
            Some(handle) => handle,
            None => return,
        };
        let buffer_view = match buffer_views.get(view_handle) {
            Some(view) => view,
            None => return,
        };

        client.navigation_history.add_snapshot(buffer_view);
    }

    fn add_snapshot(&mut self, buffer_view: &BufferView) {
        let buffer_handle = buffer_view.buffer_handle;
        let cursors = &buffer_view.cursors[..];

        if let NavigationState::IterIndex(index) = self.state {
            self.snapshots.truncate(index);
        }
        self.state = NavigationState::Insert;

        if let Some(last) = self.snapshots.last() {
            if last.buffer_handle == buffer_handle {
                let same_cursors = cursors
                    .iter()
                    .zip(self.cursors[last.cursor_range.clone()].iter())
                    .all(|(a, b)| *a == *b);
                if same_cursors {
                    return;
                }
            }
        }

        let cursors_start_index = self.cursors.len();
        for c in cursors {
            self.cursors.push(*c);
        }

        self.snapshots.push(NavigationHistorySnapshot {
            buffer_handle,
            cursor_range: cursors_start_index..self.cursors.len(),
        });
    }

    pub fn move_in_history(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        direction: NavigationDirection,
    ) {
        let client = match clients.get_mut(client_handle) {
            Some(client) => client,
            None => return,
        };

        let current_buffer_view_handle = client.buffer_view_handle();

        let history = &mut client.navigation_history;
        let mut history_index = match history.state {
            NavigationState::IterIndex(index) => index,
            NavigationState::Insert => history.snapshots.len(),
        };

        let snapshot = match direction {
            NavigationDirection::Forward => {
                if history_index + 1 >= history.snapshots.len() {
                    return;
                }

                history_index += 1;
                let snapshot = history.snapshots[history_index].clone();
                snapshot
            }
            NavigationDirection::Backward => {
                if history_index == 0 {
                    return;
                }

                if history_index == history.snapshots.len() {
                    if let Some(buffer_view) =
                        current_buffer_view_handle.and_then(|h| editor.buffer_views.get(h))
                    {
                        history.add_snapshot(buffer_view)
                    }
                }

                history_index -= 1;
                history.snapshots[history_index].clone()
            }
        };

        history.state = NavigationState::IterIndex(history_index);

        let view_handle = editor
            .buffer_views
            .buffer_view_handle_from_buffer_handle(client_handle, snapshot.buffer_handle);
        let mut cursors = match editor.buffer_views.get_mut(view_handle) {
            Some(view) => view.cursors.mut_guard(),
            None => return,
        };
        cursors.clear();
        for cursor in history.cursors[snapshot.cursor_range.clone()].iter() {
            cursors.add(*cursor);
        }
        if let Some(buffer) = editor.buffers.get(snapshot.buffer_handle) {
            let buffer = buffer.content();
            for cursor in &mut cursors[..] {
                cursor.anchor = buffer.saturate_position(cursor.anchor);
                cursor.position = buffer.saturate_position(cursor.position);
            }
        }

        if let Some(client) = clients.get_mut(client_handle) {
            client.set_buffer_view_handle(Some(view_handle), &mut editor.events);
        }
    }

    pub fn remove_snapshots_with_buffer_handle(&mut self, buffer_handle: BufferHandle) {
        for i in (0..self.snapshots.len()).rev() {
            let snapshot = self.snapshots[i].clone();
            if snapshot.buffer_handle == buffer_handle {
                self.cursors.drain(snapshot.cursor_range.clone());
                self.snapshots.remove(i);

                let cursor_range_len = snapshot.cursor_range.end - snapshot.cursor_range.start;
                for s in &mut self.snapshots[i..] {
                    if s.cursor_range.start >= snapshot.cursor_range.end {
                        s.cursor_range.start -= cursor_range_len;
                        s.cursor_range.end -= cursor_range_len;
                    }
                }

                if let NavigationState::IterIndex(index) = &mut self.state {
                    if i <= *index && *index > 0 {
                        *index -= 1;
                    }
                }
            }
        }
    }
}

impl Default for NavigationHistory {
    fn default() -> Self {
        Self {
            cursors: Vec::default(),
            snapshots: Vec::default(),
            state: NavigationState::IterIndex(0),
        }
    }
}

