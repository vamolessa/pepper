use std::ops::Range;

use crate::{
    buffer::BufferHandle,
    buffer_view::{BufferView, BufferViewCollection},
    client::Client,
    cursor::Cursor,
    editor::Editor,
};

pub enum NavigationMovement {
    Forward,
    Backward,
    PreviousBuffer,
}

#[derive(Clone)]
struct NavigationHistorySnapshot {
    pub buffer_handle: BufferHandle,
    pub cursor_range: Range<u32>,
}
impl NavigationHistorySnapshot {
    pub fn cursor_range(&self) -> Range<usize> {
        self.cursor_range.start as usize..self.cursor_range.end as usize
    }
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

    pub fn save_client_snapshot(client: &mut Client, buffer_views: &BufferViewCollection) {
        if let Some(handle) = client.buffer_view_handle() {
            let buffer_view = buffer_views.get(handle);
            client.navigation_history.add_snapshot(buffer_view);
        }
    }

    fn add_snapshot(&mut self, buffer_view: &BufferView) {
        let buffer_handle = buffer_view.buffer_handle;
        let cursors = &buffer_view.cursors[..];

        if let NavigationState::IterIndex(index) = self.state {
            self.snapshots.truncate(index);
            match self.snapshots.last() {
                Some(snapshot) => self.cursors.truncate(snapshot.cursor_range().end),
                None => self.cursors.clear(),
            }
        }
        self.state = NavigationState::Insert;

        if let Some(last) = self.snapshots.last() {
            if last.buffer_handle == buffer_handle {
                let same_cursors = cursors
                    .iter()
                    .zip(self.cursors[last.cursor_range()].iter())
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
            cursor_range: cursors_start_index as u32..self.cursors.len() as u32,
        });
    }

    pub fn move_in_history(client: &mut Client, editor: &mut Editor, movement: NavigationMovement) {
        let current_buffer_view_handle = client.buffer_view_handle();

        let mut snapshot_index = match client.navigation_history.state {
            NavigationState::IterIndex(index) => index,
            NavigationState::Insert => client.navigation_history.snapshots.len(),
        };

        let snapshot = match movement {
            NavigationMovement::Forward => {
                if snapshot_index + 1 >= client.navigation_history.snapshots.len() {
                    return;
                }

                snapshot_index += 1;
                let snapshot = client.navigation_history.snapshots[snapshot_index].clone();
                snapshot_index += 1;
                snapshot
            }
            NavigationMovement::Backward => {
                if snapshot_index == 0 {
                    return;
                }

                if snapshot_index == client.navigation_history.snapshots.len() {
                    if let Some(handle) = current_buffer_view_handle {
                        let buffer_view = editor.buffer_views.get(handle);
                        client.navigation_history.add_snapshot(buffer_view)
                    }
                }

                snapshot_index -= 1;
                client.navigation_history.snapshots[snapshot_index].clone()
            }
            NavigationMovement::PreviousBuffer => {
                if snapshot_index < client.navigation_history.snapshots.len() {
                    let buffer_handle = match current_buffer_view_handle {
                        Some(handle) => editor.buffer_views.get(handle).buffer_handle,
                        None => client.navigation_history.snapshots[snapshot_index].buffer_handle,
                    };

                    if let Some(i) = client.navigation_history.snapshots[snapshot_index..]
                        .iter()
                        .position(|s| s.buffer_handle != buffer_handle)
                    {
                        snapshot_index += i;
                    } else if let Some(i) = client.navigation_history.snapshots[..snapshot_index]
                        .iter()
                        .rposition(|s| s.buffer_handle != buffer_handle)
                    {
                        snapshot_index = i;
                    } else {
                        return;
                    }
                } else {
                    match current_buffer_view_handle {
                        Some(handle) => {
                            let buffer_view = editor.buffer_views.get(handle);
                            match client
                                .navigation_history
                                .snapshots
                                .iter()
                                .rposition(|s| s.buffer_handle != buffer_view.buffer_handle)
                            {
                                Some(i) => {
                                    client.navigation_history.add_snapshot(buffer_view);
                                    snapshot_index = i
                                }
                                None => return,
                            }
                        }
                        None => {
                            snapshot_index = client.navigation_history.snapshots.len();
                            if snapshot_index == 0 {
                                client.set_buffer_view_handle(None, &mut editor.events);
                                return;
                            }
                            snapshot_index -= 1;
                        }
                    }
                }

                client.navigation_history.snapshots[snapshot_index].clone()
            }
        };

        client.navigation_history.state = NavigationState::IterIndex(snapshot_index);

        let view_handle = editor
            .buffer_views
            .buffer_view_handle_from_buffer_handle(client.handle(), snapshot.buffer_handle);
        let mut cursors = editor.buffer_views.get_mut(view_handle).cursors.mut_guard();
        cursors.clear();
        for cursor in client.navigation_history.cursors[snapshot.cursor_range()].iter() {
            cursors.add(*cursor);
        }
        let buffer = editor.buffers.get(snapshot.buffer_handle).content();
        for cursor in &mut cursors[..] {
            cursor.anchor = buffer.saturate_position(cursor.anchor);
            cursor.position = buffer.saturate_position(cursor.position);
        }

        client.set_buffer_view_handle(Some(view_handle), &mut editor.events);
    }

    pub fn remove_snapshots_with_buffer_handle(&mut self, buffer_handle: BufferHandle) {
        for i in (0..self.snapshots.len()).rev() {
            let snapshot = self.snapshots[i].clone();
            if snapshot.buffer_handle == buffer_handle {
                self.cursors.drain(snapshot.cursor_range());
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
