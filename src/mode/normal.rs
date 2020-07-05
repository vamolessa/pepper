use copypasta::{ClipboardContext, ClipboardProvider};

use crate::{
    buffer::TextRef,
    buffer_position::BufferOffset,
    buffer_view::MovementKind,
    event::Key,
    mode::{FromMode, Mode, ModeContext, ModeOperation},
};

fn on_event_no_buffer(ctx: ModeContext) -> ModeOperation {
    match ctx.keys {
        [Key::Char('q')] => return ModeOperation::Pending,
        [Key::Char('q'), Key::Char('q')] => return ModeOperation::Quit,
        _ => (),
    }

    ModeOperation::None
}

pub fn on_enter(_ctx: ModeContext) {}

pub fn on_event(ctx: ModeContext) -> ModeOperation {
    let handle = if let Some(handle) = ctx
        .viewports
        .current_viewport()
        .current_buffer_view_handle()
    {
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
        [Key::Char('i')] => return ModeOperation::EnterMode(Mode::Insert),
        [Key::Char('v')] => return ModeOperation::EnterMode(Mode::Select),
        [Key::Char('s')] => return ModeOperation::EnterMode(Mode::Search(FromMode::Normal)),
        [Key::Char('p')] => {
            if let Ok(text) = ClipboardContext::new().and_then(|mut c| c.get_contents()) {
                ctx.buffer_views
                    .insert_text(ctx.buffers, handle, TextRef::Str(&text[..]));
                ctx.buffer_views.get_mut(handle).commit_edits(ctx.buffers);
            }
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
        [Key::Char(':')] => return ModeOperation::EnterMode(Mode::Command(FromMode::Normal)),
        [Key::Char('u')] => ctx.buffer_views.undo(ctx.buffers, handle),
        [Key::Char('U')] => ctx.buffer_views.redo(ctx.buffers, handle),
        [Key::Ctrl('p')] => {
            let mut child = std::process::Command::new("fzf").spawn().unwrap();
            child.wait().unwrap();
        }
        [Key::Ctrl('s')] => {
            let buffer_handle = ctx.buffer_views.get(handle).buffer_handle;
            if let Some(buffer) = ctx.buffers.get(buffer_handle) {
                let mut file = std::fs::File::create("buffer_content.txt").unwrap();
                buffer.content.write(&mut file).unwrap();
            }
        }
        [Key::Tab] => ctx.viewports.focus_next_viewport(ctx.buffer_views),
        _ => return on_event_no_buffer(ctx),
    };

    ModeOperation::None
}
