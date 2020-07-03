use crate::{
    buffer_position::BufferOffset,
    buffer_view::MovementKind,
    event::Key,
    mode::{Mode, ModeContext, Operation},
};

fn on_event_no_buffer(ctx: ModeContext) -> Operation {
    match ctx.keys {
        [Key::Char('q')] => return Operation::Waiting,
        [Key::Char('q'), Key::Char('q')] => return Operation::Exit,
        _ => (),
    }

    Operation::None
}

pub fn on_enter(_ctx: ModeContext) {}

pub fn on_event(ctx: ModeContext) -> Operation {
    let handle = if let Some(handle) = ctx.current_buffer_view_handle() {
        handle
    } else {
        return on_event_no_buffer(ctx);
    };

    match ctx.keys {
        [Key::Char('h')] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, -1),
                MovementKind::PositionWithAnchor,
            );
        }
        [Key::Char('j')] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(1, 0),
                MovementKind::PositionWithAnchor,
            );
        }
        [Key::Char('k')] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(-1, 0),
                MovementKind::PositionWithAnchor,
            );
        }
        [Key::Char('l')] => {
            ctx.buffer_views.get_mut(handle).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, 1),
                MovementKind::PositionWithAnchor,
            );
        }
        [Key::Char('J')] => {
            let buffer_view = ctx.buffer_views.get_mut(handle);
            let buffer_handle = buffer_view.buffer_handle;
            let buffer_line_count = ctx
                .buffers
                .get(buffer_handle)
                .map(|b| b.content.line_count())
                .unwrap_or(0);
            let mut cursor = *buffer_view.cursors.main_cursor();
            cursor.position.column_index = 0;
            cursor.position.line_index += 1;
            cursor.position.line_index = cursor.position.line_index.min(buffer_line_count - 1);
            cursor.anchor = cursor.position;
            buffer_view.cursors.add_cursor(cursor);
        }
        [Key::Char('i')] => return Operation::EnterMode(Mode::Insert),
        [Key::Char('v')] => return Operation::EnterMode(Mode::Select),
        [Key::Char('s')] => return Operation::EnterMode(Mode::Search),
        [Key::Char('n')] => {
            ctx.buffer_views
                .get_mut(handle)
                .move_to_next_search_match(ctx.buffers, MovementKind::PositionWithAnchor);
        }
        [Key::Char('N')] => {
            ctx.buffer_views
                .get_mut(handle)
                .move_to_previous_search_match(ctx.buffers, MovementKind::PositionWithAnchor);
        }
        [Key::Char('u')] => ctx.buffer_views.undo(ctx.buffers, handle),
        [Key::Char('U')] => ctx.buffer_views.redo(ctx.buffers, handle),
        [Key::Ctrl('s')] => {
            let buffer_handle = ctx.buffer_views.get(handle).buffer_handle;
            if let Some(buffer) = ctx.buffers.get(buffer_handle) {
                let mut file = std::fs::File::create("buffer_content.txt").unwrap();
                buffer.content.write(&mut file).unwrap();
            }
        }
        [Key::Tab] => return Operation::NextViewport,
        _ => return on_event_no_buffer(ctx),
    };

    Operation::None
}
