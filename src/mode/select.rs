use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer_position::BufferOffset,
    buffer_view::MovementKind,
    client_event::Key,
    editor::KeysIterator,
    mode::{FromMode, Mode, ModeContext, ModeOperation},
};

pub fn on_enter(_ctx: &mut ModeContext) {}
pub fn on_exit(_ctx: &mut ModeContext) {}

pub fn on_event(ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    let handle = match ctx.current_buffer_view_handle() {
        Some(handle) => handle,
        None => return ModeOperation::EnterMode(Mode::Normal),
    };

    match keys.next() {
        Key::Esc | Key::Ctrl('c') => {
            let view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
            view.commit_edits(ctx.buffers);
            view.cursors.collapse_anchors();
            return ModeOperation::EnterMode(Mode::Normal);
        }
        Key::Char('h') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, -1),
                MovementKind::PositionOnly,
            );
        }
        Key::Char('j') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(1, 0),
                MovementKind::PositionOnly,
            );
        }
        Key::Char('k') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(-1, 0),
                MovementKind::PositionOnly,
            );
        }
        Key::Char('l') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, 1),
                MovementKind::PositionOnly,
            );
        }
        Key::Char('o') => unwrap_or_none!(ctx.buffer_views.get_mut(handle))
            .cursors
            .swap_positions_and_anchors(),
        Key::Char('s') => return ModeOperation::EnterMode(Mode::Search(FromMode::Select)),
        Key::Char('d') => {
            ctx.buffer_views.delete_in_selection(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
            );
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).commit_edits(ctx.buffers);
            return ModeOperation::EnterMode(Mode::Normal);
        }
        Key::Char('y') => {
            if let Ok(mut clipboard) = ClipboardContext::new() {
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
                let mut text = String::new();
                buffer_view.get_selection_text(ctx.buffers, &mut text);
                let _ = clipboard.set_contents(text);
            }
        }
        Key::Char('p') => {
            ctx.buffer_views.delete_in_selection(
                ctx.buffers,
                ctx.word_database,
                &ctx.config.syntaxes,
                handle,
            );
            if let Ok(text) = ClipboardContext::new().and_then(|mut c| c.get_contents()) {
                ctx.buffer_views.insert_text(
                    ctx.buffers,
                    ctx.word_database,
                    &ctx.config.syntaxes,
                    handle,
                    &text,
                );
            }
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).commit_edits(ctx.buffers);
            return ModeOperation::EnterMode(Mode::Normal);
        }
        Key::Char('n') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_next_search_match(ctx.buffers, MovementKind::PositionOnly);
        }
        Key::Char('N') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_previous_search_match(ctx.buffers, MovementKind::PositionOnly);
        }
        Key::Char(':') => return ModeOperation::EnterMode(Mode::Script(FromMode::Select)),
        _ => (),
    };

    ModeOperation::None
}
