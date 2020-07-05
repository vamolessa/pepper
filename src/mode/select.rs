use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer::TextRef,
    buffer_position::BufferOffset,
    buffer_view::MovementKind,
    event::Key,
    mode::{FromMode, Mode, ModeContext, Operation},
};

pub fn on_enter(_ctx: ModeContext) {}

pub fn on_event(ctx: ModeContext) -> Operation {
    let handle = if let Some(handle) = ctx.current_buffer_view_handle() {
        handle
    } else {
        return Operation::EnterMode(Mode::Normal);
    };

    match ctx.keys {
        [Key::Esc] | [Key::Ctrl('c')] => {
            ctx.buffer_views.get_mut(handle).commit_edits(ctx.buffers);
            ctx.buffer_views.get_mut(handle).cursors.collapse_anchors();
            return Operation::EnterMode(Mode::Normal);
        }
        [Key::Char('h')] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, -1),
                MovementKind::PositionOnly,
            );
        }
        [Key::Char('j')] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(1, 0),
                MovementKind::PositionOnly,
            );
        }
        [Key::Char('k')] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(-1, 0),
                MovementKind::PositionOnly,
            );
        }
        [Key::Char('l')] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, 1),
                MovementKind::PositionOnly,
            );
        }
        [Key::Char('o')] => ctx
            .buffer_views
            .get_mut(handle)
            .cursors
            .swap_positions_and_anchors(),
        [Key::Char('s')] => return Operation::EnterMode(Mode::Search(FromMode::Select)),
        [Key::Char('d')] => {
            ctx.buffer_views.remove_in_selection(ctx.buffers, handle);
            ctx.buffer_views.get_mut(handle).commit_edits(ctx.buffers);
            return Operation::EnterMode(Mode::Normal);
        }
        [Key::Char('y')] => {
            if let Ok(mut clipboard) = ClipboardContext::new() {
                let buffer_view = ctx.buffer_views.get(handle);
                let text = buffer_view.get_selection_text(ctx.buffers);
                match clipboard.set_contents(text) {
                    _ => (),
                }
            }
        }
        [Key::Char('p')] => {
            ctx.buffer_views.remove_in_selection(ctx.buffers, handle);
            if let Ok(text) = ClipboardContext::new().and_then(|mut c| c.get_contents()) {
                ctx.buffer_views
                    .insert_text(ctx.buffers, handle, TextRef::Str(&text[..]));
            }
            ctx.buffer_views.get_mut(handle).commit_edits(ctx.buffers);
            return Operation::EnterMode(Mode::Normal);
        }
        [Key::Char('n')] => {
            ctx.buffer_views
                .get_mut(handle)
                .move_to_next_search_match(ctx.buffers, MovementKind::PositionOnly);
        }
        [Key::Char('N')] => {
            ctx.buffer_views
                .get_mut(handle)
                .move_to_previous_search_match(ctx.buffers, MovementKind::PositionOnly);
        }
        [Key::Char(':')] => return Operation::EnterMode(Mode::Command(FromMode::Select)),
        _ => (),
    };

    Operation::None
}
