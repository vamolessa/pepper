use std::fmt::Write;

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{BufferViewHandle, CursorMovement, CursorMovementKind},
    client::ClientHandle,
    editor::{Editor, EditorContext, EditorControlFlow, KeysIterator},
    editor_utils::AUTO_MACRO_REGISTER,
    mode::{ModeKind, ModeState},
    platform::Key,
    plugin::{CompletionContext, PluginHandle},
    word_database::{WordIndicesIter, WordKind},
};

#[derive(Default)]
pub struct State {
    completion_positions: Vec<BufferPosition>,
    completing_plugin_handle: Option<PluginHandle>,
}

impl ModeState for State {
    fn on_enter(editor: &mut Editor) {
        cancel_completion(editor);
    }

    fn on_exit(editor: &mut Editor) {
        cancel_completion(editor);
    }

    fn on_client_keys(
        ctx: &mut EditorContext,
        client_handle: ClientHandle,
        keys: &mut KeysIterator,
    ) -> Option<EditorControlFlow> {
        let handle = match ctx.clients.get(client_handle).buffer_view_handle() {
            Some(handle) => handle,
            None => {
                ctx.editor.enter_mode(ModeKind::default());
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
                ctx.editor.enter_mode(ModeKind::default());
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
                apply_completion(ctx, client_handle, handle, 1);
                return Some(EditorControlFlow::Continue);
            }
            Key::Ctrl('p') => {
                apply_completion(ctx, client_handle, handle, -1);
                return Some(EditorControlFlow::Continue);
            }
            _ => return Some(EditorControlFlow::Continue),
        };

        ctx.trigger_event_handlers();
        update_completions(ctx, client_handle, handle);
        Some(EditorControlFlow::Continue)
    }
}

fn cancel_completion(editor: &mut Editor) {
    editor.picker.clear();
    editor.mode.insert_state.completion_positions.clear();
    editor.mode.insert_state.completing_plugin_handle = None;
}

fn update_completions(
    ctx: &mut EditorContext,
    client_handle: ClientHandle,
    buffer_view_handle: BufferViewHandle,
) {
    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
    let buffer_handle = buffer_view.buffer_handle;
    let buffer = ctx.editor.buffers.get(buffer_handle);
    let content = buffer.content();

    let main_cursor_position = buffer_view.cursors.main_cursor().position;
    let word = content.word_at(content.position_before(main_cursor_position));
    let word_range = BufferRange::between(word.position, word.end_position());

    let main_cursor_index = buffer_view.cursors.main_cursor_index();

    loop {
        match ctx
            .editor
            .mode
            .insert_state
            .completion_positions
            .get(main_cursor_index)
        {
            Some(&position) => {
                if main_cursor_position < position {
                    cancel_completion(&mut ctx.editor);
                    return;
                }
                if position == word.position {
                    break;
                }

                ctx.editor.mode.insert_state.completion_positions.clear();
            }
            None => {
                ctx.editor.picker.clear();

                let completion_requested = word.kind == WordKind::Identifier
                    && word.text.len() >= ctx.editor.config.completion_min_len as _;
                let completion_ctx = CompletionContext {
                    client_handle,
                    buffer_handle,
                    word_range,
                    cursor_position: main_cursor_position,
                    completion_requested,
                };

                ctx.editor.mode.insert_state.completing_plugin_handle = None;
                for plugin_handle in ctx.plugins.handles() {
                    let on_completion = ctx.plugins.get(plugin_handle).on_completion;
                    if on_completion(plugin_handle, ctx, &completion_ctx) {
                        ctx.editor.mode.insert_state.completing_plugin_handle = Some(plugin_handle);
                        break;
                    }
                }

                if !completion_requested
                    && ctx
                        .editor
                        .mode
                        .insert_state
                        .completing_plugin_handle
                        .is_none()
                {
                    cancel_completion(&mut ctx.editor);
                    return;
                }

                ctx.editor.mode.insert_state.completion_positions.clear();

                let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
                let buffer = ctx.editor.buffers.get(buffer_handle).content();
                for cursor in &buffer_view.cursors[..] {
                    let word = buffer.word_at(buffer.position_before(cursor.position));
                    let position = match word.kind {
                        WordKind::Identifier => word.position,
                        _ => cursor.position,
                    };
                    ctx.editor
                        .mode
                        .insert_state
                        .completion_positions
                        .push(position);
                }

                break;
            }
        }
    }

    let completion_filter = match ctx
        .editor
        .buffers
        .get(buffer_handle)
        .content()
        .text_range(word_range)
        .next()
    {
        Some(filter) => filter,
        None => {
            cancel_completion(&mut ctx.editor);
            return;
        }
    };

    match ctx.editor.mode.insert_state.completing_plugin_handle {
        Some(_) => {
            ctx.editor
                .picker
                .filter(WordIndicesIter::empty(), completion_filter);
        }
        None => {
            ctx.editor
                .picker
                .filter(ctx.editor.word_database.word_indices(), completion_filter);
            if ctx.editor.picker.cursor().is_none() {
                ctx.editor.picker.move_cursor(0);
            }
            if ctx.editor.picker.len() == 1 {
                ctx.editor.picker.clear();
            }
        }
    }
}

fn apply_completion(
    ctx: &mut EditorContext,
    client_handle: ClientHandle,
    buffer_view_handle: BufferViewHandle,
    cursor_movement: isize,
) {
    ctx.editor.picker.move_cursor(cursor_movement);
    let entry = match ctx.editor.picker.current_entry(&ctx.editor.word_database) {
        Some((_, entry)) => entry,
        None => {
            cancel_completion(&mut ctx.editor);

            let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
            let buffer_handle = buffer_view.buffer_handle;
            let cursor_position = buffer_view.cursors.main_cursor().position;
            let word_range = BufferRange::between(cursor_position, cursor_position);
            let completion_ctx = CompletionContext {
                client_handle,
                buffer_handle,
                word_range,
                cursor_position,
                completion_requested: true,
            };

            for plugin_handle in ctx.plugins.handles() {
                let on_completion = ctx.plugins.get(plugin_handle).on_completion;
                if !on_completion(plugin_handle, ctx, &completion_ctx) {
                    continue;
                }

                ctx.editor.mode.insert_state.completing_plugin_handle = Some(plugin_handle);

                let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
                let buffer = ctx.editor.buffers.get(buffer_handle).content();
                for cursor in &buffer_view.cursors[..] {
                    let word = buffer.word_at(buffer.position_before(cursor.position));
                    let position = match word.kind {
                        WordKind::Identifier => word.position,
                        _ => cursor.position,
                    };
                    ctx.editor
                        .mode
                        .insert_state
                        .completion_positions
                        .push(position);
                }

                break;
            }

            return;
        }
    };

    let completion = ctx.editor.string_pool.acquire_with(entry);
    let buffer_view = ctx.editor.buffer_views.get(buffer_view_handle);
    buffer_view.apply_completion(
        &mut ctx.editor.buffers,
        &mut ctx.editor.word_database,
        &completion,
        &ctx.editor.mode.insert_state.completion_positions,
        &mut ctx.editor.events,
    );
    ctx.editor.string_pool.release(completion);
}
