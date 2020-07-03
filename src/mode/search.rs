use crate::{
    event::Key,
    mode::{Mode, ModeContext, Operation},
};

pub fn on_enter(_ctx: ModeContext) {}
pub fn on_leave(_ctx: ModeContext) {}

pub fn on_event(ctx: ModeContext) -> Operation {
    let mut operation = Operation::None;

    match ctx.keys {
        [Key::Esc] | [Key::Ctrl('c')] => {
            ctx.input.clear();
            operation = Operation::EnterMode(Mode::Normal);
        }
        [Key::Ctrl('m')] => {
            operation = Operation::EnterMode(Mode::Normal);
        }
        [Key::Ctrl('w')] => {
            ctx.input.clear();
        }
        [Key::Ctrl('h')] => {
            if let Some((last_char_index, _)) = ctx.input.char_indices().rev().next() {
                ctx.input.drain(last_char_index..);
            }
        }
        [Key::Char(c)] => {
            ctx.input.push(*c);
        }
        _ => (),
    }

    if let Some(handle) = ctx.current_buffer_view_handle {
        let buffer_view = ctx.buffer_views.get(handle);
        if let Some(buffer) = ctx.buffers.get_mut(buffer_view.buffer_handle) {
            buffer.set_search(&ctx.input[..]);
        }
    }

    operation
}
