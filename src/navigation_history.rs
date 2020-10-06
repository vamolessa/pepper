use crate::{
    buffer::BufferHandle,
    buffer_view::{BufferView, BufferViewCollection},
    client::{ClientCollection, TargetClient},
    cursor::Cursor,
};

pub enum NavigationDirection {
    Forward,
    Backward,
}

#[derive(Clone, Copy)]
struct NavigationHistorySnapshot {
    buffer_handle: BufferHandle,
    cursor_range: (usize, usize),
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
    pub fn save_client_snapshot(
        clients: &mut ClientCollection,
        buffer_views: &BufferViewCollection,
        target_client: TargetClient,
    ) {
        let client = match clients.get_mut(target_client) {
            Some(client) => client,
            None => return,
        };
        let view_handle = match client.current_buffer_view_handle {
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
                    .zip(self.cursors[last.cursor_range.0..last.cursor_range.1].iter())
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
            cursor_range: (cursors_start_index, self.cursors.len()),
        });
    }

    pub fn move_in_history(
        clients: &mut ClientCollection,
        buffer_views: &mut BufferViewCollection,
        target_client: TargetClient,
        direction: NavigationDirection,
    ) {
        let client = match clients.get_mut(target_client) {
            Some(client) => client,
            None => return,
        };

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
                let snapshot = history.snapshots[history_index];
                snapshot
            }
            NavigationDirection::Backward => {
                if history_index == 0 {
                    return;
                }

                if history_index == history.snapshots.len() {
                    if let Some(buffer_view) = client
                        .current_buffer_view_handle
                        .and_then(|h| buffer_views.get(h))
                    {
                        history.add_snapshot(buffer_view)
                    }
                }

                history_index -= 1;
                history.snapshots[history_index]
            }
        };

        history.state = NavigationState::IterIndex(history_index);

        let view_handle = buffer_views
            .buffer_view_handle_from_buffer_handle(target_client, snapshot.buffer_handle);
        client.current_buffer_view_handle = Some(view_handle);

        let mut cursors = match buffer_views.get_mut(view_handle) {
            Some(view) => view.cursors.mut_guard(),
            None => return,
        };
        cursors.clear();
        for cursor in history.cursors[snapshot.cursor_range.0..snapshot.cursor_range.1].iter() {
            cursors.add(*cursor);
        }
    }

    pub fn remove_snapshots_with_buffer_handle(&mut self, buffer_handle: BufferHandle) {
        for i in (0..self.snapshots.len()).rev() {
            let snapshot = self.snapshots[i];
            if snapshot.buffer_handle == buffer_handle {
                self.cursors.drain(snapshot.cursor_range.0..snapshot.cursor_range.1);
                self.snapshots.remove(i);

                let cursor_range_len = snapshot.cursor_range.1 - snapshot.cursor_range.0;
                for s in &mut self.snapshots[i..] {
                    if s.cursor_range.0 >= snapshot.cursor_range.1 {
                        s.cursor_range.0 -= cursor_range_len;
                        s.cursor_range.1 -= cursor_range_len;
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
