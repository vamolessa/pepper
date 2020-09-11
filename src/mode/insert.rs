use crate::{
    buffer_position::BufferOffset,
    buffer_view::{BufferViewHandle, MovementKind},
    client_event::Key,
    editor::KeysIterator,
    mode::{Mode, ModeContext, ModeOperation},
    picker::CustomPickerEntry,
};

pub fn on_enter(ctx: &mut ModeContext) {
    ctx.picker.reset();
    let words = &["mate", "material", "materialize", "materialization"];
    for word in words {
        ctx.picker.add_custom_entry(CustomPickerEntry {
            name: String::from(*word),
            description: String::new(),
        });
    }
}

pub fn on_exit(ctx: &mut ModeContext) {
    ctx.picker.reset();
}

pub fn on_event(ctx: &mut ModeContext, keys: &mut KeysIterator) -> ModeOperation {
    let handle = match ctx.current_buffer_view_handle() {
        Some(handle) => handle,
        None => return ModeOperation::EnterMode(Mode::Normal),
    };

    match keys.next() {
        Key::Esc | Key::Ctrl('c') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).commit_edits(ctx.buffers);
            return ModeOperation::EnterMode(Mode::Normal);
        }
        Key::Tab => ctx
            .buffer_views
            .insert_text(ctx.buffers, &ctx.config.syntaxes, handle, "\t"),
        Key::Ctrl('m') => {
            ctx.buffer_views
                .insert_text(ctx.buffers, &ctx.config.syntaxes, handle, "\n")
        }
        Key::Char(c) => {
            let mut buf = [0; std::mem::size_of::<char>()];
            let s = c.encode_utf8(&mut buf);
            ctx.buffer_views
                .insert_text(ctx.buffers, &ctx.config.syntaxes, handle, s);
        }
        Key::Ctrl('h') => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, -1),
                MovementKind::PositionOnly,
            );
            ctx.buffer_views
                .delete_in_selection(ctx.buffers, &ctx.config.syntaxes, handle);
        }
        Key::Delete => {
            unwrap_or_none!(ctx.buffer_views.get_mut(handle)).move_cursors(
                ctx.buffers,
                BufferOffset::line_col(0, 1),
                MovementKind::PositionOnly,
            );
            ctx.buffer_views
                .delete_in_selection(ctx.buffers, &ctx.config.syntaxes, handle);
        }
        Key::Ctrl('n') => {
            apply_completion(ctx, handle, 1);
            return ModeOperation::None;
        }
        Key::Ctrl('p') => {
            apply_completion(ctx, handle, -1);
            return ModeOperation::None;
        }
        _ => (),
    }

    let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));
    let main_cursor = buffer_view.cursors.main_cursor();
    let (word_range, word) = buffer.content.find_word_at(main_cursor.position);
    if word.is_empty() || main_cursor.position.column_index < word_range.to.column_index {
        ctx.picker.clear_filtered();
    } else {
        ctx.picker.filter(word);
    }

    ModeOperation::None
}

fn apply_completion(ctx: &mut ModeContext, handle: BufferViewHandle, cursor_movement: isize) {
    ctx.picker.move_cursor(cursor_movement);
    let entry = ctx.picker.entry(ctx.picker.cursor());
    ctx.buffer_views
        .apply_completion(ctx.buffers, &ctx.config.syntaxes, handle, &entry.name);
}
