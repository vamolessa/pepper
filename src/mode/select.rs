use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer::TextRef,
    buffer_position::BufferOffset,
    buffer_view::MovementKind,
    client_event::Key,
    editor::KeysIterator,
    mode::{FromMode, Mode, ModeContext, ModeOperation},
};

pub fn on_enter(_ctx: &mut ModeContext) {}

pub fn on_event(ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    let handle = if let Some(handle) = ctx.current_buffer_view_handle {
        handle
    } else {
        return ModeOperation::EnterMode(Mode::Normal);
    };

    match keys.next() {
        Key::Esc | Key::Ctrl('c') => {
            ctx.buffer_views.get_mut(handle).commit_edits(ctx.buffers);
            ctx.buffer_views
                .get_mut(handle)
                .collapse_cursors_anchors(ctx.operations);
            return ModeOperation::EnterMode(Mode::Normal);
        }
        Key::Char('h') => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                ctx.operations,
                BufferOffset::line_col(0, -1),
                MovementKind::PositionOnly,
            );
        }
        Key::Char('j') => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                ctx.operations,
                BufferOffset::line_col(1, 0),
                MovementKind::PositionOnly,
            );
        }
        Key::Char('k') => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                ctx.operations,
                BufferOffset::line_col(-1, 0),
                MovementKind::PositionOnly,
            );
        }
        Key::Char('l') => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                ctx.operations,
                BufferOffset::line_col(0, 1),
                MovementKind::PositionOnly,
            );
        }
        Key::Char('o') => ctx
            .buffer_views
            .get_mut(handle)
            .swap_cursors_positions_and_anchors(ctx.operations),
        Key::Char('s') => return ModeOperation::EnterMode(Mode::Search(FromMode::Select)),
        Key::Char('d') => {
            ctx.buffer_views
                .delete_in_selection(ctx.buffers, ctx.operations, handle);
            ctx.buffer_views.get_mut(handle).commit_edits(ctx.buffers);
            return ModeOperation::EnterMode(Mode::Normal);
        }
        Key::Char('y') => {
            if let Ok(mut clipboard) = ClipboardContext::new() {
                let buffer_view = ctx.buffer_views.get(handle);
                let mut text = String::new();
                buffer_view.get_selection_text(ctx.buffers, &mut text);
                let _ = clipboard.set_contents(text);
            }
        }
        Key::Char('p') => {
            ctx.buffer_views
                .delete_in_selection(ctx.buffers, ctx.operations, handle);
            if let Ok(text) = ClipboardContext::new().and_then(|mut c| c.get_contents()) {
                ctx.buffer_views.insert_text(
                    ctx.buffers,
                    ctx.operations,
                    handle,
                    TextRef::Str(&text[..]),
                );
            }
            ctx.buffer_views.get_mut(handle).commit_edits(ctx.buffers);
            return ModeOperation::EnterMode(Mode::Normal);
        }
        Key::Char('n') => {
            ctx.buffer_views.get_mut(handle).move_to_next_search_match(
                ctx.buffers,
                ctx.operations,
                MovementKind::PositionOnly,
            );
        }
        Key::Char('N') => {
            ctx.buffer_views
                .get_mut(handle)
                .move_to_previous_search_match(
                    ctx.buffers,
                    ctx.operations,
                    MovementKind::PositionOnly,
                );
        }
        Key::Char(':') => return ModeOperation::EnterMode(Mode::Script(FromMode::Select)),
        _ => (),
    };

    ModeOperation::None
}
