use crate::{
    buffer::{BufferCollection, TextRef},
    buffer_position::BufferOffset,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    event::Key,
};

pub enum Operation {
    None,
    Waiting,
    Exit,
    NextViewport,
    EnterMode(Box<dyn Mode>),
}

pub trait Mode {
    fn on_event(
        &mut self,
        buffers: &mut BufferCollection,
        buffer_views: &mut BufferViewCollection,
        current_buffer_view_handle: Option<&BufferViewHandle>,
        keys: &[Key],
    ) -> Operation;
}

pub fn initial_mode() -> Box<dyn Mode> {
    Box::new(Normal)
}

pub struct Normal;
impl Normal {
    fn handle_no_buffer_events(&mut self, keys: &[Key]) -> Operation {
        match keys {
            [Key::Char('q')] => return Operation::Waiting,
            [Key::Char('q'), Key::Char('q')] => return Operation::Exit,
            _ => (),
        }

        Operation::None
    }
}
impl Mode for Normal {
    fn on_event(
        &mut self,
        buffers: &mut BufferCollection,
        buffer_views: &mut BufferViewCollection,
        current_buffer_view_handle: Option<&BufferViewHandle>,
        keys: &[Key],
    ) -> Operation {
        let handle = if let Some(handle) = current_buffer_view_handle {
            handle
        } else {
            return self.handle_no_buffer_events(keys);
        };

        match keys {
            [Key::Char('h')] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(0, -1),
                    true,
                );
            }
            [Key::Char('j')] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(1, 0),
                    true,
                );
            }
            [Key::Char('k')] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(-1, 0),
                    true,
                );
            }
            [Key::Char('l')] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(0, 1),
                    true,
                );
            }
            [Key::Char('J')] => {
                let buffer_handle = buffer_views.get_mut(handle).buffer_handle;
                let buffer_line_count = buffers
                    .get(buffer_handle)
                    .map(|b| b.content.line_count())
                    .unwrap_or(0);
                let mut cursor = *buffer_views.get_mut(handle).cursors.main_cursor();
                cursor.position.column_index = 0;
                cursor.position.line_index += 1;
                cursor.position.line_index = cursor.position.line_index.min(buffer_line_count - 1);
                cursor.anchor = cursor.position;
                buffer_views.get_mut(handle).cursors.add_cursor(cursor);
            }
            [Key::Char('i')] => return Operation::EnterMode(Box::new(Insert)),
            [Key::Char('v')] => return Operation::EnterMode(Box::new(Selection)),
            [Key::Char('u')] => buffer_views.undo(buffers, handle),
            [Key::Char('U')] => buffer_views.redo(buffers, handle),
            [Key::Ctrl('s')] => {
                if let Some(buffer) = buffers.get(buffer_views.get_mut(handle).buffer_handle) {
                    let mut file = std::fs::File::create("buffer_content.txt").unwrap();
                    buffer.content.write(&mut file).unwrap();
                }
            }
            [Key::Tab] => return Operation::NextViewport,
            _ => return self.handle_no_buffer_events(keys),
        };

        Operation::None
    }
}

pub struct Selection;
impl Mode for Selection {
    fn on_event(
        &mut self,
        buffers: &mut BufferCollection,
        buffer_views: &mut BufferViewCollection,
        current_buffer_view_handle: Option<&BufferViewHandle>,
        keys: &[Key],
    ) -> Operation {
        let handle = if let Some(handle) = current_buffer_view_handle {
            handle
        } else {
            return Operation::EnterMode(Box::new(Normal));
        };

        match keys {
            [Key::Esc] | [Key::Ctrl('c')] => {
                buffer_views.get_mut(handle).commit_edits(buffers);
                buffer_views.get_mut(handle).cursors.collapse_anchors();
                return Operation::EnterMode(Box::new(Normal));
            }
            [Key::Char('h')] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(0, -1),
                    false,
                );
            }
            [Key::Char('j')] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(1, 0),
                    false,
                );
            }
            [Key::Char('k')] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(-1, 0),
                    false,
                );
            }
            [Key::Char('l')] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(0, 1),
                    false,
                );
            }
            [Key::Char('o')] => buffer_views
                .get_mut(handle)
                .cursors
                .swap_positions_and_anchors(),
            [Key::Char('d')] => {
                buffer_views.remove_in_selection(buffers, handle);
                buffer_views.get_mut(handle).commit_edits(buffers);
                return Operation::EnterMode(Box::new(Normal));
            }
            _ => (),
        };

        Operation::None
    }
}

struct Insert;
impl Mode for Insert {
    fn on_event(
        &mut self,
        buffers: &mut BufferCollection,
        buffer_views: &mut BufferViewCollection,
        current_buffer_view_handle: Option<&BufferViewHandle>,
        keys: &[Key],
    ) -> Operation {
        let handle = if let Some(handle) = current_buffer_view_handle {
            handle
        } else {
            return Operation::EnterMode(Box::new(Normal));
        };

        match keys {
            [Key::Esc] | [Key::Ctrl('c')] => {
                buffer_views.get_mut(handle).commit_edits(buffers);
                return Operation::EnterMode(Box::new(Normal));
            }
            [Key::Tab] => buffer_views.insert_text(buffers, handle, TextRef::Char('\t')),
            [Key::Enter] | [Key::Ctrl('m')] => {
                buffer_views.insert_text(buffers, handle, TextRef::Char('\n'))
            }
            [Key::Char(c)] => buffer_views.insert_text(buffers, handle, TextRef::Char(*c)),
            [Key::Backspace] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(0, -1),
                    false,
                );
                buffer_views.remove_in_selection(buffers, handle);
            }
            [Key::Delete] => {
                buffer_views.get_mut(handle).move_cursors(
                    buffers,
                    BufferOffset::line_col(0, 1),
                    false,
                );
                buffer_views.remove_in_selection(buffers, handle);
            }
            _ => (),
        }

        Operation::None
    }
}
