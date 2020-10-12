use crate::{
    editor::{KeysIterator, ReadLinePoll},
    mode::{Mode, ModeContext, ModeOperation, ModeState},
    navigation_history::{NavigationDirection, NavigationHistory},
};

#[derive(Default)]
pub struct State;

impl ModeState for State {
    fn on_enter(&mut self, ctx: &mut ModeContext) {
        NavigationHistory::save_client_snapshot(ctx.clients, ctx.buffer_views, ctx.target_client);

        ctx.read_line.reset("/");
        update_search(ctx);
    }

    fn on_exit(&mut self, ctx: &mut ModeContext) {
        ctx.read_line.reset("");
    }

    fn on_event(&mut self, ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
        match ctx.read_line.poll(keys) {
            ReadLinePoll::Pending => {
                update_search(ctx);
                ModeOperation::None
            }
            ReadLinePoll::Submited => {
                ctx.search.set_text(ctx.read_line.input());
                ModeOperation::EnterMode(Mode::default())
            }
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

fn update_search(ctx: &mut ModeContext) {
    for buffer in ctx.buffers.iter_mut() {
        buffer.set_search("");
    }

    let client = unwrap_or_return!(ctx.clients.get_mut(ctx.target_client));
    let handle = unwrap_or_return!(client.current_buffer_view_handle);
    let buffer_view = unwrap_or_return!(ctx.buffer_views.get_mut(handle));
    let buffer = unwrap_or_return!(ctx.buffers.get_mut(buffer_view.buffer_handle));
    buffer.set_search(&ctx.read_line.input());
    let search_ranges = buffer.search_ranges();

    if search_ranges.is_empty() {
        return;
    }

    let mut cursors = buffer_view.cursors.mut_guard();
    let main_cursor = cursors.main_cursor();
    match search_ranges.binary_search_by_key(&main_cursor.position, |r| r.from) {
        Ok(i) => main_cursor.position = search_ranges[i].from,
        Err(0) => main_cursor.position = search_ranges[0].from,
        Err(i) => {
            let before = search_ranges[i - 1].from;
            let after = search_ranges[i].from;

            let main_line_index = main_cursor.position.line_index;
            if main_line_index - before.line_index < after.line_index - main_line_index {
                main_cursor.position = before;
            } else {
                main_cursor.position = after;
            }
        }
    }

    main_cursor.anchor = main_cursor.position;

    let main_line_index = main_cursor.position.line_index;
    let height = client.height as usize;
    if main_line_index < client.scroll || main_line_index >= client.scroll + height {
        client.scroll = main_line_index.saturating_sub(height / 2);
    }
}
