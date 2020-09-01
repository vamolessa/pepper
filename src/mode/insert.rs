use crate::{
    buffer::TextRef,
    buffer_position::BufferOffset,
    buffer_view::MovementKind,
    client_event::Key,
    editor::KeysIterator,
    mode::{Mode, ModeContext, ModeOperation},
    select::SelectEntry,
};

static AUTOCOMPLETE_ENTRIES: &[SelectEntry] = &[
    SelectEntry::from_str("matheus"),
    SelectEntry::from_str("mate"),
    SelectEntry::from_str("material"),
    SelectEntry::from_str("materializar"),
    SelectEntry::from_str("materiale"),
];

pub fn on_enter(ctx: &mut ModeContext) {
    ctx.selects.add_provider(Box::new(AUTOCOMPLETE_ENTRIES));
}

pub fn on_exit(ctx: &mut ModeContext) {
    ctx.selects.clear_filtered();
    ctx.selects.clear_providers();
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
        Key::Tab => ctx.buffer_views.insert_text(
            ctx.buffers,
            &ctx.config.syntaxes,
            handle,
            TextRef::Char('\t'),
        ),
        Key::Ctrl('m') => ctx.buffer_views.insert_text(
            ctx.buffers,
            &ctx.config.syntaxes,
            handle,
            TextRef::Char('\n'),
        ),
        Key::Char(c) => ctx.buffer_views.insert_text(
            ctx.buffers,
            &ctx.config.syntaxes,
            handle,
            TextRef::Char(c),
        ),
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
        Key::Ctrl('n') => ctx.selects.move_cursor(1),
        Key::Ctrl('p') => ctx.selects.move_cursor(-1),
        _ => (),
    }

    let buffer_view = unwrap_or_none!(ctx.buffer_views.get(handle));
    let buffer = unwrap_or_none!(ctx.buffers.get(buffer_view.buffer_handle));
    let main_cursor = buffer_view.cursors.main_cursor();
    let line = buffer.content.line(main_cursor.position.line_index);
    let line = line.text(..main_cursor.position.column_index);
    let current_word_index = line
        .char_indices()
        .rev()
        .take_while(|(_i, c)| c.is_alphanumeric())
        .last()
        .map(|(i, _c)| i);
    if let Some(index) = current_word_index {
        let current_word = &line[index..];
        ctx.selects.set_filter(current_word);
    } else {
        ctx.selects.clear_filtered();
    }

    ModeOperation::None
}
