use crate::{
    buffer::{BufferCollection, TextRef},
    buffer_position::BufferOffset,
    buffer_view::{BufferViewCollection, BufferViewHandle, MovementKind},
    event::Key,
};

pub enum Operation {
    None,
    Waiting,
    Exit,
    NextViewport,
    EnterMode(Mode),
}

pub enum Mode {
    Normal,
    Select,
    Insert,
    Search,
}

pub struct ModeContext<'a> {
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub current_buffer_view_handle: Option<&'a BufferViewHandle>,
    pub keys: &'a [Key],
    pub input: &'a mut String,
}

impl Mode {
    pub fn on_event(&mut self, context: ModeContext) -> Operation {
        match self {
            Mode::Normal => on_event_normal(context),
            Mode::Select => on_event_select(context),
            Mode::Insert => on_event_insert(context),
            Mode::Search => on_event_search(context),
        }
    }
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Normal
    }
}

fn on_event_normal_no_buffer(ctx: ModeContext) -> Operation {
    match ctx.keys {
        [Key::Char('q')] => return Operation::Waiting,
        [Key::Char('q'), Key::Char('q')] => return Operation::Exit,
        _ => (),
    }

    Operation::None
}

fn on_event_normal(ctx: ModeContext) -> Operation {
    let handle = if let Some(handle) = ctx.current_buffer_view_handle {
        handle
    } else {
        return on_event_normal_no_buffer(ctx);
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
            let buffer_handle = ctx.buffer_views.get(handle).buffer_handle;
            let buffer_line_count = ctx
                .buffers
                .get(buffer_handle)
                .map(|b| b.content.line_count())
                .unwrap_or(0);
            let mut cursor = *ctx.buffer_views.get(handle).cursors.main_cursor();
            cursor.position.column_index = 0;
            cursor.position.line_index += 1;
            cursor.position.line_index = cursor.position.line_index.min(buffer_line_count - 1);
            cursor.anchor = cursor.position;
            ctx.buffer_views.get_mut(handle).cursors.add_cursor(cursor);
        }
        [Key::Char('i')] => return Operation::EnterMode(Mode::Insert),
        [Key::Char('v')] => return Operation::EnterMode(Mode::Select),
        [Key::Char('s')] => {
            ctx.input.clear();
            let buffer_handle = ctx.buffer_views.get(handle).buffer_handle;
            if let Some(buffer) = ctx.buffers.get_mut(buffer_handle) {
                buffer.set_search(&ctx.input[..]);
            }
            return Operation::EnterMode(Mode::Search);
        }
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
            if let Some(buffer) = ctx.buffers.get(ctx.buffer_views.get(handle).buffer_handle) {
                let mut file = std::fs::File::create("buffer_content.txt").unwrap();
                buffer.content.write(&mut file).unwrap();
            }
        }
        [Key::Tab] => return Operation::NextViewport,
        _ => return on_event_normal_no_buffer(ctx),
    };

    Operation::None
}

fn on_event_select(ctx: ModeContext) -> Operation {
    let handle = if let Some(handle) = ctx.current_buffer_view_handle {
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
        [Key::Char('d')] => {
            ctx.buffer_views.remove_in_selection(ctx.buffers, handle);
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
        _ => (),
    };

    Operation::None
}

fn on_event_insert(ctx: ModeContext) -> Operation {
    let handle = if let Some(handle) = ctx.current_buffer_view_handle {
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

fn on_event_search(ctx: ModeContext) -> Operation {
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
