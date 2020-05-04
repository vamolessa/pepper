use std::collections::{hash_map::Entry, HashMap};

use crate::{buffer::BufferCollection, buffer_view::BufferView, event::Key};

pub enum Keybind {
    Partial(usize),
    Command(Box<dyn Command>),
}

pub struct Mode {
    pub keybindings: Vec<HashMap<Key, Keybind>>,
}

impl Default for Mode {
    fn default() -> Self {
        Self {
            keybindings: vec![HashMap::default()],
        }
    }
}

impl Mode {
    pub fn register<I>(&mut self, mut key_iterator: I, command: Box<dyn Command>) -> Result<(), ()>
    where
        I: Iterator<Item = Key>,
    {
        let mut index = 0;
        let mut last_key = None;
        while let Some(key) = key_iterator.next() {
            match self.keybindings[index].entry(key) {
                Entry::Occupied(entry) => match entry.get() {
                    Keybind::Partial(next) => index = *next,
                    Keybind::Command(_) => return Err(()),
                },
                Entry::Vacant(_entry) => {
                    let next = self.keybindings.len();
                    self.keybindings.push(HashMap::default());
                    self.keybindings[index].insert(key, Keybind::Partial(index));
                    index = next;
                }
            }
            last_key = Some(key);
        }

        if let Some(key) = last_key {
            if let Some(entry) = self.keybindings[index].get_mut(&key) {
                *entry = Keybind::Command(command);
                if index > 0 {
                    self.keybindings.pop();
                }
                Ok(())
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }
}

pub enum Transition {
    None,
    Exit,
    Waiting,
    EnterMode(Box<dyn ModeTrait>),
}

pub trait Command {
    fn run(&self, buffer_view: &mut BufferView, buffers: &mut BufferCollection) -> Transition;
}

pub struct ExitCommand;
impl Command for ExitCommand {
    fn run(&self, _buffer_view: &mut BufferView, _buffers: &mut BufferCollection) -> Transition {
        Transition::Exit
    }
}

pub struct MoveCursorCommand(pub i16, pub i16);
impl Command for MoveCursorCommand {
    fn run(&self, buffer_view: &mut BufferView, buffers: &mut BufferCollection) -> Transition {
        buffer_view.move_cursor(buffers, (self.0, self.1));
        Transition::None
    }
}

pub fn new_normal_mode() -> Result<Mode, ()> {
    let mut mode = Mode::default();
    let it = [Key::Char('q'), Key::Char('q')].into_iter();
    mode.register(it, Box::new(ExitCommand))?;
    mode.register(
        std::iter::once(Key::Char('h')),
        Box::new(MoveCursorCommand(-1, 0)),
    )?;
    mode.register(
        std::iter::once(Key::Char('j')),
        Box::new(MoveCursorCommand(0, 1)),
    )?;
    mode.register(
        std::iter::once(Key::Char('k')),
        Box::new(MoveCursorCommand(0, -1)),
    )?;
    mode.register(
        std::iter::once(Key::Char('l')),
        Box::new(MoveCursorCommand(1, 0)),
    )?;

    Ok(mode)
}

//--------------------------------------------------

pub trait ModeTrait {
    fn on_event(
        &mut self,
        buffer_view: &mut BufferView,
        buffers: &mut BufferCollection,
        keys: &[Key],
    ) -> Transition;
}

pub fn initial_mode() -> Box<dyn ModeTrait> {
    Box::new(Normal)
}

pub struct Normal;

impl ModeTrait for Normal {
    fn on_event(
        &mut self,
        buffer_view: &mut BufferView,
        buffers: &mut BufferCollection,
        keys: &[Key],
    ) -> Transition {
        match keys {
            [Key::Char('q')] => return Transition::Waiting,
            [Key::Char('q'), Key::Char('q')] => return Transition::Exit,
            [Key::Char('h')] => buffer_view.move_cursor(buffers, (-1, 0)),
            [Key::Char('j')] => buffer_view.move_cursor(buffers, (0, 1)),
            [Key::Char('k')] => buffer_view.move_cursor(buffers, (0, -1)),
            [Key::Char('l')] => buffer_view.move_cursor(buffers, (1, 0)),
            [Key::Char('i')] => return Transition::EnterMode(Box::new(Insert)),
            _ => (),
        }

        Transition::None
    }
}

struct Insert;

impl ModeTrait for Insert {
    fn on_event(
        &mut self,
        buffer_view: &mut BufferView,
        buffers: &mut BufferCollection,
        keys: &[Key],
    ) -> Transition {
        match keys {
            [Key::Esc] | [Key::Ctrl('c')] => return Transition::EnterMode(Box::new(Normal)),
            [Key::Tab] => {
                buffer_view.insert_text(buffers, "    ");
            }
            [Key::Enter] => {
                buffer_view.break_line(buffers);
            }
            [Key::Char(c)] => {
                buffer_view.insert_text(buffers, c.encode_utf8(&mut [0 as u8; 4]));
            }
            _ => (),
        }

        Transition::None
    }
}
