use crate::{
    buffer::{BufferCollection, TextRef},
    buffer_position::BufferOffset,
    buffer_view::BufferViewCollection,
    event::Key,
};

pub enum Transition {
    None,
    Waiting,
    Exit,
    EnterMode(Box<dyn Mode>),
}

pub trait Mode {
    fn on_event(
        &mut self,
        buffers: &mut BufferCollection,
        buffer_views: &mut BufferViewCollection,
        current_buffer_view_index: Option<usize>,
        keys: &[Key],
    ) -> Transition;
}

pub fn initial_mode() -> Box<dyn Mode> {
    Box::new(Normal)
}

pub struct Normal;
impl Normal {
    fn handle_no_buffer_events(&mut self, keys: &[Key]) -> Transition {
        match keys {
            [Key::Char('q')] => return Transition::Waiting,
            [Key::Char('q'), Key::Char('q')] => return Transition::Exit,
            _ => (),
        }

        Transition::None
    }
}
impl Mode for Normal {
    fn on_event(
        &mut self,
        buffers: &mut BufferCollection,
        buffer_views: &mut BufferViewCollection,
        current_buffer_view_index: Option<usize>,
        keys: &[Key],
    ) -> Transition {
        let index = if let Some(index) = current_buffer_view_index {
            index
        } else {
            return self.handle_no_buffer_events(keys);
        };

        match keys {
            [Key::Char('h')] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(0, -1), true);
            }
            [Key::Char('j')] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(1, 0), true);
            }
            [Key::Char('k')] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(-1, 0), true);
            }
            [Key::Char('l')] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(0, 1), true);
            }
            [Key::Char('J')] => {
                let buffer = buffer_views[index].buffer(buffers);
                let mut cursor = *buffer_views[index].cursors.main_cursor();
                cursor.position.column_index = 0;
                cursor.position.line_index += 1;
                cursor.position.line_index = cursor
                    .position
                    .line_index
                    .min(buffer.content.line_count() - 1);
                cursor.anchor = cursor.position;
                buffer_views[index].cursors.add_cursor(cursor);
            }
            [Key::Char('i')] => return Transition::EnterMode(Box::new(Insert)),
            [Key::Char('v')] => return Transition::EnterMode(Box::new(Selection)),
            [Key::Char('u')] => buffer_views.undo(buffers, index),
            [Key::Char('U')] => buffer_views.redo(buffers, index),
            [Key::Ctrl('s')] => {
                let buffer = buffer_views[index].buffer(buffers);
                let mut file = std::fs::File::create("buffer_content.txt").unwrap();
                buffer.content.write(&mut file).unwrap();
            }
            _ => return self.handle_no_buffer_events(keys),
        };

        Transition::None
    }
}

pub struct Selection;
impl Mode for Selection {
    fn on_event(
        &mut self,
        buffers: &mut BufferCollection,
        buffer_views: &mut BufferViewCollection,
        current_buffer_view_index: Option<usize>,
        keys: &[Key],
    ) -> Transition {
        let index = if let Some(index) = current_buffer_view_index {
            index
        } else {
            return Transition::EnterMode(Box::new(Normal));
        };

        match keys {
            [Key::Esc] | [Key::Ctrl('c')] => {
                buffer_views[index].commit_edits(buffers);
                buffer_views[index].cursors.collapse_anchors();
                return Transition::EnterMode(Box::new(Normal));
            }
            [Key::Char('h')] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(0, -1), false);
            }
            [Key::Char('j')] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(1, 0), false);
            }
            [Key::Char('k')] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(-1, 0), false);
            }
            [Key::Char('l')] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(0, 1), false);
            }
            [Key::Char('o')] => buffer_views[index].cursors.swap_positions_and_anchors(),
            [Key::Char('d')] => {
                buffer_views.remove_in_selection(buffers, index);
                buffer_views[index].commit_edits(buffers);
                return Transition::EnterMode(Box::new(Normal));
            }
            _ => (),
        };

        Transition::None
    }
}

struct Insert;
impl Mode for Insert {
    fn on_event(
        &mut self,
        buffers: &mut BufferCollection,
        buffer_views: &mut BufferViewCollection,
        current_buffer_view_index: Option<usize>,
        keys: &[Key],
    ) -> Transition {
        let index = if let Some(index) = current_buffer_view_index {
            index
        } else {
            return Transition::EnterMode(Box::new(Normal));
        };

        match keys {
            [Key::Esc] | [Key::Ctrl('c')] => {
                buffer_views[index].commit_edits(buffers);
                return Transition::EnterMode(Box::new(Normal));
            }
            [Key::Tab] => buffer_views.insert_text(buffers, index, TextRef::Char('\t')),
            [Key::Enter] | [Key::Ctrl('m')] => {
                buffer_views.insert_text(buffers, index, TextRef::Char('\n'))
            }
            [Key::Char(c)] => buffer_views.insert_text(buffers, index, TextRef::Char(*c)),
            [Key::Backspace] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(0, -1), false);
                buffer_views.remove_in_selection(buffers, index);
            }
            [Key::Delete] => {
                buffer_views[index].move_cursors(buffers, BufferOffset::line_col(0, 1), false);
                buffer_views.remove_in_selection(buffers, index);
            }
            _ => (),
        }

        Transition::None
    }
}
