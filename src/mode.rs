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
        current_buffer_view: Option<usize>,
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
        current_buffer_view: Option<usize>,
        keys: &[Key],
    ) -> Transition {
        let _ = match keys {
            [Key::Char('q')] => return Transition::Waiting,
            [Key::Char('q'), Key::Char('q')] => return Transition::Exit,
            [Key::Char('h')] => buffer_views.move_cursors(
                buffers,
                current_buffer_view,
                BufferOffset {
                    column_offset: -1,
                    line_offset: 0,
                },
            ),
            [Key::Char('j')] => buffer_views.move_cursors(
                buffers,
                current_buffer_view,
                BufferOffset {
                    column_offset: 0,
                    line_offset: 1,
                },
            ),
            [Key::Char('k')] => buffer_views.move_cursors(
                buffers,
                current_buffer_view,
                BufferOffset {
                    column_offset: 0,
                    line_offset: -1,
                },
            ),
            [Key::Char('l')] => buffer_views.move_cursors(
                buffers,
                current_buffer_view,
                BufferOffset {
                    column_offset: 1,
                    line_offset: 0,
                },
            ),
            [Key::Char('i')] => return Transition::EnterMode(Box::new(Insert)),
            [Key::Char('u')] => buffer_views.undo(buffers, current_buffer_view),
            [Key::Char('U')] => buffer_views.redo(buffers, current_buffer_view),
            [Key::Ctrl('s')] => buffer_views
                .get_mut(current_buffer_view)
                .and_then(|v| v.buffer_mut(buffers))
                .and_then(|b| {
                    let mut file = std::fs::File::create("buffer_content.txt").unwrap();
                    b.content.write(&mut file).unwrap();
                    None
                }),
            _ => None,
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
        current_buffer_view: Option<usize>,
        keys: &[Key],
    ) -> Transition {
        let _ = match keys {
            [Key::Esc] | [Key::Ctrl('c')] => {
                buffer_views.commit_edits(buffers, current_buffer_view);
                return Transition::EnterMode(Box::new(Normal));
            }
            [Key::Tab] => {
                buffer_views.insert_text(buffers, current_buffer_view, TextRef::Str("    "))
            }
            [Key::Enter] | [Key::Ctrl('m')] => {
                buffer_views.insert_text(buffers, current_buffer_view, TextRef::Char('\n'))
            }
            [Key::Char(c)] => {
                buffer_views.insert_text(buffers, current_buffer_view, TextRef::Char(*c))
            }
            [Key::Delete] => buffer_views.delete_selection(buffers, current_buffer_view),
            _ => None,
        };

        Transition::None
    }
}
