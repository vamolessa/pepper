use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer_position::BufferOffset,
    buffer_view::MovementKind,
    client_event::Key,
    editor::KeysIterator,
    mode::{FromMode, Mode, ModeContext, ModeOperation},
};

fn on_event_no_buffer(_ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    match keys.next() {
        Key::Char(':') => ModeOperation::EnterMode(Mode::Script(FromMode::Normal)),
        _ => ModeOperation::None,
    }
}

pub fn on_enter(_ctx: &mut ModeContext) {}
pub fn on_exit(_ctx: &mut ModeContext) {}

pub fn on_event(ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    let handle = match ctx.current_buffer_view_handle() {
        Some(handle) => handle,
        None => return on_event_no_buffer(ctx, keys),
    };

    match keys.next() {
        Key::Char('h') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, -1),
                MovementKind::PositionWithAnchor,
            );
        }
        Key::Char('j') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(1, 0),
                MovementKind::PositionWithAnchor,
            );
        }
        Key::Char('k') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(-1, 0),
                MovementKind::PositionWithAnchor,
            );
        }
        Key::Char('l') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, 1),
                MovementKind::PositionWithAnchor,
            );
        }
        Key::Char(' ') => {
            let cursors = &mut unwrap_or_none!(ctx.buffer_views.get_mut(handle)).cursors;
            let main_cursor = *cursors.main_cursor();
            let mut cursors = cursors.mut_guard();
            cursors.clear();
            cursors.add_cursor(main_cursor);
        }
        Key::Char('J') => {
            let buffer_view = unwrap_or_none!(ctx.buffer_views.get_mut(handle));
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
            buffer_view.cursors.mut_guard().add_cursor(cursor);
        }
        Key::Char('i') => return ModeOperation::EnterMode(Mode::Insert),
        Key::Char('v') => return ModeOperation::EnterMode(Mode::Select),
        Key::Char('s') => return ModeOperation::EnterMode(Mode::Search(FromMode::Normal)),
        Key::Char('p') => {
            if let Ok(text) = ClipboardContext::new().and_then(|mut c| c.get_contents()) {
                ctx.buffer_views
                    .insert_text(ctx.buffers, &ctx.config.syntaxes, handle, &text[..]);
                unwrap_or_none!(ctx.buffer_views.get_mut(handle)).commit_edits(ctx.buffers);
            }
        }
        Key::Char('n') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_next_search_match(ctx.buffers, MovementKind::PositionWithAnchor);
        }
        Key::Char('N') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle))
                .move_to_previous_search_match(ctx.buffers, MovementKind::PositionWithAnchor);
        }
        Key::Char('u') => ctx
            .buffer_views
            .undo(ctx.buffers, &ctx.config.syntaxes, handle),
        Key::Char('U') => ctx
            .buffer_views
            .redo(ctx.buffers, &ctx.config.syntaxes, handle),
        _ => {
            keys.put_back();
            return on_event_no_buffer(ctx, keys);
        }
    };

    ModeOperation::None
}
