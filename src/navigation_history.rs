use std::ops::Range;

use crate::{
    buffer::BufferHandle,
    buffer_view::{BufferView, BufferViewCollection},
    client::{Client, ClientView},
    cursor::Cursor,
    editor::Editor,
};

pub enum NavigationMovement {
    Current,
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
        let buffer_view = match client
            .buffer_view_handle()
            .and_then(|h| buffer_views.get(h))
        {
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

    pub fn apply_history_state(client: &mut Client, editor: &mut Editor) {
        let history = &mut client.navigation_history;
        let snapshot_index = match history.state {
            NavigationState::IterIndex(index) => index,
            NavigationState::Insert => {
                if history.snapshots.is_empty() {
                    client.set_view(ClientView::None, &mut editor.events);
                    return;
                } else {
                    history.snapshots.len() - 1
                }
            }
        };
        history.state = NavigationState::IterIndex(snapshot_index + 1);

        let snapshot = history.snapshots[snapshot_index].clone();
        apply_snapshot(client, editor, snapshot);
    }

    pub fn move_in_history(client: &mut Client, editor: &mut Editor, movement: NavigationMovement) {
        let current_buffer_view_handle = client.buffer_view_handle();

        let history = &mut client.navigation_history;
        let mut snapshot_index = match history.state {
            NavigationState::IterIndex(index) => index,
            NavigationState::Insert => history.snapshots.len(),
        };

        let snapshot = match movement {
            NavigationMovement::Current => {
                if snapshot_index < history.snapshots.len() {
                    let snapshot = history.snapshots[snapshot_index].clone();
                    snapshot_index += 1;
                    snapshot
                } else {
                    client.set_view(ClientView::None, &mut editor.events);
                    return;
                }
            }
            NavigationMovement::Forward => {
                if snapshot_index + 1 >= history.snapshots.len() {
                    return;
                }

                snapshot_index += 1;
                let snapshot = history.snapshots[snapshot_index].clone();
                snapshot_index += 1;
                snapshot
            }
            NavigationMovement::Backward => {
                if snapshot_index == 0 {
                    return;
                }

                if snapshot_index == history.snapshots.len() {
                    if let Some(buffer_view) =
                        current_buffer_view_handle.and_then(|h| editor.buffer_views.get(h))
                    {
                        history.add_snapshot(buffer_view)
                    }
                }

                snapshot_index -= 1;
                history.snapshots[snapshot_index].clone()
            }
            NavigationMovement::PreviousBuffer => {
                let buffer_view =
                    current_buffer_view_handle.and_then(|h| editor.buffer_views.get(h));

                if snapshot_index < history.snapshots.len() {
                    let buffer_handle = match buffer_view {
                        Some(buffer_view) => buffer_view.buffer_handle,
                        None => history.snapshots[snapshot_index].buffer_handle,
                    };

                    if let Some(i) = history.snapshots[snapshot_index..]
                        .iter()
                        .position(|s| s.buffer_handle != buffer_handle)
                    {
                        snapshot_index += i;
                    } else if let Some(i) = history.snapshots[..snapshot_index]
                        .iter()
                        .rposition(|s| s.buffer_handle != buffer_handle)
                    {
                        snapshot_index = i;
                    } else {
                        return;
                    }
                } else {
                    match buffer_view {
                        Some(buffer_view) => match history
                            .snapshots
                            .iter()
                            .rposition(|s| s.buffer_handle != buffer_view.buffer_handle)
                        {
                            Some(i) => {
                                history.add_snapshot(buffer_view);
                                snapshot_index = i
                            }
                            None => return,
                        },
                        None => {
                            snapshot_index = history.snapshots.len();
                            if snapshot_index == 0 {
                                client.set_view(ClientView::None, &mut editor.events);
                                return;
                            }
                            snapshot_index -= 1;
                        }
                    }
                }

                history.snapshots[snapshot_index].clone()
            }
        };

        history.state = NavigationState::IterIndex(snapshot_index);

        let view_handle = editor
            .buffer_views
            .buffer_view_handle_from_buffer_handle(client.handle(), snapshot.buffer_handle);
        let mut cursors = match editor.buffer_views.get_mut(view_handle) {
            Some(view) => view.cursors.mut_guard(),
            None => return,
        };
        cursors.clear();
        for cursor in client.navigation_history.cursors[snapshot.cursor_range()].iter() {
            cursors.add(*cursor);
        }
        if let Some(buffer) = editor.buffers.get(snapshot.buffer_handle) {
            let buffer = buffer.content();
            for cursor in &mut cursors[..] {
                cursor.anchor = buffer.saturate_position(cursor.anchor);
                cursor.position = buffer.saturate_position(cursor.position);
            }
        }

        client.set_view(ClientView::Buffer(view_handle), &mut editor.events);
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

fn apply_snapshot(client: &mut Client, editor: &mut Editor, snapshot: NavigationHistorySnapshot) {
    let view_handle = editor
        .buffer_views
        .buffer_view_handle_from_buffer_handle(client.handle(), snapshot.buffer_handle);
    let mut cursors = match editor.buffer_views.get_mut(view_handle) {
        Some(view) => view.cursors.mut_guard(),
        None => return,
    };
    cursors.clear();
    for cursor in client.navigation_history.cursors[snapshot.cursor_range()].iter() {
        cursors.add(*cursor);
    }
    if let Some(buffer) = editor.buffers.get(snapshot.buffer_handle) {
        let buffer = buffer.content();
        for cursor in &mut cursors[..] {
            cursor.anchor = buffer.saturate_position(cursor.anchor);
            cursor.position = buffer.saturate_position(cursor.position);
        }
    }

    client.set_view(ClientView::Buffer(view_handle), &mut editor.events);
}

