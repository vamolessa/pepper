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

pub struct NavigationHistory {
    cursors: Vec<Cursor>,
    snapshots: Vec<NavigationHistorySnapshot>,
    current_snapshot_index: u32,
}

impl NavigationHistory {
    pub fn clear(&mut self) {
        self.cursors.clear();
        self.snapshots.clear();
        self.current_snapshot_index = 0;
    }

    pub fn save_client_snapshot(client: &mut Client, buffer_views: &BufferViewCollection) {
        if let Some(handle) = client.buffer_view_handle() {
            let buffer_view = buffer_views.get(handle);
            client.navigation_history.add_snapshot(buffer_view);
        }
    }

    fn buffer_view_equals_to_snapshot(
        &self,
        buffer_view: &BufferView,
        snapshot: &NavigationHistorySnapshot,
    ) -> bool {
        if snapshot.buffer_handle != buffer_view.buffer_handle {
            return false;
        }

        let buffer_view_cursors = &buffer_view.cursors[..];
        let snapshot_cursors = &self.cursors[snapshot.cursor_range()];

        if buffer_view_cursors.len() != snapshot_cursors.len() {
            return false;
        }

        let same_cursors = buffer_view_cursors
            .iter()
            .zip(snapshot_cursors.iter())
            .all(|(a, b)| a == b);
        same_cursors
    }

    fn add_snapshot(&mut self, buffer_view: &BufferView) {
        self.snapshots.truncate(self.current_snapshot_index as _);

        match self.snapshots.last() {
            Some(snapshot) => {
                self.cursors.truncate(snapshot.cursor_range().end);
                if self.buffer_view_equals_to_snapshot(buffer_view, snapshot) {
                    return;
                }
            }
            None => self.cursors.clear(),
        }

        let cursors_start_index = self.cursors.len();
        for c in &buffer_view.cursors[..] {
            self.cursors.push(*c);
        }

        self.snapshots.push(NavigationHistorySnapshot {
            buffer_handle: buffer_view.buffer_handle,
            cursor_range: cursors_start_index as u32..self.cursors.len() as u32,
        });
        self.current_snapshot_index = self.snapshots.len() as _;
    }

    fn move_to_previous_buffer(&self) {
        /*
        match current_buffer_view_handle {
            Some(handle) => {
                let buffer_view = editor.buffer_views.get(handle);
                let buffer_handle = buffer_view.buffer_handle;

                match client.navigation_history.snapshots[this.current_snapshot_index as usize..]
                    .iter()
                    .enumerate()
                    .skip_while(|(_, s)| s.buffer_handle == buffer_handle)
                    .take_while(|(_, s)| s.buffer_handle != buffer_handle)
                    .map(|(i, _)| i)
                    .last()
                {
                    Some(i) => {
                        let should_save_snapshot = client
                            .navigation_history
                            .snapshots
                            .get(this.current_snapshot_index as usize)
                            .map(|s| {
                                !client
                                    .navigation_history
                                    .buffer_view_equals_to_snapshot(buffer_view, s)
                            })
                            .unwrap_or(true);

                        this.current_snapshot_index += (i + 1) as u32;

                        if should_save_snapshot {
                            Self::save_client_snapshot(client, &editor.buffer_views);
                        }

                        if this.current_snapshot_index
                            >= client.navigation_history.snapshots.len() as _
                        {
                            match client
                                .navigation_history
                                .snapshots
                                .iter()
                                .rposition(|s| s.buffer_handle != buffer_handle)
                            {
                                Some(i) => this.current_snapshot_index = i as _,
                                None => return,
                            }
                        }
                    }
                    None => {
                        Self::save_client_snapshot(client, &editor.buffer_views);
                        match client
                            .navigation_history
                            .snapshots
                            .iter()
                            .rposition(|s| s.buffer_handle != buffer_handle)
                        {
                            Some(i) => this.current_snapshot_index = i as _,
                            None => return,
                        }
                    }
                }
            }
            None => match client.navigation_history.snapshots.len().checked_sub(1) {
                Some(i) => this.current_snapshot_index = i as _,
                None => return,
            },
        }
        */
    }

    pub fn move_in_history(client: &mut Client, editor: &mut Editor, movement: NavigationMovement) {
        let mut this = &mut client.navigation_history;
        let snapshot = match movement {
            NavigationMovement::Forward => {
                if this.current_snapshot_index + 1 >= this.snapshots.len() as _ {
                    return;
                }

                this.current_snapshot_index += 1;
                this.snapshots[this.current_snapshot_index as usize].clone()
            }
            NavigationMovement::Backward => {
                if this.current_snapshot_index == 0 {
                    return;
                }

                if this.current_snapshot_index == this.snapshots.len() as _ {
                    Self::save_client_snapshot(client, &editor.buffer_views);
                    this = &mut client.navigation_history;
                }

                this.current_snapshot_index -= 1;
                this.snapshots[this.current_snapshot_index as usize].clone()
            }
            NavigationMovement::PreviousBuffer => {
                this.move_to_previous_buffer();
                todo!()
            }
        };

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

                if self.current_snapshot_index > 0 && i <= self.current_snapshot_index as _ {
                    self.current_snapshot_index -= 1;
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
            current_snapshot_index: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    fn setup() -> (Editor, Client) {
        let mut client = Client::default();
        let mut editor = Editor::new(PathBuf::new());

        let buffer_a = editor.buffers.add_new();
        assert_eq!(0, buffer_a.handle().0);
        let buffer_b = editor.buffers.add_new();
        assert_eq!(1, buffer_b.handle().0);
        let buffer_c = editor.buffers.add_new();
        assert_eq!(2, buffer_c.handle().0);

        let view_a = editor
            .buffer_views
            .add_new(client.handle(), BufferHandle(0));
        let view_b = editor
            .buffer_views
            .add_new(client.handle(), BufferHandle(1));
        let view_c = editor
            .buffer_views
            .add_new(client.handle(), BufferHandle(2));

        NavigationHistory::save_client_snapshot(&mut client, &editor.buffer_views);
        client.set_buffer_view_handle(Some(view_a), &mut editor.events);
        NavigationHistory::save_client_snapshot(&mut client, &editor.buffer_views);
        client.set_buffer_view_handle(Some(view_b), &mut editor.events);
        NavigationHistory::save_client_snapshot(&mut client, &editor.buffer_views);
        client.set_buffer_view_handle(Some(view_c), &mut editor.events);
        NavigationHistory::save_client_snapshot(&mut client, &editor.buffer_views);

        (editor, client)
    }

    fn buffer_handle(client: &Client, editor: &Editor) -> BufferHandle {
        let buffer_view_handle = client.buffer_view_handle().unwrap();
        let buffer_view = editor.buffer_views.get(buffer_view_handle);
        buffer_view.buffer_handle
    }

    #[test]
    fn move_back_and_forward_in_history() {
        let (mut editor, mut client) = setup();

        assert_eq!(3, client.navigation_history.current_snapshot_index);
        assert_eq!(2, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Forward);
        assert_eq!(3, client.navigation_history.current_snapshot_index);
        assert_eq!(2, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(2, client.navigation_history.current_snapshot_index);
        assert_eq!(2, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(0, client.navigation_history.current_snapshot_index);
        assert_eq!(0, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(0, client.navigation_history.current_snapshot_index);
        assert_eq!(0, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Forward);
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_handle(&client, &editor).0);
    }

    #[test]
    fn move_to_previous_buffer() {
        /*
        let (mut editor, mut client) = setup();

        NavigationHistory::move_in_history(
            &mut client,
            &mut editor,
            NavigationMovement::PreviousBuffer,
        );
        */
    }
}

