use crate::{buffer::BufferHandle, buffer_view::BufferView, cursor::Cursor};

#[derive(Clone, Copy)]
pub struct NavigationHistorySnapshotRef<'a> {
    pub buffer_handle: BufferHandle,
    pub cursors: &'a [Cursor],
}

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
    pub fn add_snapshot(&mut self, buffer_view: &BufferView) {
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

    pub fn navigate_backward(&mut self) -> Option<NavigationHistorySnapshotRef> {
        if self.current_index == 0 {
            return None;
        }

        self.current_index -= 1;
        let snapshot = &self.snapshots[self.current_index];

        Some(NavigationHistorySnapshotRef {
            buffer_handle: snapshot.buffer_handle,
            cursors: std::slice::from_ref(&snapshot.cursor),
        })
    }

    pub fn navigate_forward(&mut self) -> Option<NavigationHistorySnapshotRef> {
        if self.current_index == self.snapshots.len() {
            return None;
        }

        let snapshot = &self.snapshots[self.current_index];
        self.current_index += 1;

        Some(NavigationHistorySnapshotRef {
            buffer_handle: snapshot.buffer_handle,
            cursors: std::slice::from_ref(&snapshot.cursor),
        })
    }
}
