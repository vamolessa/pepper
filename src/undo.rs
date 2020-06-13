use crate::buffer::BufferPosition;

pub enum Text {
    Char(char),
    String(String),
}

pub enum Command {
    Insert(Text, Vec<BufferPosition>),
    Delete(Vec<(BufferPosition, BufferPosition)>),
}

#[derive(Default)]
pub struct Undo {
    history: Vec<Command>,
    current_history_index: usize,
}

impl Undo {
    pub fn push_command(&mut self, command: Command) {
        self.current_history_index += 1;
        self.history.truncate(self.current_history_index);
        self.history.push(command);
    }

    pub fn undo(&mut self) -> Option<&Command> {
        if self.current_history_index > 0 {
            self.current_history_index -= 1;
        }

        if self.current_history_index < self.history.len() {
            Some(&self.history[self.current_history_index])
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<&Command> {
        if self.current_history_index < self.history.len() - 1 {
            self.current_history_index += 1;
        }

        if self.current_history_index < self.history.len() {
            Some(&self.history[self.current_history_index])
        } else {
            None
        }
    }
}
