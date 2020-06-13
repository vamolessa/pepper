use crate::buffer::BufferPosition;

pub enum Text {
    Char(char),
    String(String),
}

pub enum Edit {
    Insert(Text, BufferPosition),
    Delete(BufferPosition, BufferPosition),
}

#[derive(Default)]
pub struct Undo {
    history: Vec<(Edit, usize)>,
    current_history_index: usize,
    current_edit_group_id: usize,
}

impl Undo {
    pub fn push_edit(&mut self, edit: Edit) {
        self.current_history_index += 1;
        self.history.truncate(self.current_history_index);
        self.history.push((edit, self.current_edit_group_id));
    }

    pub fn commit_edits(&mut self) {
        self.current_edit_group_id += 1;
    }

    pub fn undo(&mut self) -> Option<&Edit> {
        self.commit_edits();

        if self.current_history_index > 0 {
            self.current_history_index -= 1;
        }

        if self.current_history_index < self.history.len() {
            Some(&self.history[self.current_history_index].0)
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<&Edit> {
        self.commit_edits();

        if self.current_history_index < self.history.len() - 1 {
            self.current_history_index += 1;
        }

        if self.current_history_index < self.history.len() {
            Some(&self.history[self.current_history_index].0)
        } else {
            None
        }
    }
}
