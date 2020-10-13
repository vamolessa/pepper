use crate::{
    buffer_position::BufferPosition,
    cursor::Cursor,
    editor::{KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    navigation_history::{NavigationDirection, NavigationHistory},
    word_database::WordKind,
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        NavigationHistory::save_client_snapshot(ctx.clients, ctx.buffer_views, ctx.target_client);
        ctx.read_line.reset("#");
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.read_line.reset("");
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match ctx.read_line.poll(keys) {
            ReadLinePoll::Pending => {
                let line_number: usize = match ctx.read_line.input().parse() {
                    Ok(number) => number,
                    Err(_) => return ModeOperation::None,
                };
                let line_index = line_number.saturating_sub(1);

                let handle = unwrap_or_none!(ctx.current_buffer_view_handle());
                let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
                let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));

                let mut position = BufferPosition::line_col(line_index, 0);
                let (first_word, _, mut right_words) = buffer.content().words_from(position);
                if first_word.kind == WordKind::Whitespace {
                    if let Some(word) = right_words.next() {
                        position = word.position;
                    }
                }

                let mut cursors = buffer_view.cursors.mut_guard();
                cursors.clear();
                cursors.add(Cursor {
                    anchor: position,
                    position,
                });

                ModeOperation::None
            }
            ReadLinePoll::Submitted => ModeOperation::EnterMode(Mode::default()),
            ReadLinePoll::Canceled => {
                NavigationHistory::move_in_history(
                    ctx.clients,
                    ctx.buffer_views,
                    ctx.target_client,
                    NavigationDirection::Backward,
                );
                ModeOperation::EnterMode(Mode::default())
            }
        }
    }
}
