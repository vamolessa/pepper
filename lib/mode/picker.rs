use crate::platform::Key;

use crate::{
    buffer_view::BufferViewError,
    client::{ClientManager, ClientHandle},
    editor::{Editor, KeysIterator, ReadLinePoll, EditorOutputTarget},
    mode::{Mode, ModeKind, ModeOperation, ModeState},
    word_database::EmptyWordCollection,
};

pub struct State {
    on_client_keys:
        fn(&mut Editor, &mut ClientManager, ClientHandle, &mut KeysIterator, ReadLinePoll),
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _, _, _| (),
        }
    }
}

impl ModeState for State {
    fn on_enter(editor: &mut Editor, _: &mut ClientManager, _: ClientHandle) {
        editor.read_line.set_input("");
        editor.picker.filter(&EmptyWordCollection, "");
    }

    fn on_exit(editor: &mut Editor, _: &mut ClientManager, _: ClientHandle) {
        editor.read_line.set_input("");
        editor.picker.reset();
    }

    fn on_client_keys(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<ModeOperation> {
        let this = &mut editor.mode.picker_state;
        let poll = editor.read_line.poll(&editor.buffered_keys, keys);
        if let ReadLinePoll::Pending = poll {
            keys.put_back();
            match keys.next(&editor.buffered_keys) {
                Key::Ctrl('n') | Key::Ctrl('j') | Key::Down => editor.picker.move_cursor(1),
                Key::Ctrl('p') | Key::Ctrl('k') | Key::Up => editor.picker.move_cursor(-1),
                Key::Ctrl('d') | Key::PageDown => {
                    let picker_height = editor
                        .picker
                        .height(editor.config.picker_max_height.get() as _)
                        as isize;
                    editor.picker.move_cursor(picker_height / 2);
                }
                Key::Ctrl('u') | Key::PageUp => {
                    let picker_height = editor
                        .picker
                        .height(editor.config.picker_max_height.get() as _)
                        as isize;
                    editor.picker.move_cursor(-picker_height / 2);
                }
                Key::Ctrl('b') | Key::Home => {
                    let cursor = editor.picker.cursor() as isize;
                    editor.picker.move_cursor(-cursor);
                }
                Key::Ctrl('e') | Key::End => {
                    let cursor = editor.picker.cursor() as isize;
                    let entry_count = editor.picker.len() as isize;
                    editor.picker.move_cursor(entry_count - cursor - 1);
                }
                _ => editor
                    .picker
                    .filter(&EmptyWordCollection, editor.read_line.input()),
            }
        }

        (this.on_client_keys)(editor, clients, client_handle, keys, poll);
        None
    }
}

pub mod buffer {
    use super::*;

    use std::path::Path;

    use crate::{buffer::Buffer, navigation_history::NavigationHistory, picker::Picker};

    pub fn enter_mode(
        editor: &mut Editor,
        clients: &mut ClientManager,
        client_handle: ClientHandle,
    ) {
        fn on_client_keys(
            editor: &mut Editor,
            clients: &mut ClientManager,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) {
            match poll {
                ReadLinePoll::Pending => return,
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => {
                    Mode::change_to(editor, clients, client_handle, ModeKind::default());
                    return;
                }
            }

            let path = match editor.picker.current_entry(&EmptyWordCollection) {
                Some(entry) => entry.name,
                None => {
                    Mode::change_to(editor, clients, client_handle, ModeKind::default());
                    return;
                }
            };

            NavigationHistory::save_client_snapshot(clients, client_handle, &editor.buffer_views);

            match editor.buffer_views.buffer_view_handle_from_path(
                client_handle,
                &mut editor.buffers,
                &mut editor.word_database,
                &editor.current_directory,
                Path::new(path),
                None,
                &mut editor.events,
            ) {
                Ok(handle) => {
                    if let Some(client) = clients.get_mut(client_handle) {
                        client.set_buffer_view_handle(editor, Some(handle));
                    }
                }
                Err(BufferViewError::InvalidPath) => editor
                    .output
                    .write(EditorOutputTarget::Error)
                    .fmt(format_args!("invalid path '{}'", path)),
            }

            Mode::change_to(editor, clients, client_handle, ModeKind::default());
        }

        fn add_buffer_to_picker(picker: &mut Picker, buffer: &Buffer) {
            if let Some(path) = buffer.path().and_then(|p| p.to_str()) {
                picker.add_custom_entry(path, if buffer.needs_save() { "changed" } else { "" });
            }
        }

        editor.read_line.set_prompt("buffer:");
        editor.picker.reset();

        let buffers = &editor.buffers;
        let buffer_views = &editor.buffer_views;
        let prevous_buffer_handle = clients
            .get(client_handle)
            .and_then(|c| c.previous_buffer_view_handle())
            .and_then(|h| buffer_views.get(h))
            .map(|v| v.buffer_handle);

        if let Some(buffer) = prevous_buffer_handle.and_then(|h| buffers.get(h)) {
            add_buffer_to_picker(&mut editor.picker, buffer);
        }

        for buffer in editor.buffers.iter() {
            let buffer_handle = buffer.handle();
            if prevous_buffer_handle
                .map(|h| h != buffer_handle)
                .unwrap_or(true)
            {
                add_buffer_to_picker(&mut editor.picker, buffer);
            }
        }

        editor.mode.picker_state.on_client_keys = on_client_keys;
        Mode::change_to(editor, clients, client_handle, ModeKind::Picker);
    }
}
