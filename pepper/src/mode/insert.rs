use std::{fmt::Write, path::Path};

use crate::{
    buffer::{Buffer, BufferHandle},
    buffer_position::BufferPosition,
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    client::ClientHandle,
    editor::{Editor, EditorContext, EditorControlFlow, KeysIterator},
    mode::{Mode, ModeKind, ModeState},
    platform::Key,
    plugin::{CompletionContext, PluginCollection, PluginContext, PluginHandle},
    register::AUTO_MACRO_REGISTER,
    word_database::{WordIndicesIter, WordKind},
};

struct CompletionState {
    pub buffer_handle: BufferHandle,
    pub plugin_handle: PluginHandle,
}

#[derive(Default)]
pub struct State {
    completion_positions: Vec<BufferPosition>,
    completion_state: Option<CompletionState>,
}

// TODO: make completions work again
impl State {
    /*
    fn get_completion_plugin_handle(
        &mut self,
        plugins: &PluginCollection,
        buffer: &Buffer,
    ) -> Option<PluginHandle> {
        match &self.completion_state {
            Some(state) => {
            //
            }
        }
        //if buffer.handle()

        if let Some(plugin) = self.completion_plugin_handle.map(|h| plugins.get(h)) {
            //
        }
    }
    */

    /*
    fn get_lsp_client_handle(
        &mut self,
        lsp_clients: &lsp::ClientManager,
        buffer_path: &Path,
    ) -> Option<lsp::ClientHandle> {
        if self
            .lsp_client_handle
            .and_then(|h| lsp_clients.get(h))
            .is_some()
        {
            return self.lsp_client_handle;
        }

        let buffer_path = buffer_path.to_str()?;
        let handle = lsp_clients
            .clients()
            .find(|c| c.handles_path(buffer_path))
            .map(|c| c.handle());
        self.lsp_client_handle = handle;
        handle
    }
    */
}

impl ModeState for State {
    fn on_enter(ctx: &mut EditorContext) {
        cancel_completion(&mut ctx.editor);
    }

    fn on_exit(ctx: &mut EditorContext) {
        cancel_completion(&mut ctx.editor);
    }

    fn on_client_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<EditorControlFlow> {
        let handle = match ctx.clients.get(client_handle).buffer_view_handle() {
            Some(handle) => handle,
            None => {
                Mode::change_to(ctx, ModeKind::default());
                return Some(EditorControlFlow::Continue);
            }
        };

        let key = keys.next(&ctx.editor.buffered_keys);
        let register = ctx.editor.registers.get_mut(AUTO_MACRO_REGISTER);
        let _ = write!(register, "{}", key);

        match key {
            Key::Esc | Key::Ctrl('c') => {
                let buffer_view = ctx.editor.buffer_views.get(handle);
                ctx.editor
                    .buffers
                    .get_mut(buffer_view.buffer_handle)
                    .commit_edits();
                Mode::change_to(ctx, ModeKind::default());
                return Some(EditorControlFlow::Continue);
            }
            Key::Left => {
                ctx.editor.buffer_views.get_mut(handle).move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::ColumnsBackward(1),
                    CursorMovementKind::PositionAndAnchor,
                    ctx.editor.config.tab_size,
                );
                cancel_completion(&mut ctx.editor);
                return Some(EditorControlFlow::Continue);
            }
            Key::Down => {
                ctx.editor.buffer_views.get_mut(handle).move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::LinesForward(1),
                    CursorMovementKind::PositionAndAnchor,
                    ctx.editor.config.tab_size,
                );
                cancel_completion(&mut ctx.editor);
                return Some(EditorControlFlow::Continue);
            }
            Key::Up => {
                ctx.editor.buffer_views.get_mut(handle).move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::LinesBackward(1),
                    CursorMovementKind::PositionAndAnchor,
                    ctx.editor.config.tab_size,
                );
                cancel_completion(&mut ctx.editor);
                return Some(EditorControlFlow::Continue);
            }
            Key::Right => {
                ctx.editor.buffer_views.get_mut(handle).move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::ColumnsForward(1),
                    CursorMovementKind::PositionAndAnchor,
                    ctx.editor.config.tab_size,
                );
                cancel_completion(&mut ctx.editor);
                return Some(EditorControlFlow::Continue);
            }
            Key::Tab => {
                static SPACES_BUF: &[u8; u8::MAX as usize] = &[b' '; u8::MAX as usize];
                let text = if ctx.editor.config.indent_with_tabs {
                    "\t"
                } else {
                    let len = ctx.editor.config.tab_size.get() as usize;
                    unsafe { std::str::from_utf8_unchecked(&SPACES_BUF[..len]) }
                };

                ctx.editor
                    .buffer_views
                    .get(handle)
                    .insert_text_at_cursor_positions(
                        &mut ctx.editor.buffers,
                        &mut ctx.editor.word_database,
                        text,
                        &mut ctx.editor.events,
                    );
            }
            Key::Enter | Key::Ctrl('m') => {
                let buffer_view = ctx.editor.buffer_views.get(handle);
                let cursor_count = buffer_view.cursors[..].len();
                let buffer = ctx.editor.buffers.get_mut(buffer_view.buffer_handle);

                let mut buf = ctx.editor.string_pool.acquire();
                for i in (0..cursor_count).rev() {
                    let position = buffer_view.cursors[i].position;

                    buf.push('\n');
                    let indentation_word = buffer
                        .content()
                        .word_at(BufferPosition::line_col(position.line_index, 0));
                    if indentation_word.kind == WordKind::Whitespace {
                        let indentation_len = position
                            .column_byte_index
                            .min(indentation_word.text.len() as _);
                        buf.push_str(&indentation_word.text[..indentation_len as usize]);
                    }

                    buffer.insert_text(
                        &mut ctx.editor.word_database,
                        position,
                        &buf,
                        &mut ctx.editor.events,
                    );
                    buf.clear();
                }
                ctx.editor.string_pool.release(buf);
            }
            Key::Char(c) => {
                let mut buf = [0; std::mem::size_of::<char>()];
                let s = c.encode_utf8(&mut buf);
                let buffer_view = ctx.editor.buffer_views.get(handle);
                buffer_view.insert_text_at_cursor_positions(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    s,
                    &mut ctx.editor.events,
                );
            }
            Key::Backspace | Key::Ctrl('h') => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle);
                buffer_view.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::ColumnsBackward(1),
                    CursorMovementKind::PositionOnly,
                    ctx.editor.config.tab_size,
                );
                buffer_view.delete_text_in_cursor_ranges(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );
            }
            Key::Delete => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle);
                buffer_view.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::ColumnsForward(1),
                    CursorMovementKind::PositionOnly,
                    ctx.editor.config.tab_size,
                );
                buffer_view.delete_text_in_cursor_ranges(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );
            }
            Key::Ctrl('w') => {
                let buffer_view = ctx.editor.buffer_views.get_mut(handle);
                buffer_view.move_cursors(
                    &ctx.editor.buffers,
                    CursorMovement::WordsBackward(1),
                    CursorMovementKind::PositionOnly,
                    ctx.editor.config.tab_size,
                );
                buffer_view.delete_text_in_cursor_ranges(
                    &mut ctx.editor.buffers,
                    &mut ctx.editor.word_database,
                    &mut ctx.editor.events,
                );
            }
            Key::Ctrl('n') => {
                apply_completion(ctx, handle, 1);
                return Some(EditorControlFlow::Continue);
            }
            Key::Ctrl('p') => {
                apply_completion(ctx, handle, -1);
                return Some(EditorControlFlow::Continue);
            }
            _ => return Some(EditorControlFlow::Continue),
        };

        ctx.trigger_event_handlers();
        update_completions(ctx, handle);
        Some(EditorControlFlow::Continue)
    }
}

fn cancel_completion(editor: &mut Editor) {
    editor.picker.clear();
    editor.mode.insert_state.completion_positions.clear();
}

fn update_completions(ctx: &mut EditorContext, buffer_view_handle: BufferViewHandle) {
    let state = &mut ctx.editor.mode.insert_state;

    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);
    let content = buffer.content();

    let main_cursor_position = buffer_view.cursors.main_cursor().position;
    let word = content.word_at(content.position_before(main_cursor_position));

    let completion_ctx = CompletionContext {
        buffer_handle: buffer.handle(),
        buffer_view_handle,
        position: main_cursor_position,
        last_char: '\0',
    };

    /*
    for plugin in ctx.editor.plugins.iter_mut() {
        //match plugin.on_completion_flow(
        //
    }
    */

    /*
    if word.position > main_completion_position {
        cancel_completion(ctx.editor);
        return;
    }
    */

    /*
    let state = &mut ctx.editor.mode.insert_state;
    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
    let buffer = ctx.editor.buffers.get(buffer_view.buffer_handle);
    let content = buffer.content();

    let main_cursor_position = buffer_view.cursors.main_cursor().position;
    let word = content.word_at(content.position_before(main_cursor_position));

    let lsp_client_handle = state.get_lsp_client_handle(&ctx.editor.lsp, &buffer.path);

    let mut force_trigger_completion = false;
    if let Some(last_char) = word.text.chars().next_back() {
        let lsp = &ctx.editor.lsp;
        if let Some(client) = lsp_client_handle.and_then(|h| lsp.get(h)) {
            if client.signature_help_triggers().contains(last_char) {
                let lsp_client_handle = client.handle();
                let platform = &mut *ctx.platform;
                let buffer_handle = buffer.handle();
                lsp::ClientManager::access(ctx.editor, lsp_client_handle, |e, c| {
                    c.signature_help(e, platform, buffer_handle, main_cursor_position)
                });
                return;
            }

            if client.completion_triggers().contains(last_char) {
                force_trigger_completion = true;
                state.completion_positions.clear();
            }
        }
    }

    let main_cursor_index = buffer_view.cursors.main_cursor_index();
    let main_completion_position = match state.completion_positions.get(main_cursor_index) {
        Some(&position) => {
            if main_cursor_position < position {
                cancel_completion(ctx.editor);
                return;
            }

            position
        }
        None => {
            if !force_trigger_completion
                && (word.kind != WordKind::Identifier
                    || word.text.len() < ctx.editor.config.completion_min_len as _)
            {
                cancel_completion(ctx.editor);
                return;
            }

            state.completion_positions.clear();
            for cursor in &buffer_view.cursors[..] {
                let word = content.word_at(content.position_before(cursor.position));
                let position = match word.kind {
                    WordKind::Identifier => word.position,
                    _ => cursor.position,
                };
                state.completion_positions.push(position);
            }

            if let Some(lsp_client_handle) = lsp_client_handle {
                ctx.editor.picker.clear();

                let platform = &mut *ctx.platform;
                let client_handle = ctx.client_handle;
                let buffer_handle = buffer.handle();
                lsp::ClientManager::access(ctx.editor, lsp_client_handle, |e, c| {
                    c.completion(
                        e,
                        platform,
                        client_handle,
                        buffer_handle,
                        main_cursor_position,
                    )
                });
                return;
            }

            state.completion_positions[main_cursor_index]
        }
    };

    if word.position > main_completion_position {
        cancel_completion(ctx.editor);
        return;
    }

    match ctx.editor.mode.insert_state.lsp_client_handle {
        Some(_) => ctx
            .editor
            .picker
            .filter(WordIndicesIter::empty(), word.text),
        None => {
            ctx.editor
                .picker
                .filter(ctx.editor.word_database.word_indices(), word.text);
            if ctx.editor.picker.cursor().is_none() {
                ctx.editor.picker.move_cursor(0);
            }
            if ctx.editor.picker.len() == 1 {
                ctx.editor.picker.clear();
            }
        }
    }
    */
}

fn apply_completion(
    ctx: &mut EditorContext,
    buffer_view_handle: BufferViewHandle,
    cursor_movement: isize,
) {
    /*
        let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);

        ctx.editor.picker.move_cursor(cursor_movement);
        let entry = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
            Some((_, entry)) => entry,
            None => {
                let buffer_handle = buffer_view.buffer_handle;
                let buffer = ctx.editor.buffers.get(buffer_handle);
                let state = &mut ctx.editor.mode.insert_state;
                let lsp_client_handle = match state.get_lsp_client_handle(&ctx.editor.lsp, &buffer.path)
                {
                    Some(handle) => handle,
                    None => return,
                };

                let content = buffer.content();
                state.completion_positions.clear();
                for cursor in &buffer_view.cursors[..] {
                    let word = content.word_at(content.position_before(cursor.position));
                    let position = match word.kind {
                        WordKind::Identifier => word.position,
                        _ => cursor.position,
                    };
                    state.completion_positions.push(position);
                }

                let platform = &mut *ctx.platform;
                let client_handle = ctx.client_handle;
                let buffer_position = buffer_view.cursors.main_cursor().position;
                lsp::ClientManager::access(ctx.editor, lsp_client_handle, |e, c| {
                    c.completion(e, platform, client_handle, buffer_handle, buffer_position)
                });

                return;
            }
        };

        let completion = ctx.editor.string_pool.acquire_with(entry);
        buffer_view.apply_completion(
            &mut ctx.editor.buffers,
            &mut ctx.editor.word_database,
            &completion,
            &ctx.editor.mode.insert_state.completion_positions,
            &mut ctx.editor.events,
        );
        ctx.editor.string_pool.release(completion);
    */
}
