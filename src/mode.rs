use crate::{
    buffer::BufferCollection,
    buffer_view::BufferView,
    event::{Event, KeyEvent},
};

pub enum Transition {
    None,
    MoveToMode(Box<dyn Mode>),
    Exit,
}

pub trait Mode {
    fn on_event(
        &mut self,
        buffer_view: &mut BufferView,
        buffers: &mut BufferCollection,
        event: &Event,
    ) -> Transition;
}

pub struct Normal {}

impl Mode for Normal {
    fn on_event(
        &mut self,
        buffer_view: &mut BufferView,
        buffers: &mut BufferCollection,
        event: &Event,
    ) -> Transition {
        match event {
            Event::Key(KeyEvent::Char('q')) => return Transition::Exit,
            Event::Key(KeyEvent::Char('Q')) => return Transition::Exit,
            Event::Key(KeyEvent::Char('h')) => buffer_view.move_cursor(buffers, (-1, 0)),
            Event::Key(KeyEvent::Char('j')) => buffer_view.move_cursor(buffers, (0, 1)),
            Event::Key(KeyEvent::Char('k')) => buffer_view.move_cursor(buffers, (0, -1)),
            Event::Key(KeyEvent::Char('l')) => buffer_view.move_cursor(buffers, (1, 0)),
            _ => (),
        }

        Transition::None
    }
}
