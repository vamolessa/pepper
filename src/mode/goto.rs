use crate::{
    buffer_position::BufferPosition,
    cursor::Cursor,
    editor::KeysIterator,
    mode::{poll_input, InputPollResult, Mode, ModeContext, ModeOperation},
};

#[derive(Default)]
pub struct State {
    saved_position: BufferPosition,
}

pub fn on_enter(state: &mut State, ctx: &mut ModeContext) {
    state.saved_position = ctx
        .current_buffer_view_handle()
        .and_then(|h| ctx.buffer_views.get(h))
        .map(|v| v.cursors.main_cursor().position)
        .unwrap_or(Default::default());
    ctx.input.clear();
}

pub fn on_exit(ctx: &mut ModeContext) {
    ctx.input.clear();
}

pub fn on_event(
    state: &mut State,
    mut ctx: &mut ModeContext,
    keys: &mut KeysIterator,
) -> ModeOperation {
    match poll_input(&mut ctx, keys) {
        InputPollResult::Pending => {
            let line_number: usize = match ctx.input.parse() {
                Ok(number) => number,
                Err(_) => return ModeOperation::None,
            };
            let line_index = line_number.saturating_sub(1);

            let handle = unwrap_or_none!(ctx.current_buffer_view_handle());
            let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
            let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

            let position = BufferPosition::line_col(line_index, state.saved_position.column_byte_index);
            let position = buffer.content.saturate_position(position);

            let mut cursors = buffer_view.cursors.mut_guard();
            cursors.clear();
            cursors.add(Cursor {
                anchor: position,
                position,
            });

            ModeOperation::None
        }
        InputPollResult::Submited => ModeOperation::EnterMode(Mode::default()),
        InputPollResult::Canceled => {
            let handle = unwrap_or_none!(ctx.current_buffer_view_handle());
            let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
            let mut cursors = buffer_view.cursors.mut_guard();
            cursors.clear();
            cursors.add(Cursor {
                anchor: state.saved_position,
                position: state.saved_position,
            });
            ModeOperation::EnterMode(Mode::default())
        }
    }
}
