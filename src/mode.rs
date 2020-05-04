use std::collections::{hash_map::Entry, HashMap};

use crate::{buffer::BufferCollection, buffer_view::BufferView, event::Key};

#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Normal,
    Insert,
}

enum Keybind {
    Partial(usize),
    Command(Box<dyn Command>),
}

pub struct ModeData {
    keybindings: Vec<HashMap<Key, Keybind>>,
}

impl Default for ModeData {
    fn default() -> Self {
        Self {
            keybindings: vec![HashMap::default()],
        }
    }
}

impl ModeData {
    fn register_keybinding<I>(
        &mut self,
        mut key_iterator: I,
        command: Box<dyn Command>,
    ) -> Result<(), ()>
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

pub enum ModeTransition {
    None,
    Exit
}

pub struct Modes {
    pub current_mode: Mode,
    key_context: usize,
    normal_mode_data: ModeData,
    insert_mode_data: ModeData,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            current_mode: Mode::Normal,
            key_context: 0,
            normal_mode_data: normal_mode_data().unwrap(),
            insert_mode_data: insert_mode_data().unwrap(),
        }
    }
}

impl Modes {
    pub fn on_key(&mut self, buffer_view: &mut BufferView, buffers: &mut BufferCollection, key: Key) -> ModeTransition {
        let key_context = self.key_context;
        match self.current_mode_data_mut().keybindings[key_context].get(&key) {
            Some(Keybind::Partial(next)) => self.key_context = *next,
            Some(Keybind::Command(command)) => {
                match command.run(buffer_view, buffers) {
                    CommandTransition::None => (),
                    CommandTransition::Exit => return ModeTransition::Exit,
                    CommandTransition::EnterMode(mode) => self.current_mode = mode
                }
                self.key_context = 0;
            }
            None => self.key_context = 0,
        }

        ModeTransition::None
    }

    fn current_mode_data_mut(&mut self) -> &mut ModeData {
        match self.current_mode {
            Mode::Normal => &mut self.normal_mode_data,
            Mode::Insert => &mut self.insert_mode_data,
        }
    }
}

enum CommandTransition {
    None,
    Exit,
    EnterMode(Mode),
}

trait Command {
    fn run(&self, buffer_view: &mut BufferView, buffers: &mut BufferCollection) -> CommandTransition;
}

struct ExitCommand;
impl Command for ExitCommand {
    fn run(&self, _buffer_view: &mut BufferView, _buffers: &mut BufferCollection) -> CommandTransition {
        CommandTransition::Exit
    }
}

struct EnterModeCommand(pub Mode);
impl Command for EnterModeCommand {
    fn run(&self, _buffer_view: &mut BufferView, _buffers: &mut BufferCollection) -> CommandTransition {
        CommandTransition::EnterMode(self.0)
    }
}

struct MoveCursorCommand(pub i16, pub i16);
impl Command for MoveCursorCommand {
    fn run(&self, buffer_view: &mut BufferView, buffers: &mut BufferCollection) -> CommandTransition {
        buffer_view.move_cursor(buffers, (self.0, self.1));
        CommandTransition::None
    }
}

struct InsertTextCommand(pub String);
impl Command for InsertTextCommand {
    fn run(&self, buffer_view: &mut BufferView, buffers: &mut BufferCollection) -> CommandTransition {
        buffer_view.insert_text(buffers, &self.0);
        CommandTransition::None
    }
}

struct BreakLineCommand;
impl Command for BreakLineCommand {
    fn run(&self, buffer_view: &mut BufferView, buffers: &mut BufferCollection) -> CommandTransition {
        buffer_view.break_line(buffers);
        CommandTransition::None
    }
}

fn normal_mode_data() -> Result<ModeData, ()> {
    let mut mode = ModeData::default();
    mode.register_keybinding(
        [Key::Char('q'), Key::Char('q')].iter().cloned(),
        Box::new(ExitCommand),
    )?;
    mode.register_keybinding(
        [Key::Char('h')].iter().cloned(),
        Box::new(MoveCursorCommand(-1, 0)),
    )?;
    mode.register_keybinding(
        [Key::Char('j')].iter().cloned(),
        Box::new(MoveCursorCommand(0, 1)),
    )?;
    mode.register_keybinding(
        [Key::Char('k')].iter().cloned(),
        Box::new(MoveCursorCommand(-1, -1)),
    )?;
    mode.register_keybinding(
        [Key::Char('l')].iter().cloned(),
        Box::new(MoveCursorCommand(1, 0)),
    )?;
    mode.register_keybinding(
        [Key::Char('i')].iter().cloned(),
        Box::new(EnterModeCommand(Mode::Insert)),
    )?;

    Ok(mode)
}

fn insert_mode_data() -> Result<ModeData, ()> {
    let mut mode = ModeData::default();
    mode.register_keybinding(
        [Key::Esc].iter().cloned(),
        Box::new(EnterModeCommand(Mode::Normal)),
    )?;
    mode.register_keybinding(
        [Key::Ctrl('c')].iter().cloned(),
        Box::new(EnterModeCommand(Mode::Normal)),
    )?;
    mode.register_keybinding(
        [Key::Tab].iter().cloned(),
        Box::new(InsertTextCommand("    ".into())),
    )?;
    mode.register_keybinding([Key::Enter].iter().cloned(), Box::new(BreakLineCommand))?;

    for c in (b'a'..=b'z').chain(b'A'..=b'Z') {
        let c = c as char;
        let mut text = String::with_capacity(1);
        text.push(c);
        mode.register_keybinding(
            [Key::Char(c)].iter().cloned(),
            Box::new(InsertTextCommand(text)),
        )?;
    }

    Ok(mode)
}
