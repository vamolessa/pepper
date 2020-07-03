use crate::{
    buffer::TextRef,
    buffer_position::BufferOffset,
    buffer_view::MovementKind,
    event::Key,
    mode::{Mode, ModeContext, Operation},
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
            return Operation::EnterMode(Mode::Normal);
        }
        [Key::Tab] => ctx
            .buffer_views
            .insert_text(ctx.buffers, handle, TextRef::Char('\t')),
        [Key::Ctrl('m')] => ctx
            .buffer_views
            .insert_text(ctx.buffers, handle, TextRef::Char('\n')),
        [Key::Char(c)] => ctx
            .buffer_views
            .insert_text(ctx.buffers, handle, TextRef::Char(*c)),
        [Key::Ctrl('h')] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, -1),
                MovementKind::PositionOnly,
            );
            ctx.buffer_views.remove_in_selection(ctx.buffers, handle);
        }
        [Key::Delete] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, 1),
                MovementKind::PositionOnly,
            );
            ctx.buffer_views.remove_in_selection(ctx.buffers, handle);
        }
        _ => (),
    }

    Operation::None
}
