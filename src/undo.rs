use crate::buffer::BufferPosition;

pub enum Text {
    Char(char),
    String(String),
}

pub enum Edit {
    Insert(Text, BufferPosition),
    Delete(BufferPosition, BufferPosition),
}

pub struct Undo {
    history: Vec<Edit>,
    group_end_indexes: Vec<usize>,
    current_group_index: usize,
}

impl Undo {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            group_end_indexes: vec![0, 0],
            current_group_index: 1,
        }
    }

    pub fn push_edit(&mut self, edit: Edit) {
        self.history
            .truncate(self.group_end_indexes[self.current_group_index]);
        self.group_end_indexes
            .truncate(self.current_group_index + 1);

        self.history.push(edit);
        self.group_end_indexes[self.current_group_index] += 1;
    }

    pub fn commit_edits(&mut self) {
        let current_group_size = self.group_end_indexes[self.current_group_index]
            - self.group_end_indexes[self.current_group_index - 1];
        if current_group_size > 0 {
            self.current_group_index = self.group_end_indexes.len();
            self.group_end_indexes.push(self.history.len());
        }
    }

    pub fn undo(&mut self) -> impl Iterator<Item = &Edit> {
        self.commit_edits();

        let start = self.group_end_indexes[self.current_group_index - 1];
        let end = self.group_end_indexes[self.current_group_index];
        self.history[start..end].iter().rev()
    }

    pub fn redo(&mut self) -> impl Iterator<Item = &Edit> {
        self.commit_edits();

        let start = self.group_end_indexes[self.current_group_index - 1];
        let end = self.group_end_indexes[self.current_group_index];
        self.history[start..end].iter()
    }
}
