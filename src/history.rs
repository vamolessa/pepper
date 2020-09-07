use std::ops::Range;

use crate::buffer_position::BufferRange;

#[derive(Clone, Copy)]
pub enum EditKind {
    Insert,
    Delete,
}

#[derive(Clone, Copy)]
pub struct Edit<'a> {
    pub kind: EditKind,
    pub range: BufferRange,
    pub text: &'a str,
}

struct EditInternal {
    pub kind: EditKind,
    pub buffer_range: BufferRange,
    pub texts_range: Range<usize>,
}

impl EditInternal {
    pub fn as_edit_ref<'a>(&self, texts: &'a str) -> Edit<'a> {
        Edit {
            kind: self.kind,
            range: self.buffer_range,
            text: &texts[self.texts_range.clone()],
        }
    }
}

enum HistoryState {
    IterIndex(usize),
    InsertGroup(Range<usize>),
}

pub struct History {
    texts: String,
    edits: Vec<EditInternal>,
    group_ranges: Vec<Range<usize>>,
    state: HistoryState,
}

impl History {
    pub fn new() -> Self {
        Self {
            texts: String::new(),
            edits: Vec::new(),
            group_ranges: Vec::new(),
            state: HistoryState::IterIndex(0),
        }
    }

    pub fn add_edit(&mut self, edit: Edit) {
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

        let texts_range_start = self.texts.len();
        self.texts.push_str(edit.text);
        self.edits.push(EditInternal {
            kind: edit.kind,
            buffer_range: edit.range,
            texts_range: texts_range_start..self.texts.len(),
        });
    }

    pub fn commit_edits(&mut self) {
        if let HistoryState::InsertGroup(range) = &self.state {
            self.group_ranges.push(range.clone());
            self.state = HistoryState::IterIndex(self.group_ranges.len());
        }
    }

    pub fn undo_edits(&mut self) -> impl Clone + Iterator<Item = Edit> {
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

        let texts = &self.texts[..];
        self.edits[range].iter().rev().map(move |e| {
            let mut edit = e.as_edit_ref(texts);
            edit.kind = match edit.kind {
                EditKind::Insert => EditKind::Delete,
                EditKind::Delete => EditKind::Insert,
            };
            edit
        })
    }

    pub fn redo_edits(&mut self) -> impl Clone + Iterator<Item = Edit> {
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

        let texts = &self.texts[..];
        self.edits[range].iter().map(move |e| e.as_edit_ref(texts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer_position::BufferRange;

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

        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::default(),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: BufferRange::default(),
            text: "b",
        });

        assert_eq!(0, history.redo_edits().count());

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Insert));
        assert!(matches!(edit.text, "b"));
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Delete));
        assert!(matches!(edit.text, "a"));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        let mut edit_iter = history.redo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Insert));
        assert!(matches!(edit.text, "a"));
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Delete));
        assert!(matches!(edit.text, "b"));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        assert_eq!(0, history.redo_edits().count());

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Insert));
        assert!(matches!(edit.text, "b"));
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Delete));
        assert!(matches!(edit.text, "a"));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::default(),
            text: "c",
        });

        assert_eq!(0, history.redo_edits().count());

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert!(matches!(edit.kind, EditKind::Delete));
        assert!(matches!(edit.text, "c"));
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        assert_eq!(0, history.undo_edits().count());
    }
}
