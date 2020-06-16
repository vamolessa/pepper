use std::ops::Range;

use crate::{
    buffer::Text,
    buffer_position::{BufferOffset, BufferPosition, BufferRange},
};

#[derive(Clone, Copy)]
pub enum EditKind {
    Insert,
    Delete,
}

pub struct Edit {
    pub kind: EditKind,
    pub range: BufferRange,
    pub text: Text,
}

impl Edit {
    pub fn new(kind: EditKind, position: BufferPosition, text: Text) -> Self {
        let range = match &text {
            Text::Char(_c) => BufferRange::between(
                position,
                position.offset_by(BufferOffset {
                    column_offset: 1,
                    line_offset: 0,
                }),
            ),
            Text::String(s) => BufferRange::from_str_position(position, &s[..]),
        };
        Self { kind, text, range }
    }
}

pub struct History {
    edits: Vec<Edit>,
    group_end_indexes: Vec<usize>,
    current_group_index: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            edits: Vec::new(),
            group_end_indexes: vec![0, 0],
            current_group_index: 1,
        }
    }

    pub fn push_edit(&mut self, edit: Edit) {
        self.edits
            .truncate(self.group_end_indexes[self.current_group_index]);
        self.group_end_indexes
            .truncate(self.current_group_index + 1);

        self.edits.push(edit);
        self.group_end_indexes[self.current_group_index] += 1;
    }

    pub fn commit_edits(&mut self) {
        let current_group_size = self.group_end_indexes[self.current_group_index]
            - self.group_end_indexes[self.current_group_index - 1];
        if current_group_size > 0 {
            self.current_group_index = self.group_end_indexes.len();
            self.group_end_indexes.push(self.edits.len());
        }
    }

    pub fn undo_edits(&mut self) -> impl Iterator<Item = &Edit> {
        self.commit_edits();

        let range = self.get_current_group_edit_range();
        self.current_group_index = 1.max(self.current_group_index - 1);
        self.edits[range].iter_mut().rev().map(|e| {
            e.kind = match e.kind {
                EditKind::Insert => EditKind::Delete,
                EditKind::Delete => EditKind::Insert,
            };
            e as &_
        })
    }

    pub fn redo_edits(&mut self) -> impl Iterator<Item = &Edit> {
        self.commit_edits();

        let range = self.get_current_group_edit_range();
        self.current_group_index = self
            .group_end_indexes
            .len()
            .min(self.current_group_index + 1);
        self.edits[range].iter()
    }

    fn get_current_group_edit_range(&self) -> Range<usize> {
        let start = self.group_end_indexes[self.current_group_index - 1];
        let end = self.group_end_indexes[self.current_group_index];
        start..end
    }
}
