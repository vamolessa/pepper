use std::process::Stdio;

use crate::{
    buffer::BufferProperties,
    client::ClientHandle,
    editor::{Editor, EditorContext, EditorFlow, KeysIterator},
    editor_utils::{parse_process_command, MessageKind, ReadLine, ReadLinePoll},
    mode::{ModeKind, ModeState},
    picker::Picker,
    platform::{Key, PlatformRequest, ProcessTag},
    word_database::WordIndicesIter,
};

pub struct State {
    pub on_client_keys: fn(
        ctx: &mut EditorContext,
        ClientHandle,
        &mut KeysIterator,
        ReadLinePoll,
    ) -> Option<EditorFlow>,
    find_file_waiting_for_process: bool,
    find_file_buf: Vec<u8>,
}

impl State {
    pub(crate) fn on_process_output(
        &mut self,
        picker: &mut Picker,
        read_line: &ReadLine,
        bytes: &[u8],
    ) {
        if !self.find_file_waiting_for_process {
            return;
        }

        self.find_file_buf.extend_from_slice(bytes);

        {
            let mut filtered_entry_adder = picker.add_custom_filtered_entries(read_line.input());
            if let Some(i) = self.find_file_buf.iter().rposition(|&b| b == b'\n') {
                for line in self
                    .find_file_buf
                    .drain(..i + 1)
                    .as_slice()
                    .split(|&b| matches!(b, b'\n' | b'\r'))
                {
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(line) = std::str::from_utf8(line) {
                        filtered_entry_adder.add(line);
                    }
                }
            }
        }

        picker.move_cursor(0);
    }

    pub(crate) fn on_process_exit(&mut self, picker: &mut Picker, read_line: &ReadLine) {
        if !self.find_file_waiting_for_process {
            return;
        }

        self.find_file_waiting_for_process = false;

        {
            let mut filtered_entry_adder = picker.add_custom_filtered_entries(read_line.input());
            for line in self.find_file_buf.split(|&b| b == b'\n') {
                if line.is_empty() {
                    continue;
                }
                if let Ok(line) = std::str::from_utf8(line) {
                    filtered_entry_adder.add(line);
                }
            }
        }

        self.find_file_buf.clear();
        picker.move_cursor(0);
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            on_client_keys: |_, _, _, _| Some(EditorFlow::Continue),
            find_file_waiting_for_process: false,
            find_file_buf: Vec::new(),
        }
    }
}

impl ModeState for State {
    fn on_enter(editor: &mut Editor) {
        editor.read_line.input_mut().clear();
    }

    fn on_exit(editor: &mut Editor) {
        editor.mode.plugin_handle = None;
        editor.mode.picker_state.find_file_waiting_for_process = false;
        editor.read_line.input_mut().clear();
        editor.picker.clear();
    }

    fn on_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<EditorFlow> {
        let this = &mut ctx.editor.mode.picker_state;
        let poll = ctx.editor.read_line.poll(
            &mut ctx.platform,
            &mut ctx.editor.string_pool,
            &ctx.editor.buffered_keys,
            keys,
        );
        if let ReadLinePoll::Pending = poll {
            keys.index = keys.index.saturating_sub(1);
            match keys.next(&ctx.editor.buffered_keys) {
                Key::Ctrl('n') | Key::Down => ctx.editor.picker.move_cursor(1),
                Key::Ctrl('p') | Key::Up => ctx.editor.picker.move_cursor(-1),
                Key::Ctrl('j') | Key::PageDown => {
                    let picker_height = ctx
                        .editor
                        .picker
                        .len()
                        .min(ctx.editor.config.picker_max_height as _)
                        as isize;
                    ctx.editor.picker.move_cursor(picker_height / 2);
                }
                Key::Ctrl('k') | Key::PageUp => {
                    let picker_height = ctx
                        .editor
                        .picker
                        .len()
                        .min(ctx.editor.config.picker_max_height as _)
                        as isize;
                    ctx.editor.picker.move_cursor(-picker_height / 2);
                }
                Key::Ctrl('b') | Key::Home => {
                    let cursor = ctx.editor.picker.cursor().unwrap_or(0) as isize;
                    ctx.editor.picker.move_cursor(-cursor);
                }
                Key::Ctrl('e') | Key::End => {
                    let cursor = ctx.editor.picker.cursor().unwrap_or(0) as isize;
                    let entry_count = ctx.editor.picker.len() as isize;
                    ctx.editor.picker.move_cursor(entry_count - cursor - 1);
                }
                _ => {
                    ctx.editor
                        .picker
                        .filter(WordIndicesIter::empty(), ctx.editor.read_line.input());
                    ctx.editor.picker.move_cursor(0);
                }
            }
        }

        let f = this.on_client_keys;
        f(ctx, client_handle, keys, poll)
    }
}

pub mod opened_buffers {
    use super::*;

    use std::path::Path;

    pub fn enter_mode(ctx: &mut EditorContext) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorFlow> {
            match poll {
                ReadLinePoll::Pending => return Some(EditorFlow::Continue),
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => {
                    ctx.editor.enter_mode(ModeKind::default());
                    return Some(EditorFlow::Continue);
                }
            }

            let path = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
                Some((_, entry)) => entry,
                _ => {
                    ctx.editor.enter_mode(ModeKind::default());
                    return Some(EditorFlow::Continue);
                }
            };

            let path = ctx.editor.string_pool.acquire_with(path);
            if let Ok(buffer_view_handle) = ctx.editor.buffer_view_handle_from_path(
                client_handle,
                Path::new(&path),
                BufferProperties::text(),
                false,
            ) {
                let client = ctx.clients.get_mut(client_handle);
                client.set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);
            }
            ctx.editor.string_pool.release(path);

            ctx.editor.enter_mode(ModeKind::default());
            Some(EditorFlow::Continue)
        }

        ctx.editor.read_line.set_prompt("buffer:");
        ctx.editor.picker.clear();

        for path in ctx.editor.buffers.iter().filter_map(|b| b.path.to_str()) {
            ctx.editor.picker.add_custom_entry(path);
        }

        ctx.editor.picker.filter(WordIndicesIter::empty(), "");
        ctx.editor.picker.move_cursor(0);

        if ctx.editor.picker.len() > 0 {
            ctx.editor.mode.picker_state.on_client_keys = on_client_keys;
            ctx.editor.enter_mode(ModeKind::Picker);
        } else {
            ctx.editor
                .status_bar
                .write(MessageKind::Error)
                .str("no buffer opened");
        }
    }
}

pub mod find_file {
    use super::*;

    use std::path::Path;

    pub fn enter_mode(ctx: &mut EditorContext, command: &str, prompt: &str) {
        fn on_client_keys(
            ctx: &mut EditorContext,
            client_handle: ClientHandle,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorFlow> {
            match poll {
                ReadLinePoll::Pending => return Some(EditorFlow::Continue),
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => {
                    ctx.editor.enter_mode(ModeKind::default());
                    return Some(EditorFlow::Continue);
                }
            }

            let path = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
                Some((_, entry)) => entry,
                _ => {
                    ctx.editor.enter_mode(ModeKind::default());
                    return Some(EditorFlow::Continue);
                }
            };

            let path = ctx.editor.string_pool.acquire_with(path);
            match ctx.editor.buffer_view_handle_from_path(
                client_handle,
                Path::new(&path),
                BufferProperties::text(),
                false,
            ) {
                Ok(buffer_view_handle) => {
                    let client = ctx.clients.get_mut(client_handle);
                    client
                        .set_buffer_view_handle(Some(buffer_view_handle), &ctx.editor.buffer_views);
                }
                Err(error) => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error)),
            }
            ctx.editor.string_pool.release(path);

            ctx.editor.enter_mode(ModeKind::default());
            Some(EditorFlow::Continue)
        }

        ctx.editor.read_line.set_prompt(prompt);
        ctx.editor.picker.clear();

        let command = match parse_process_command(command) {
            Some(mut command) => {
                command.stdin(Stdio::null());
                command.stdout(Stdio::piped());
                command.stderr(Stdio::null());

                command
            }
            None => {
                ctx.editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("invalid find file command '{}'", command));
                return;
            }
        };

        ctx.editor.mode.picker_state.find_file_waiting_for_process = true;
        ctx.platform
            .requests
            .enqueue(PlatformRequest::SpawnProcess {
                tag: ProcessTag::FindFiles,
                command,
                buf_len: 4 * 1024,
            });

        ctx.editor.mode.picker_state.on_client_keys = on_client_keys;
        ctx.editor.enter_mode(ModeKind::Picker);
    }
}
