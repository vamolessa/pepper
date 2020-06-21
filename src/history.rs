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

enum HistoryState {
    IterStart,
    IterIndex(usize),
    IterEnd,
    InsertGroup(Range<usize>),
}

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
            state: HistoryState::IterEnd,
        }
    }

    pub fn push_edit(&mut self, edit: Edit) {
        match self.state {
            HistoryState::IterStart => {
                self.edits.clear();
                self.group_ranges.clear();
                self.state = HistoryState::InsertGroup(0..1);
            }
            HistoryState::IterIndex(index) => {
                let range = &self.group_ranges[index];
                self.edits.truncate(range.start);
                self.state = HistoryState::InsertGroup(range.start..range.start + 1);
                self.group_ranges.truncate(index);
            }
            HistoryState::IterEnd => {
                let edit_count = self.edits.len();
                self.state = HistoryState::InsertGroup(edit_count..edit_count + 1);
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
            self.state = HistoryState::IterEnd;
        }
    }

    pub fn undo_edits<'a>(&'a mut self) -> impl Iterator<Item = EditRef<'a>> {
        self.commit_edits();

        let range = match self.state {
            HistoryState::IterStart => 0..0,
            HistoryState::IterIndex(ref mut index) => {
                if *index > 0 {
                    *index -= 1;
                    self.group_ranges[*index].clone()
                } else {
                    self.state = HistoryState::IterStart;
                    0..0
                }
            }
            HistoryState::IterEnd => {
                let group_count = self.group_ranges.len();
                if group_count > 0 {
                    self.state = HistoryState::IterIndex(group_count - 1);
                    self.group_ranges[group_count - 1].clone()
                } else {
                    self.state = HistoryState::IterStart;
                    0..0
                }
            }
            HistoryState::InsertGroup(_) => unreachable!(),
        };

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

        let range = match self.state {
            HistoryState::IterStart => {
                if self.group_ranges.len() > 0 {
                    self.state = HistoryState::IterIndex(0);
                    self.group_ranges[0].clone()
                } else {
                    self.state = HistoryState::IterEnd;
                    0..0
                }
            }
            HistoryState::IterIndex(ref mut index) => {
                *index += 1;
                if *index < self.group_ranges.len() {
                    self.group_ranges[*index].clone()
                } else {
                    self.state = HistoryState::IterEnd;
                    0..0
                }
            }
            HistoryState::IterEnd => 0..0,
            HistoryState::InsertGroup(_) => unreachable!(),
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
