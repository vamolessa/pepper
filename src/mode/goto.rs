use crate::{
    buffer_position::BufferPosition,
    cursor::Cursor,
    editor::KeysIterator,
    mode::{poll_input, InputPollResult, Mode, ModeContext, ModeOperation, ModeState},
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        ctx.save_snapshot_to_navigation_history();
        ctx.input.clear();
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.input.clear();
    }

    fn on_event(&mut self, mut ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
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

                let position = BufferPosition::line_col(line_index, 0);
                let position = buffer
                    .content
                    .words_from(position)
                    .2
                    .next()
                    .map(|w| w.position)
                    .unwrap_or(position);

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
                let client = unwrap_or_none!(ctx.clients.get_mut(ctx.target_client));
                let handle = unwrap_or_none!(client.current_buffer_view_handle);
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));

                let navigation_snapshot =
                    unwrap_or_none!(client.navigation_history.navigate_backward());

                let mut cursors = buffer_view.cursors.mut_guard();
                cursors.clear();
                for cursor in navigation_snapshot.cursors {
                    cursors.add(*cursor);
                }

                ModeOperation::EnterMode(Mode::default())
            }
        }
    }
}
