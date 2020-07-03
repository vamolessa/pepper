use crate::{
    event::Key,
    mode::{ModeContext, Operation},
};

pub fn on_enter(ctx: ModeContext) {
    ctx.input.clear();
    update_search(ctx);
}

pub fn on_event(ctx: ModeContext) -> Operation {
    let mut operation = Operation::None;

    match ctx.keys {
        [Key::Esc] | [Key::Ctrl('c')] => {
            ctx.input.clear();
            operation = Operation::EnterMode(ctx.previous_mode);
        }
        [Key::Ctrl('m')] => {
            operation = Operation::EnterMode(ctx.previous_mode);
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

    update_search(ctx);
    operation
}

pub fn update_search(ctx: ModeContext) {
    for viewport in ctx.viewports.iter() {
        if let Some(handle) = viewport.current_buffer_view_handle() {
            let buffer_handle = ctx.buffer_views.get(handle).buffer_handle;
            if let Some(buffer) = ctx.buffers.get_mut(buffer_handle) {
                buffer.set_search(&ctx.input[..]);
            }
        };
    }
}
