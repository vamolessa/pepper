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
impl Mode for Normal {
    fn on_event(
        &mut self,
        buffers: &mut BufferCollection,
        buffer_views: &mut BufferViewCollection,
        current_buffer_view_index: Option<usize>,
        keys: &[Key],
    ) -> Transition {
        match keys {
            [Key::Esc] | [Key::Ctrl('c')] => {
                buffer_views.commit_edits(buffers, current_buffer_view_index);
                return Transition::EnterMode(Box::new(Normal));
            }
            [Key::Char('h')] => {
                buffer_views.move_cursors(
                    buffers,
                    current_buffer_view_index,
                    BufferOffset::line_col(0, -1),
                );
                buffer_views.collapse_cursor_anchors(current_buffer_view_index);
            }
            [Key::Char('j')] => {
                buffer_views.move_cursors(
                    buffers,
                    current_buffer_view_index,
                    BufferOffset::line_col(1, 0),
                );
                buffer_views.collapse_cursor_anchors(current_buffer_view_index);
            }
            [Key::Char('k')] => {
                buffer_views.move_cursors(
                    buffers,
                    current_buffer_view_index,
                    BufferOffset::line_col(-1, 0),
                );
                buffer_views.collapse_cursor_anchors(current_buffer_view_index);
            }
            [Key::Char('l')] => {
                buffer_views.move_cursors(
                    buffers,
                    current_buffer_view_index,
                    BufferOffset::line_col(0, 1),
                );
                buffer_views.collapse_cursor_anchors(current_buffer_view_index);
            }
            [Key::Char('i')] => return Transition::EnterMode(Box::new(Insert)),
            [Key::Char('u')] => buffer_views.undo(buffers, current_buffer_view_index),
            [Key::Char('U')] => buffer_views.redo(buffers, current_buffer_view_index),
            [Key::Ctrl('s')] => {
                if let Some(index) = current_buffer_view_index {
                    let buffer = buffer_views[index].buffer_mut(buffers);
                    let mut file = std::fs::File::create("buffer_content.txt").unwrap();
                    buffer.content.write(&mut file).unwrap();
                }
            }
            _ => (),
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
        match keys {
            [Key::Char('q')] => return Transition::Waiting,
            [Key::Char('q'), Key::Char('q')] => return Transition::Exit,
            [Key::Char('h')] => {
                buffer_views.move_cursors(
                    buffers,
                    current_buffer_view_index,
                    BufferOffset::line_col(0, -1),
                );
            }
            [Key::Char('j')] => {
                buffer_views.move_cursors(
                    buffers,
                    current_buffer_view_index,
                    BufferOffset::line_col(1, 0),
                );
            }
            [Key::Char('k')] => {
                buffer_views.move_cursors(
                    buffers,
                    current_buffer_view_index,
                    BufferOffset::line_col(-1, 0),
                );
            }
            [Key::Char('l')] => {
                buffer_views.move_cursors(
                    buffers,
                    current_buffer_view_index,
                    BufferOffset::line_col(0, 1),
                );
            }
            [Key::Char('o')] => {
                buffer_views.swap_cursor_position_and_anchor(current_buffer_view_index)
            }
            [Key::Char('i')] => {
                buffer_views.delete_selection(buffers, current_buffer_view_index);
                return Transition::EnterMode(Box::new(Insert));
            }
            [Key::Char('d')] => {
                buffer_views.delete_selection(buffers, current_buffer_view_index);
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
        match keys {
            [Key::Esc] | [Key::Ctrl('c')] => {
                buffer_views.collapse_cursor_anchors(current_buffer_view_index);
                buffer_views.commit_edits(buffers, current_buffer_view_index);
                return Transition::EnterMode(Box::new(Normal));
            }
            [Key::Tab] => {
                buffer_views.insert_text(buffers, current_buffer_view_index, TextRef::Char('\t'))
            }
            [Key::Enter] | [Key::Ctrl('m')] => {
                buffer_views.insert_text(buffers, current_buffer_view_index, TextRef::Char('\n'))
            }
            [Key::Char(c)] => {
                buffer_views.insert_text(buffers, current_buffer_view_index, TextRef::Char(*c))
            }
            [Key::Delete] => buffer_views.delete_selection(buffers, current_buffer_view_index),
            _ => (),
        }

        Transition::None
    }
}
