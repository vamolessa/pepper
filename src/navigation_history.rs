use crate::{
    buffer::BufferHandle,
    buffer_position::BufferPosition,
    buffer_view::{BufferView, BufferViewCollection},
    client::Client,
    cursor::Cursor,
    editor::Editor,
};

#[derive(Clone, Copy)]
pub enum NavigationMovement {
    Forward,
    Backward,
    PreviousBuffer,
}

#[derive(Clone)]
struct NavigationHistorySnapshot {
    pub buffer_handle: BufferHandle,
    pub position: BufferPosition,
}

pub struct NavigationHistory {
    snapshots: Vec<NavigationHistorySnapshot>,
    current_snapshot_index: u32,
    previous_movement: NavigationMovement,
}

impl NavigationHistory {
    pub fn clear(&mut self) {
        self.snapshots.clear();
        self.current_snapshot_index = 0;
        self.previous_movement = NavigationMovement::Forward;
    }

    pub fn save_client_snapshot(client: &mut Client, buffer_views: &BufferViewCollection) {
        if let Some(handle) = client.buffer_view_handle() {
            let buffer_view = buffer_views.get(handle);
            client.navigation_history.add_snapshot(buffer_view);
        }
    }

    fn add_snapshot(&mut self, buffer_view: &BufferView) {
        self.snapshots.truncate(self.current_snapshot_index as _);

        let buffer_handle = buffer_view.buffer_handle;
        let position = buffer_view.cursors.main_cursor().position;

        if self
            .snapshots
            .last()
            .map(|s| s.buffer_handle == buffer_handle && s.position == position)
            .unwrap_or(false)
        {
            return;
        }

        self.snapshots.push(NavigationHistorySnapshot {
            buffer_handle,
            position,
        });
        self.current_snapshot_index = self.snapshots.len() as _;
    }

    pub fn move_in_history(client: &mut Client, editor: &mut Editor, movement: NavigationMovement) {
        let previous_movement = client.navigation_history.previous_movement;
        client.navigation_history.previous_movement = movement;

        match movement {
            NavigationMovement::Forward => {
                if client.navigation_history.current_snapshot_index + 1
                    >= client.navigation_history.snapshots.len() as _
                {
                    return;
                }

                client.navigation_history.current_snapshot_index += 1;
            }
            NavigationMovement::Backward => {
                if client.navigation_history.current_snapshot_index == 0 {
                    return;
                }

                if client.navigation_history.current_snapshot_index
                    == client.navigation_history.snapshots.len() as _
                {
                    Self::save_client_snapshot(client, &editor.buffer_views);
                    if client.navigation_history.current_snapshot_index > 1 {
                        client.navigation_history.current_snapshot_index -= 1;
                    }
                }

                client.navigation_history.current_snapshot_index -= 1;
            }
            NavigationMovement::PreviousBuffer => {
                let buffer_view_handle = client.buffer_view_handle();
                let buffer_handle =
                    buffer_view_handle.map(|h| editor.buffer_views.get(h).buffer_handle);

                let index = client.navigation_history.current_snapshot_index as usize;
                match previous_movement {
                    NavigationMovement::Forward => match client.navigation_history.snapshots
                        [..index]
                        .iter()
                        .rposition(|s| Some(s.buffer_handle) != buffer_handle)
                    {
                        Some(i) => {
                            client.navigation_history.current_snapshot_index =
                                client.navigation_history.snapshots.len().min(i + 1) as _;
                            Self::save_client_snapshot(client, &editor.buffer_views);
                        }
                        None => return,
                    },
                    NavigationMovement::Backward => match client.navigation_history.snapshots
                        [index..]
                        .iter()
                        .enumerate()
                        .skip_while(|(_, s)| Some(s.buffer_handle) == buffer_handle)
                        .take_while(|(_, s)| Some(s.buffer_handle) != buffer_handle)
                        .map(|(i, _)| i)
                        .last()
                    {
                        Some(i) => {
                            client.navigation_history.current_snapshot_index =
                                client.navigation_history.snapshots.len().min(index + i + 1) as _;
                            Self::save_client_snapshot(client, &editor.buffer_views);
                        }
                        None => return,
                    },
                    NavigationMovement::PreviousBuffer => (),
                }

                let snapshots = &mut client.navigation_history.snapshots;
                let len = snapshots.len();
                if len < 2 {
                    return;
                }

                if let Some(handle) = buffer_view_handle {
                    let buffer_view = editor.buffer_views.get(handle);
                    let last = &mut snapshots[len - 1];
                    last.buffer_handle = buffer_view.buffer_handle;
                    last.position = buffer_view.cursors.main_cursor().position;

                    snapshots.swap(len - 2, len - 1);
                }

                client.navigation_history.current_snapshot_index = (len - 1) as _;
            }
        }

        let snapshot = &client.navigation_history.snapshots
            [client.navigation_history.current_snapshot_index as usize];

        let position = editor
            .buffers
            .get(snapshot.buffer_handle)
            .content()
            .saturate_position(snapshot.position);

        let view_handle = editor
            .buffer_views
            .buffer_view_handle_from_buffer_handle(client.handle(), snapshot.buffer_handle);

        let mut cursors = editor.buffer_views.get_mut(view_handle).cursors.mut_guard();
        cursors.clear();
        cursors.add(Cursor {
            anchor: position,
            position,
        });

        client.set_buffer_view_handle(Some(view_handle), &mut editor.events);
    }

    pub fn remove_snapshots_with_buffer_handle(&mut self, buffer_handle: BufferHandle) {
        for i in (0..self.snapshots.len()).rev() {
            let snapshot = self.snapshots[i].clone();
            if snapshot.buffer_handle == buffer_handle {
                self.snapshots.remove(i);

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
            snapshots: Vec::default(),
            current_snapshot_index: 0,
            previous_movement: NavigationMovement::Forward,
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

        assert_eq!(3, client.navigation_history.snapshots.len());
    }

    #[test]
    fn move_to_previous_buffer_three_times() {
        let (mut editor, mut client) = setup();

        NavigationHistory::move_in_history(
            &mut client,
            &mut editor,
            NavigationMovement::PreviousBuffer,
        );
        assert_eq!(2, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(
            &mut client,
            &mut editor,
            NavigationMovement::PreviousBuffer,
        );
        assert_eq!(2, client.navigation_history.current_snapshot_index);
        assert_eq!(2, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(
            &mut client,
            &mut editor,
            NavigationMovement::PreviousBuffer,
        );
        assert_eq!(2, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_handle(&client, &editor).0);

        assert_eq!(3, client.navigation_history.snapshots.len());
    }

    #[test]
    fn navigate_back_then_move_to_previous_buffer() {
        let (mut editor, mut client) = setup();

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(0, client.navigation_history.current_snapshot_index);
        assert_eq!(0, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Forward);
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_handle(&client, &editor).0);

        assert_eq!(3, client.navigation_history.snapshots.len());

        NavigationHistory::move_in_history(
            &mut client,
            &mut editor,
            NavigationMovement::PreviousBuffer,
        );
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(0, buffer_handle(&client, &editor).0);

        assert_eq!(2, client.navigation_history.snapshots.len());

        NavigationHistory::move_in_history(
            &mut client,
            &mut editor,
            NavigationMovement::PreviousBuffer,
        );
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_handle(&client, &editor).0);

        NavigationHistory::move_in_history(
            &mut client,
            &mut editor,
            NavigationMovement::PreviousBuffer,
        );
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(0, buffer_handle(&client, &editor).0);

        assert_eq!(2, client.navigation_history.snapshots.len());
    }
}
