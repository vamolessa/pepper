use std::ops::Range;

use crate::buffer_position::BufferRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        let current_group_len = match self.state {
            HistoryState::IterIndex(index) => {
                let edit_index = if index < self.group_ranges.len() {
                    self.group_ranges[index].start
                } else {
                    self.edits.len()
                };
                self.edits.truncate(edit_index);
                self.state = HistoryState::InsertGroup(edit_index..edit_index);
                self.group_ranges.truncate(index);
                0
            }
            HistoryState::InsertGroup(ref range) => range.end - range.start,
        };

        let mut append_edit = true;
        if current_group_len > 0 {
            let last_edit_index = self.edits.len() - 1;
            let last_edit = &mut self.edits[last_edit_index];

            match (last_edit.kind, edit.kind) {
                (EditKind::Insert, EditKind::Insert) => {
                    if edit.range.from == last_edit.buffer_range.to {
                        append_edit = false;
                        last_edit.buffer_range.to = edit.range.to;
                        self.texts.push_str(edit.text);
                        last_edit.texts_range.end = self.texts.len();
                    } else if edit.range.from == last_edit.buffer_range.from {
                        append_edit = false;
                        last_edit.buffer_range.to = last_edit.buffer_range.to.insert(edit.range);
                        self.texts
                            .insert_str(last_edit.texts_range.start, edit.text);
                        last_edit.texts_range.end = self.texts.len();
                    }
                }
                (EditKind::Delete, EditKind::Delete) => {
                    if edit.range.from == last_edit.buffer_range.from {
                        append_edit = false;
                        last_edit.buffer_range.to = last_edit.buffer_range.to.insert(edit.range);
                        self.texts.push_str(edit.text);
                        last_edit.texts_range.end = self.texts.len();
                    } else if edit.range.to == last_edit.buffer_range.from {
                        append_edit = false;
                        last_edit.buffer_range.from = edit.range.from;
                        self.texts
                            .insert_str(last_edit.texts_range.start, edit.text);
                        last_edit.texts_range.end = self.texts.len();
                    }
                }
                (EditKind::Insert, EditKind::Delete) => {
                    if last_edit.buffer_range.from == edit.range.from
                        && edit.range.to <= last_edit.buffer_range.to
                    {
                        let deleted_text_range = last_edit.texts_range.start
                            ..(last_edit.texts_range.start + edit.text.len());
                        if edit.text == &self.texts[deleted_text_range.clone()] {
                            append_edit = false;
                            last_edit.buffer_range.to =
                                last_edit.buffer_range.to.delete(edit.range);
                            self.texts.drain(deleted_text_range);
                            last_edit.texts_range.end = self.texts.len();
                        }
                    } else if edit.range.to == last_edit.buffer_range.to
                        && last_edit.buffer_range.from <= edit.range.from
                    {
                        let deleted_text_range = (last_edit.texts_range.end - edit.text.len())
                            ..last_edit.texts_range.end;
                        if edit.text == &self.texts[deleted_text_range.clone()] {
                            append_edit = false;
                            last_edit.buffer_range.to = edit.range.from;
                            self.texts.truncate(deleted_text_range.start);
                            last_edit.texts_range.end = self.texts.len();
                        }
                    } else if edit.range.from == last_edit.buffer_range.from
                        && last_edit.buffer_range.to <= edit.range.to
                    {
                        //
                    } else if last_edit.buffer_range.to == edit.range.to
                        && edit.range.from <= last_edit.buffer_range.from
                    {
                        //
                    }
                }
                _ => (),
            }
        }

        if append_edit {
            if let HistoryState::InsertGroup(range) = &mut self.state {
                range.end += 1;
            }

            let texts_range_start = self.texts.len();
            self.texts.push_str(edit.text);
            self.edits.push(EditInternal {
                kind: edit.kind,
                buffer_range: edit.range,
                texts_range: texts_range_start..self.texts.len(),
            });
        }
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
    use crate::buffer_position::BufferPosition;

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
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("b", edit.text);
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("a", edit.text);
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        let mut edit_iter = history.redo_edits();
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("a", edit.text);
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("b", edit.text);
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        assert_eq!(0, history.redo_edits().count());

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("b", edit.text);
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("a", edit.text);
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
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("c", edit.text);
        assert!(edit_iter.next().is_none());
        drop(edit_iter);

        assert_eq!(0, history.undo_edits().count());
    }

    #[test]
    fn compress_insert_insert_edits() {
        let mut history = History::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 3),
            ),
            text: "abc",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::between(
                BufferPosition::line_col(0, 3),
                BufferPosition::line_col(0, 6),
            ),
            text: "def",
        });

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("abcdef", edit.text);
        assert_eq!(
            BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 6)
            ),
            edit.range
        );
        assert!(edit_iter.next().is_none());

        let mut history = History::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 3),
            ),
            text: "abc",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 3),
            ),
            text: "def",
        });

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("defabc", edit.text);
        assert_eq!(
            BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 6)
            ),
            edit.range
        );
        assert!(edit_iter.next().is_none());
    }

    #[test]
    fn compress_delete_delete_edits() {
        let mut history = History::new();
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 3),
            ),
            text: "abc",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 3),
            ),
            text: "def",
        });

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("abcdef", edit.text);
        assert_eq!(
            BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 6)
            ),
            edit.range
        );
        assert!(edit_iter.next().is_none());

        let mut history = History::new();
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: BufferRange::between(
                BufferPosition::line_col(0, 3),
                BufferPosition::line_col(0, 6),
            ),
            text: "abc",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 3),
            ),
            text: "def",
        });

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("defabc", edit.text);
        assert_eq!(
            BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 6)
            ),
            edit.range
        );
        assert!(edit_iter.next().is_none());
    }

    fn compress_insert_delete_edits() {
        let mut history = History::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 6),
            ),
            text: "abcdef",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 3),
            ),
            text: "abc",
        });

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("def", edit.text);
        assert_eq!(
            BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 3)
            ),
            edit.range
        );
        assert!(edit_iter.next().is_none());

        let mut history = History::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 6),
            ),
            text: "abcdef",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: BufferRange::between(
                BufferPosition::line_col(0, 3),
                BufferPosition::line_col(0, 6),
            ),
            text: "def",
        });

        let mut edit_iter = history.undo_edits();
        let edit = edit_iter.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("abc", edit.text);
        assert_eq!(
            BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 3)
            ),
            edit.range
        );
        assert!(edit_iter.next().is_none());
    }
}
