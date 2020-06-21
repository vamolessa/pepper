use std::ops::Range;

use crate::{
    buffer::{Text, TextRef},
    buffer_position::BufferRange,
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
    pub fn as_edit_ref<'a>(&'a self) -> EditRef<'a> {
        EditRef {
            kind: self.kind,
            range: self.range,
            text: self.text.as_text_ref(),
        }
    }
}

pub struct EditRef<'a> {
    pub kind: EditKind,
    pub range: BufferRange,
    pub text: TextRef<'a>,
}

pub struct History {
    edits: Vec<Edit>,
    group_ranges: Vec<Range<usize>>,
    current_group_index: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            edits: Vec::new(),
            group_ranges: vec![0..0],
            current_group_index: 0,
        }
    }

    pub fn push_edit(&mut self, edit: Edit) {
        self.edits
            .truncate(self.group_ranges[self.current_group_index].end);
        self.group_ranges.truncate(self.current_group_index + 1);

        self.edits.push(edit);
        self.group_ranges[self.current_group_index].end += 1;
    }

    pub fn commit_edits(&mut self) {
        let range = &self.group_ranges[self.current_group_index];
        if range.end - range.start > 0 {
            self.current_group_index = self.group_ranges.len();
            let edits_end_index = self.edits.len();
            self.group_ranges.push(edits_end_index..edits_end_index);
        }
    }

    pub fn undo_edits<'a>(&'a mut self) -> impl Iterator<Item = EditRef<'a>> {
        self.commit_edits();

        let range = self.get_current_group_edit_range();
        if self.current_group_index > 0 {
            self.current_group_index -= 1;
        }

        self.edits[range].iter().rev().map(|e| {
            let mut edit = e.as_edit_ref();
            edit.kind = match edit.kind {
                EditKind::Insert => EditKind::Delete,
                EditKind::Delete => EditKind::Insert,
            };
            edit
        })
    }

    pub fn redo_edits<'a>(&'a mut self) -> impl Iterator<Item = EditRef<'a>> {
        self.commit_edits();

        let range = self.get_current_group_edit_range();
        if self.current_group_index < self.group_ranges.len() - 1 {
            self.current_group_index += 1;
        }

        self.edits[range].iter().map(|e| e.as_edit_ref())
    }

    fn get_current_group_edit_range(&self) -> Range<usize> {
        let range = &self.group_ranges[self.current_group_index];
        if range.start == 0 || range.end - range.start > 0 {
            range.clone()
        } else {
            self.group_ranges[self.current_group_index - 1].clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        buffer::{Text, TextRef},
        buffer_position::BufferRange,
    };

    #[test]
    fn commit_edits_on_emtpy_history() {
        let mut history = History::new();
        assert_eq!(0, history.undo_edits().count());
        assert_eq!(0, history.redo_edits().count());
        history.commit_edits();
        assert_eq!(0, history.redo_edits().count());
        assert_eq!(0, history.undo_edits().count());
        history.commit_edits();
        history.commit_edits();
        assert_eq!(0, history.undo_edits().count());
        assert_eq!(0, history.redo_edits().count());
    }

    #[test]
    fn edit_grouping() {
        let mut history = History::new();

        history.push_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::default(),
            text: Text::Char('a'),
        });
        history.push_edit(Edit {
            kind: EditKind::Delete,
            range: BufferRange::default(),
            text: Text::Char('b'),
        });

        assert_eq!(0, history.redo_edits().count());

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Insert));
        assert!(matches!(edit.text, TextRef::Char('b')));
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Delete));
        assert!(matches!(edit.text, TextRef::Char('a')));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        let mut edit_iter = history.redo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Insert));
        assert!(matches!(edit.text, TextRef::Char('a')));
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Delete));
        assert!(matches!(edit.text, TextRef::Char('b')));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        assert_eq!(0, history.redo_edits().count());

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Insert));
        assert!(matches!(edit.text, TextRef::Char('b')));
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Delete));
        assert!(matches!(edit.text, TextRef::Char('a')));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        history.push_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::default(),
            text: Text::Char('a'),
        });
    }
}
