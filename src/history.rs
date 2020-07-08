use std::ops::Range;

use crate::{
    buffer::{Text, TextRef},
    buffer_position::BufferRange,
};

#[derive(Debug, Clone, Copy)]
pub enum EditKind {
    Insert,
    Remove,
}

#[derive(Debug)]
pub struct Edit {
    pub kind: EditKind,
    pub range: BufferRange,
    pub text: Text,
}

impl Edit {
    pub fn as_edit_ref(&self) -> EditRef {
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

#[derive(Debug)]
enum HistoryState {
    IterIndex(usize),
    InsertGroup(Range<usize>),
}

#[derive(Debug)]
pub struct History {
    edits: Vec<Edit>,
    group_ranges: Vec<Range<usize>>,
    state: HistoryState,
}

impl History {
    pub fn new() -> Self {
        Self {
            edits: Vec::new(),
            group_ranges: Vec::new(),
            state: HistoryState::IterIndex(0),
        }
    }

    pub fn push_edit(&mut self, edit: Edit) {
        match self.state {
            HistoryState::IterIndex(index) => {
                let edit_index = if index < self.group_ranges.len() {
                    self.group_ranges[index].start
                } else {
                    self.edits.len()
                };
                self.edits.truncate(edit_index);
                self.state = HistoryState::InsertGroup(edit_index..edit_index + 1);
                self.group_ranges.truncate(index);
            }
            HistoryState::InsertGroup(ref mut range) => {
                range.end = self.edits.len() + 1;
            }
        }

        self.edits.push(edit);
    }

    pub fn commit_edits(&mut self) {
        if let HistoryState::InsertGroup(range) = &self.state {
            self.group_ranges.push(range.clone());
            self.state = HistoryState::IterIndex(self.group_ranges.len());
        }
    }

    pub fn undo_edits(&mut self) -> impl Iterator<Item = EditRef> {
        self.commit_edits();

        let range = match self.state {
            HistoryState::IterIndex(ref mut index) => {
                if *index > 0 {
                    *index -= 1;
                    self.group_ranges[*index].clone()
                } else {
                    0..0
                }
            }
            _ => unreachable!(),
        };

        self.edits[range].iter().rev().map(|e| {
            let mut edit = e.as_edit_ref();
            edit.kind = match edit.kind {
                EditKind::Insert => EditKind::Remove,
                EditKind::Remove => EditKind::Insert,
            };
            edit
        })
    }

    pub fn redo_edits(&mut self) -> impl Iterator<Item = EditRef> {
        self.commit_edits();

        let range = match self.state {
            HistoryState::IterIndex(ref mut index) => {
                if *index < self.group_ranges.len() {
                    let range = self.group_ranges[*index].clone();
                    *index += 1;
                    range
                } else {
                    0..0
                }
            }
            _ => unreachable!(),
        };

        self.edits[range].iter().map(|e| e.as_edit_ref())
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
            kind: EditKind::Remove,
            range: BufferRange::default(),
            text: Text::Char('b'),
        });

        assert_eq!(0, history.redo_edits().count());

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Insert));
        assert!(matches!(edit.text, TextRef::Char('b')));
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Remove));
        assert!(matches!(edit.text, TextRef::Char('a')));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        let mut edit_iter = history.redo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Insert));
        assert!(matches!(edit.text, TextRef::Char('a')));
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Remove));
        assert!(matches!(edit.text, TextRef::Char('b')));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        assert_eq!(0, history.redo_edits().count());

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Insert));
        assert!(matches!(edit.text, TextRef::Char('b')));
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Remove));
        assert!(matches!(edit.text, TextRef::Char('a')));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        history.push_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::default(),
            text: Text::Char('c'),
        });

        assert_eq!(0, history.redo_edits().count());

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Remove));
        assert!(matches!(edit.text, TextRef::Char('c')));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        assert_eq!(0, history.undo_edits().count());
    }
}
