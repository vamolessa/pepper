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
    cursor: Cursor,
}

#[derive(Default)]
pub struct NavigationHistory {
    snapshots: Vec<NavigationHistorySnapshot>,
    current_index: usize,
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
        let cursor = *buffer_view.cursors.main_cursor();

        self.snapshots.truncate(self.current_index);

        if let Some(last) = self.snapshots.last() {
            if last.buffer_handle == buffer_handle && last.cursor == cursor {
                return;
            }
        }

        self.snapshots.push(NavigationHistorySnapshot {
            buffer_handle,
            cursor,
        });
        self.current_index = self.snapshots.len();
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
        let snapshot = match direction {
            NavigationDirection::Forward => {
                if history.current_index == history.snapshots.len() {
                    return;
                }

                let snapshot = history.snapshots[history.current_index];
                history.current_index += 1;
                snapshot
            }
            NavigationDirection::Backward => {
                if history.current_index == history.snapshots.len() {
                    if let Some(buffer_view) = client
                        .current_buffer_view_handle
                        .and_then(|h| buffer_views.get(h))
                    {
                        history.add_snapshot(buffer_view)
                    }
                }

                if history.current_index == 0 {
                    return;
                }

                history.current_index -= 1;
                history.snapshots[history.current_index]
            }
        };

        let view_handle = buffer_views
            .buffer_view_handle_from_buffer_handle(target_client, snapshot.buffer_handle);
        client.current_buffer_view_handle = Some(view_handle);

        let mut cursors = match buffer_views.get_mut(view_handle) {
            Some(view) => view.cursors.mut_guard(),
            None => return,
        };
        cursors.clear();
        for cursor in std::slice::from_ref(&snapshot.cursor) {
            cursors.add(*cursor);
        }
    }

    pub fn remove_snapshots_with_buffer_handle(&mut self, buffer_handle: BufferHandle) {
        for i in (0..self.snapshots.len()).rev() {
            if self.snapshots[i].buffer_handle == buffer_handle {
                self.snapshots.remove(i);
                if i <= self.current_index && self.current_index > 0 {
                    self.current_index -= 1;
                }
            }
        }
    }
}
