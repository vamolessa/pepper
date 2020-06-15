use crate::buffer::{Buffer, BufferPosition, BufferRange};

pub enum Text {
    Char(char),
    String(String),
}

pub enum EditKind {
    Insert,
    Delete,
}

pub struct Edit {
    pub kind: EditKind,
    pub text: Text,
    pub range: BufferRange,
}

impl Edit {
    pub fn new(kind: EditKind, text: Text, position: BufferPosition) -> Self {
        let range = match &text {
            Text::Char(c) => {
                let mut buf = [0 as u8; 4];
                let s = c.encode_utf8(&mut buf);
                BufferRange::from_str_position(position, s)
            }
            Text::String(s) => BufferRange::from_str_position(position, &s[..]),
        };

        Self { kind, text, range }
    }
    
    pub fn appply(&self, buffer: &mut Buffer) {

    }
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
