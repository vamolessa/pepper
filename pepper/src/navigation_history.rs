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
}

#[derive(Clone)]
struct NavigationHistorySnapshot {
    pub buffer_handle: BufferHandle,
    pub position: BufferPosition,
}

#[derive(Default)]
pub struct NavigationHistory {
    snapshots: Vec<NavigationHistorySnapshot>,
    current_snapshot_index: u32,
    on_previous_buffer: bool,
}

impl NavigationHistory {
    pub fn clear(&mut self) {
        self.snapshots.clear();
        self.current_snapshot_index = 0;
        self.on_previous_buffer = false;
    }

    pub fn save_snapshot(client: &mut Client, buffer_views: &BufferViewCollection) {
        let buffer_view_handle = match client.buffer_view_handle() {
            Some(handle) => handle,
            None => return,
        };
        let buffer_view = buffer_views.get(buffer_view_handle);

        let this = &mut client.navigation_history;
        if this.on_previous_buffer {
            this.current_snapshot_index = this.snapshots.len() as _;
        }
        this.snapshots.truncate(this.current_snapshot_index as _);
        this.on_previous_buffer = false;

        let buffer_handle = buffer_view.buffer_handle;
        let position = buffer_view.cursors.main_cursor().position;

        if this
            .snapshots
            .last()
            .map(|s| s.buffer_handle == buffer_handle && s.position == position)
            .unwrap_or(false)
        {
            return;
        }

        this.snapshots.push(NavigationHistorySnapshot {
            buffer_handle,
            position,
        });
        this.current_snapshot_index = this.snapshots.len() as _;
    }

    pub fn move_in_history(client: &mut Client, editor: &mut Editor, movement: NavigationMovement) {
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
                    Self::save_snapshot(client, &editor.buffer_views);
                    if client.navigation_history.current_snapshot_index > 1 {
                        client.navigation_history.current_snapshot_index -= 1;
                    }
                }

                client.navigation_history.current_snapshot_index -= 1;
            }
        }

        let snapshot = &client.navigation_history.snapshots
            [client.navigation_history.current_snapshot_index as usize];

        let position = editor
            .buffers
            .get(snapshot.buffer_handle)
            .content()
            .saturate_position(snapshot.position);

        let buffer_view_handle = editor
            .buffer_views
            .buffer_view_handle_from_buffer_handle(client.handle(), snapshot.buffer_handle);

        let mut cursors = editor
            .buffer_views
            .get_mut(buffer_view_handle)
            .cursors
            .mut_guard();
        cursors.clear();
        cursors.add(Cursor {
            anchor: position,
            position,
        });

        client.set_buffer_view_handle_no_history(Some(buffer_view_handle), &mut editor.events);
        client.navigation_history.on_previous_buffer = false;
    }

    pub fn move_to_previous_buffer(client: &mut Client, editor: &mut Editor) {
        fn save_snapshot_if_current_buffer_is_different_from_last(
            client: &mut Client,
            buffer_view: &BufferView,
        ) {
            if let Some(current_snapshot) = client
                .navigation_history
                .snapshots
                .get(client.navigation_history.current_snapshot_index as usize)
            {
                if current_snapshot.buffer_handle == buffer_view.buffer_handle {
                    return;
                }
            }

            client
                .navigation_history
                .snapshots
                .push(NavigationHistorySnapshot {
                    buffer_handle: buffer_view.buffer_handle,
                    position: buffer_view.cursors.main_cursor().position,
                });
        }

        let current_buffer_view = client
            .buffer_view_handle()
            .map(|h| editor.buffer_views.get(h));

        if let Some(current_buffer_view) = current_buffer_view {
            save_snapshot_if_current_buffer_is_different_from_last(client, current_buffer_view);
        }

        let current_buffer_handle = current_buffer_view.map(|v| v.buffer_handle);

        for (i, snapshot) in client.navigation_history.snapshots.iter().enumerate().rev() {
            if current_buffer_handle != Some(snapshot.buffer_handle) {
                let buffer_view_handle = editor
                    .buffer_views
                    .buffer_view_handle_from_buffer_handle(client.handle(), snapshot.buffer_handle);
                client.set_buffer_view_handle_no_history(
                    Some(buffer_view_handle),
                    &mut editor.events,
                );
                client.navigation_history.current_snapshot_index = i as _;
                client.navigation_history.on_previous_buffer = true;
                break;
            }
        }
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

        NavigationHistory::save_snapshot(&mut client, &editor.buffer_views);
        client.set_buffer_view_handle_no_history(Some(view_a), &mut editor.events);
        NavigationHistory::save_snapshot(&mut client, &editor.buffer_views);
        client.set_buffer_view_handle_no_history(Some(view_b), &mut editor.events);
        NavigationHistory::save_snapshot(&mut client, &editor.buffer_views);
        client.set_buffer_view_handle_no_history(Some(view_c), &mut editor.events);

        (editor, client)
    }

    fn buffer_index(client: &Client, editor: &Editor) -> usize {
        let buffer_view_handle = client.buffer_view_handle().unwrap();
        let buffer_view = editor.buffer_views.get(buffer_view_handle);
        buffer_view.buffer_handle.0 as _
    }

    #[test]
    fn move_back_and_forward_in_history() {
        let (mut editor, mut client) = setup();

        assert_eq!(2, client.navigation_history.current_snapshot_index);
        assert_eq!(2, buffer_index(&client, &editor));

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Forward);
        assert_eq!(2, client.navigation_history.current_snapshot_index);
        assert_eq!(2, buffer_index(&client, &editor));

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_index(&client, &editor));

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(0, client.navigation_history.current_snapshot_index);
        assert_eq!(0, buffer_index(&client, &editor));

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(0, client.navigation_history.current_snapshot_index);
        assert_eq!(0, buffer_index(&client, &editor));

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Forward);
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_index(&client, &editor));

        assert_eq!(3, client.navigation_history.snapshots.len());
    }

    #[test]
    fn move_to_previous_buffer_three_times() {
        let (mut editor, mut client) = setup();

        assert_eq!(2, buffer_index(&client, &editor));

        NavigationHistory::move_to_previous_buffer(&mut client, &mut editor);
        assert_eq!(1, buffer_index(&client, &editor));

        NavigationHistory::move_to_previous_buffer(&mut client, &mut editor);
        assert_eq!(2, buffer_index(&client, &editor));

        NavigationHistory::move_to_previous_buffer(&mut client, &mut editor);
        assert_eq!(1, buffer_index(&client, &editor));
    }

    #[test]
    fn navigate_back_then_move_to_previous_buffer() {
        let (mut editor, mut client) = setup();

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_index(&client, &editor));

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Backward);
        assert_eq!(0, client.navigation_history.current_snapshot_index);
        assert_eq!(0, buffer_index(&client, &editor));

        NavigationHistory::move_in_history(&mut client, &mut editor, NavigationMovement::Forward);
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_index(&client, &editor));

        assert_eq!(3, client.navigation_history.snapshots.len());

        NavigationHistory::move_to_previous_buffer(&mut client, &mut editor);
        assert_eq!(2, client.navigation_history.current_snapshot_index);
        assert_eq!(2, buffer_index(&client, &editor));

        assert_eq!(3, client.navigation_history.snapshots.len());

        NavigationHistory::move_to_previous_buffer(&mut client, &mut editor);
        assert_eq!(1, client.navigation_history.current_snapshot_index);
        assert_eq!(1, buffer_index(&client, &editor));

        NavigationHistory::move_to_previous_buffer(&mut client, &mut editor);
        assert_eq!(2, client.navigation_history.current_snapshot_index);
        assert_eq!(2, buffer_index(&client, &editor));

        assert_eq!(3, client.navigation_history.snapshots.len());
    }
}

