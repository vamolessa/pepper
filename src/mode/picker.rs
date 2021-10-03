use std::{path::Path, process::Stdio};

use crate::{
    buffer::{parse_path_and_position, BufferCapabilities},
    buffer_position::BufferPosition,
    cursor::Cursor,
    editor::{EditorControlFlow, KeysIterator},
    editor_utils::{parse_process_command, MessageKind, ReadLine, ReadLinePoll},
    lsp,
    mode::{Mode, ModeContext, ModeKind, ModeState},
    picker::{EntrySource, Picker},
    platform::{Key, PlatformRequest, ProcessTag},
    word_database::WordIndicesIter,
};

pub struct State {
    on_client_keys:
        fn(ctx: &mut ModeContext, &mut KeysIterator, ReadLinePoll) -> Option<EditorControlFlow>,
    find_file_waiting_for_process: bool,
    find_file_buf: Vec<u8>,
    lsp_client_handle: Option<lsp::ClientHandle>,
}

impl State {
    pub fn on_process_output(&mut self, picker: &mut Picker, read_line: &ReadLine, bytes: &[u8]) {
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

    pub fn on_process_exit(&mut self, picker: &mut Picker, read_line: &ReadLine) {
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
            on_client_keys: |_, _, _| Some(EditorControlFlow::Continue),
            find_file_waiting_for_process: false,
            find_file_buf: Vec::new(),
            lsp_client_handle: None,
        }
    }
}

impl ModeState for State {
    fn on_enter(ctx: &mut ModeContext) {
        ctx.editor.read_line.input_mut().clear();
    }

    fn on_exit(ctx: &mut ModeContext) {
        ctx.editor.mode.picker_state.find_file_waiting_for_process = false;
        ctx.editor.read_line.input_mut().clear();
        ctx.editor.picker.clear();
    }

    fn on_client_keys(ctx: &mut ModeContext, keys: &mut KeysIterator) -> Option<EditorControlFlow> {
        let this = &mut ctx.editor.mode.picker_state;
        let poll = ctx.editor.read_line.poll(
            ctx.platform,
            &mut ctx.editor.string_pool,
            &ctx.editor.buffered_keys,
            keys,
        );
        if let ReadLinePoll::Pending = poll {
            keys.index = keys.index.saturating_sub(1);
            match keys.next(&ctx.editor.buffered_keys) {
                Key::Ctrl('n') | Key::Ctrl('j') | Key::Down => ctx.editor.picker.move_cursor(1),
                Key::Ctrl('p') | Key::Ctrl('k') | Key::Up => ctx.editor.picker.move_cursor(-1),
                Key::Ctrl('d') | Key::PageDown => {
                    let picker_height = ctx
                        .editor
                        .picker
                        .len()
                        .min(ctx.editor.config.picker_max_height as _)
                        as isize;
                    ctx.editor.picker.move_cursor(picker_height / 2);
                }
                Key::Ctrl('u') | Key::PageUp => {
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
        f(ctx, keys, poll)
    }
}

pub mod opened_buffers {
    use super::*;

    use std::path::Path;

    pub fn enter_mode(ctx: &mut ModeContext) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => return Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => {
                    Mode::change_to(ctx, ModeKind::default());
                    return Some(EditorControlFlow::Continue);
                }
            }

            let path = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
                Some((_, entry)) => entry,
                _ => {
                    Mode::change_to(ctx, ModeKind::default());
                    return Some(EditorControlFlow::Continue);
                }
            };

            let path = ctx.editor.string_pool.acquire_with(path);
            if let Ok(buffer_view_handle) = ctx.editor.buffer_view_handle_from_path(
                ctx.client_handle,
                Path::new(&path),
                BufferCapabilities::text(),
            ) {
                let client = ctx.clients.get_mut(ctx.client_handle);
                client.set_buffer_view_handle(
                    Some(buffer_view_handle),
                    &ctx.editor.buffer_views,
                    &mut ctx.editor.events,
                );
            }
            ctx.editor.string_pool.release(path);

            Mode::change_to(ctx, ModeKind::default());
            Some(EditorControlFlow::Continue)
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
            Mode::change_to(ctx, ModeKind::Picker);
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

    pub fn enter_mode(ctx: &mut ModeContext, command: &str) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => return Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => (),
                ReadLinePoll::Canceled => {
                    Mode::change_to(ctx, ModeKind::default());
                    return Some(EditorControlFlow::Continue);
                }
            }

            let path = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
                Some((_, entry)) => entry,
                _ => {
                    Mode::change_to(ctx, ModeKind::default());
                    return Some(EditorControlFlow::Continue);
                }
            };

            let path = ctx.editor.string_pool.acquire_with(path);
            match ctx.editor.buffer_view_handle_from_path(
                ctx.client_handle,
                Path::new(&path),
                BufferCapabilities::text(),
            ) {
                Ok(buffer_view_handle) => {
                    let client = ctx.clients.get_mut(ctx.client_handle);
                    client.set_buffer_view_handle(
                        Some(buffer_view_handle),
                        &ctx.editor.buffer_views,
                        &mut ctx.editor.events,
                    );
                }
                Err(error) => ctx
                    .editor
                    .status_bar
                    .write(MessageKind::Error)
                    .fmt(format_args!("{}", error)),
            }
            ctx.editor.string_pool.release(path);

            Mode::change_to(ctx, ModeKind::default());
            Some(EditorControlFlow::Continue)
        }

        ctx.editor.read_line.set_prompt("open:");
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
        Mode::change_to(ctx, ModeKind::Picker);
    }
}

pub mod lsp_definition {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext, client_handle: lsp::ClientHandle) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => {
                    if let Some((_, entry)) =
                        ctx.editor.picker.current_entry(&ctx.editor.word_database)
                    {
                        let (path, position) = parse_path_and_position(entry);
                        let position = match position {
                            Some(position) => position,
                            None => BufferPosition::zero(),
                        };

                        let path = ctx.editor.string_pool.acquire_with(path);
                        match ctx.editor.buffer_view_handle_from_path(
                            ctx.client_handle,
                            Path::new(&path),
                            BufferCapabilities::text(),
                        ) {
                            Ok(buffer_view_handle) => {
                                let client = ctx.clients.get_mut(ctx.client_handle);
                                client.set_buffer_view_handle(
                                    Some(buffer_view_handle),
                                    &ctx.editor.buffer_views,
                                    &mut ctx.editor.events,
                                );

                                let mut cursors = ctx
                                    .editor
                                    .buffer_views
                                    .get_mut(buffer_view_handle)
                                    .cursors
                                    .mut_guard();
                                cursors.clear();
                                cursors.add(Cursor {
                                    anchor: position,
                                    position,
                                });
                            }
                            Err(error) => ctx
                                .editor
                                .status_bar
                                .write(MessageKind::Error)
                                .fmt(format_args!("{}", error)),
                        }
                        ctx.editor.string_pool.release(path);
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
                ReadLinePoll::Canceled => {
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
            }
        }

        ctx.editor.read_line.set_prompt("definition:");
        ctx.editor.picker.filter(WordIndicesIter::empty(), "");
        ctx.editor.picker.move_cursor(0);

        if ctx.editor.picker.len() > 0 {
            let state = &mut ctx.editor.mode.picker_state;
            state.on_client_keys = on_client_keys;
            state.lsp_client_handle = Some(client_handle);
            Mode::change_to(ctx, ModeKind::Picker);
        }
    }
}

pub mod lsp_code_action {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext, client_handle: lsp::ClientHandle) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => {
                    if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                        let index = match ctx.editor.picker.current_entry(&ctx.editor.word_database)
                        {
                            Some((EntrySource::Custom(i), _)) => i,
                            _ => 0,
                        };
                        lsp::ClientManager::access(ctx.editor, handle, |e, c| {
                            c.finish_code_action(e, index);
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
                ReadLinePoll::Canceled => {
                    if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                        lsp::ClientManager::access(ctx.editor, handle, |_, c| {
                            c.cancel_current_request();
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
            }
        }

        ctx.editor.read_line.set_prompt("code action:");
        ctx.editor.picker.filter(WordIndicesIter::empty(), "");
        ctx.editor.picker.move_cursor(0);

        if ctx.editor.picker.len() > 0 {
            let state = &mut ctx.editor.mode.picker_state;
            state.on_client_keys = on_client_keys;
            state.lsp_client_handle = Some(client_handle);
            Mode::change_to(ctx, ModeKind::Picker);
        } else {
            lsp::ClientManager::access(ctx.editor, client_handle, |_, c| {
                c.cancel_current_request();
            });
        }
    }
}

pub mod lsp_document_symbol {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext, client_handle: lsp::ClientHandle) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => {
                    if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                        let index = match ctx.editor.picker.current_entry(&ctx.editor.word_database)
                        {
                            Some((EntrySource::Custom(i), _)) => i,
                            _ => 0,
                        };
                        let clients = &mut *ctx.clients;
                        let client_handle = ctx.client_handle;
                        lsp::ClientManager::access(ctx.editor, handle, |e, c| {
                            c.finish_document_symbols(e, clients, client_handle, index);
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
                ReadLinePoll::Canceled => {
                    if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                        lsp::ClientManager::access(ctx.editor, handle, |_, c| {
                            c.cancel_current_request();
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
            }
        }

        ctx.editor.read_line.set_prompt("document symbol:");
        ctx.editor.picker.filter(WordIndicesIter::empty(), "");
        ctx.editor.picker.move_cursor(0);

        if ctx.editor.picker.len() > 0 {
            let state = &mut ctx.editor.mode.picker_state;
            state.on_client_keys = on_client_keys;
            state.lsp_client_handle = Some(client_handle);
            Mode::change_to(ctx, ModeKind::Picker);
        } else {
            lsp::ClientManager::access(ctx.editor, client_handle, |_, c| {
                c.cancel_current_request();
            });
        }
    }
}

pub mod lsp_workspace_symbol {
    use super::*;

    pub fn enter_mode(ctx: &mut ModeContext, client_handle: lsp::ClientHandle) {
        fn on_client_keys(
            ctx: &mut ModeContext,
            _: &mut KeysIterator,
            poll: ReadLinePoll,
        ) -> Option<EditorControlFlow> {
            match poll {
                ReadLinePoll::Pending => Some(EditorControlFlow::Continue),
                ReadLinePoll::Submitted => {
                    if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                        let index = match ctx.editor.picker.current_entry(&ctx.editor.word_database)
                        {
                            Some((EntrySource::Custom(i), _)) => i,
                            _ => 0,
                        };
                        let clients = &mut *ctx.clients;
                        let client_handle = ctx.client_handle;
                        lsp::ClientManager::access(ctx.editor, handle, |e, c| {
                            c.finish_workspace_symbols(e, clients, client_handle, index);
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
                ReadLinePoll::Canceled => {
                    if let Some(handle) = ctx.editor.mode.picker_state.lsp_client_handle {
                        lsp::ClientManager::access(ctx.editor, handle, |_, c| {
                            c.cancel_current_request();
                        });
                    }
                    Mode::change_to(ctx, ModeKind::default());
                    Some(EditorControlFlow::Continue)
                }
            }
        }

        ctx.editor.read_line.set_prompt("workspace symbol:");
        ctx.editor.picker.filter(WordIndicesIter::empty(), "");
        ctx.editor.picker.move_cursor(0);

        if ctx.editor.picker.len() > 0 {
            let state = &mut ctx.editor.mode.picker_state;
            state.on_client_keys = on_client_keys;
            state.lsp_client_handle = Some(client_handle);
            Mode::change_to(ctx, ModeKind::Picker);
        } else {
            lsp::ClientManager::access(ctx.editor, client_handle, |_, c| {
                c.cancel_current_request();
            });
        }
    }
}
