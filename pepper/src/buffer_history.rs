use std::ops::Range;

use crate::buffer_position::{BufferPosition, BufferRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Insert,
    Delete,
}

#[derive(Debug, Clone, Copy)]
pub struct Edit<'a> {
    pub kind: EditKind,
    pub range: BufferRange,
    pub text: &'a str,
}

#[derive(Debug)]
pub(crate) struct EditInternal {
    pub kind: EditKind,
    pub buffer_range: BufferRange,
    pub text_range: Range<u32>,
}

impl EditInternal {
    pub fn text_range(&self) -> Range<usize> {
        self.text_range.start as usize..self.text_range.end as usize
    }

    pub fn as_edit_ref<'a>(&self, texts: &'a str) -> Edit<'a> {
        Edit {
            kind: self.kind,
            range: self.buffer_range,
            text: &texts[self.text_range()],
        }
    }
}

enum HistoryState {
    IterIndex { group_index: usize },
    InsertGroup { edit_index: usize },
}

pub(crate) struct BufferHistory {
    texts: String,
    edits: Vec<EditInternal>,
    group_ranges: Vec<Range<usize>>,
    state: HistoryState,
}

impl BufferHistory {
    pub fn new() -> Self {
        Self {
            texts: String::new(),
            edits: Vec::new(),
            group_ranges: Vec::new(),
            state: HistoryState::IterIndex { group_index: 0 },
        }
    }

    pub fn clear(&mut self) {
        self.texts.clear();
        self.edits.clear();
        self.group_ranges.clear();
        self.state = HistoryState::IterIndex { group_index: 0 };
    }

    pub fn add_edit(&mut self, edit: Edit) {
        let current_group_start = match self.state {
            HistoryState::IterIndex { group_index } => {
                let edit_index = match self.group_ranges.get(group_index) {
                    Some(range) => range.start,
                    None => self.edits.len(),
                };
                self.edits.truncate(edit_index);
                match self.edits.last() {
                    Some(edit) => self.texts.truncate(edit.text_range.end as _),
                    None => self.texts.clear(),
                }
                self.state = HistoryState::InsertGroup { edit_index };
                self.group_ranges.truncate(group_index);
                edit_index
            }
            HistoryState::InsertGroup { edit_index } => edit_index,
        };

        let merged = self.try_merge_edit(current_group_start, &edit);
        if merged {
            return;
        }

        let texts_range_start = self.texts.len();
        self.texts.push_str(edit.text);
        let edit = EditInternal {
            kind: edit.kind,
            buffer_range: edit.range,
            text_range: texts_range_start as u32..self.texts.len() as u32,
        };
        self.edits.push(edit);
    }

    #[inline(never)]
    fn try_merge_edit(&mut self, current_group_start: usize, edit: &Edit) -> bool {
        fn fix_other_edits(
            group_edits: &mut [EditInternal],
            current_index: usize,
            edit_range: BufferRange,
            fix_position_fn: fn(BufferPosition, BufferRange) -> BufferPosition,
            fix_text_start: usize,
            fix_text_len: isize,
        ) {
            fn fix_text_range(edit: &mut EditInternal, start: usize, len: isize) {
                let pivot = if len >= 0 {
                    start
                } else {
                    start + (-len) as usize
                };
                if pivot <= edit.text_range.start as usize {
                    edit.text_range.start = (edit.text_range.start as isize + len) as _;
                    edit.text_range.end = (edit.text_range.end as isize + len) as _;
                } else if pivot < edit.text_range.end as usize {
                    edit.text_range.end = (edit.text_range.end as isize + len) as _;
                }
            }

            for edit in &mut group_edits[..current_index] {
                fix_text_range(edit, fix_text_start, fix_text_len);
            }
            for edit in &mut group_edits[current_index + 1..] {
                if edit_range.from < edit.buffer_range.from {
                    edit.buffer_range.from = fix_position_fn(edit.buffer_range.from, edit_range);
                    edit.buffer_range.to = fix_position_fn(edit.buffer_range.to, edit_range);
                }
                fix_text_range(edit, fix_text_start, fix_text_len);
            }
        }

        let group_edits = &mut self.edits[current_group_start..];
        let mut edit_range = edit.range;
        let edit_text_len = edit.text.len();
        for (i, other_edit) in group_edits.iter_mut().enumerate().rev() {
            match (other_edit.kind, edit.kind) {
                (EditKind::Insert, EditKind::Insert) => {
                    // -- insert --
                    //             -- insert -- (new)
                    if edit_range.from == other_edit.buffer_range.to {
                        let fix_text_start = other_edit.text_range.end as _;
                        self.texts.insert_str(fix_text_start, edit.text);
                        other_edit.buffer_range.to = edit_range.to;
                        other_edit.text_range.end += edit_text_len as u32;

                        fix_other_edits(
                            group_edits,
                            i,
                            edit_range,
                            BufferPosition::insert,
                            fix_text_start,
                            edit_text_len as _,
                        );
                        return true;

                    // -- insert --
                    // -- insert -- (new)
                    } else if edit_range.from == other_edit.buffer_range.from {
                        let fix_text_start = other_edit.text_range.start as _;
                        self.texts.insert_str(fix_text_start, edit.text);
                        other_edit.buffer_range.to = other_edit.buffer_range.to.insert(edit_range);
                        other_edit.text_range.end += edit_text_len as u32;

                        fix_other_edits(
                            group_edits,
                            i,
                            edit_range,
                            BufferPosition::insert,
                            fix_text_start,
                            edit_text_len as _,
                        );
                        return true;
                    }

                    edit_range.from = edit_range.from.delete(other_edit.buffer_range);
                    edit_range.to = edit_range.to.delete(other_edit.buffer_range);
                }
                (EditKind::Delete, EditKind::Delete) => {
                    // -- delete --
                    // -- delete -- (new)
                    if edit_range.from == other_edit.buffer_range.from {
                        let fix_text_start = other_edit.text_range.end as _;
                        self.texts.insert_str(fix_text_start, edit.text);
                        other_edit.buffer_range.to = other_edit.buffer_range.to.insert(edit_range);
                        other_edit.text_range.end += edit_text_len as u32;

                        fix_other_edits(
                            group_edits,
                            i,
                            edit_range,
                            BufferPosition::delete,
                            fix_text_start,
                            edit_text_len as _,
                        );
                        return true;

                    //             -- delete --
                    // -- delete -- (new)
                    } else if edit_range.to == other_edit.buffer_range.from {
                        let fix_text_start = other_edit.text_range.start as _;
                        self.texts.insert_str(fix_text_start, edit.text);
                        other_edit.buffer_range.from = edit_range.from;
                        other_edit.text_range.end += edit_text_len as u32;

                        fix_other_edits(
                            group_edits,
                            i,
                            edit_range,
                            BufferPosition::delete,
                            fix_text_start,
                            edit_text_len as _,
                        );
                        return true;
                    }

                    edit_range.from = edit_range.from.insert(other_edit.buffer_range);
                    edit_range.to = edit_range.to.insert(other_edit.buffer_range);
                }
                (EditKind::Insert, EditKind::Delete) => {
                    // -- insert ------
                    // -- delete ------ (new)
                    if other_edit.buffer_range == edit_range {
                        let other_text_range = other_edit.text_range();
                        if edit.text == &self.texts[other_text_range.clone()] {
                            let fix_text_start = other_edit.text_range.start as _;
                            self.texts.drain(other_text_range);

                            fix_other_edits(
                                group_edits,
                                i,
                                edit_range,
                                BufferPosition::delete,
                                fix_text_start,
                                -(edit_text_len as isize),
                            );

                            self.edits.remove(current_group_start + i);
                            return true;
                        }

                    // -- insert ------
                    // -- delete -- (new)
                    } else if other_edit.buffer_range.from == edit_range.from
                        && edit_range.to < other_edit.buffer_range.to
                    {
                        let fix_text_start = other_edit.text_range.start as usize;
                        let deleted_text_range = fix_text_start..(fix_text_start + edit_text_len);
                        if edit.text == &self.texts[deleted_text_range.clone()] {
                            self.texts.drain(deleted_text_range);
                            other_edit.buffer_range.to =
                                other_edit.buffer_range.to.delete(edit_range);
                            other_edit.text_range.end -= edit_text_len as u32;

                            fix_other_edits(
                                group_edits,
                                i,
                                edit_range,
                                BufferPosition::delete,
                                fix_text_start,
                                -(edit_text_len as isize),
                            );
                            return true;
                        }

                    // ------ insert --
                    //     -- delete -- (new)
                    } else if edit_range.to == other_edit.buffer_range.to
                        && other_edit.buffer_range.from < edit_range.from
                    {
                        let fix_text_start = other_edit.text_range.end as usize - edit_text_len;
                        let deleted_text_range = fix_text_start..other_edit.text_range.end as _;
                        if edit.text == &self.texts[deleted_text_range.clone()] {
                            self.texts.drain(deleted_text_range);
                            other_edit.buffer_range.to = edit_range.from;
                            other_edit.text_range.end -= edit_text_len as u32;

                            fix_other_edits(
                                group_edits,
                                i,
                                edit_range,
                                BufferPosition::delete,
                                fix_text_start,
                                -(edit_text_len as isize),
                            );
                            return true;
                        }

                    // -- insert --
                    // -- delete ------ (new)
                    } else if edit_range.from == other_edit.buffer_range.from
                        && other_edit.buffer_range.to < edit_range.to
                    {
                        let other_text_range = other_edit.text_range();
                        let fix_text_start = other_text_range.start;
                        let previous_other_text_end = other_text_range.end;
                        let other_text_len = previous_other_text_end - fix_text_start;
                        if edit.text[..other_text_len] == self.texts[other_text_range.clone()] {
                            self.texts
                                .replace_range(other_text_range, &edit.text[other_text_len..]);
                            other_edit.kind = EditKind::Delete;
                            other_edit.buffer_range.to =
                                edit_range.to.delete(other_edit.buffer_range);
                            other_edit.text_range.end =
                                (fix_text_start + edit_text_len - other_text_len) as _;
                            let other_text_end_diff = other_edit.text_range.end as isize
                                - previous_other_text_end as isize;

                            fix_other_edits(
                                group_edits,
                                i,
                                edit_range,
                                BufferPosition::delete,
                                fix_text_start,
                                other_text_end_diff,
                            );
                            return true;
                        }

                    //     -- insert --
                    // ------ delete -- (new)
                    } else if other_edit.buffer_range.to == edit_range.to
                        && edit_range.from < other_edit.buffer_range.from
                    {
                        let other_text_range = other_edit.text_range();
                        let fix_text_start = other_text_range.start;
                        let previous_other_text_end = other_text_range.end;
                        let other_text_len = previous_other_text_end - fix_text_start;
                        let text_len_diff = edit_text_len - other_text_len;
                        if edit.text[text_len_diff..] == self.texts[other_text_range.clone()] {
                            self.texts
                                .replace_range(other_text_range, &edit.text[..text_len_diff]);
                            other_edit.kind = EditKind::Delete;
                            other_edit.buffer_range.to = other_edit.buffer_range.from;
                            other_edit.buffer_range.from = edit_range.from;
                            other_edit.text_range.end = (fix_text_start + text_len_diff) as _;
                            let other_text_end_diff = other_edit.text_range.end as isize
                                - previous_other_text_end as isize;

                            fix_other_edits(
                                group_edits,
                                i,
                                edit_range,
                                BufferPosition::delete,
                                fix_text_start,
                                other_text_end_diff,
                            );
                            return true;
                        }
                    }

                    edit_range.from = edit_range.from.delete(other_edit.buffer_range);
                    edit_range.to = edit_range.to.delete(other_edit.buffer_range);
                }
                (EditKind::Delete, EditKind::Insert) => break,
            }
        }

        false
    }

    pub fn commit_edits(&mut self) {
        if let HistoryState::InsertGroup { edit_index } = self.state {
            self.group_ranges.push(edit_index..self.edits.len());
            self.state = HistoryState::IterIndex {
                group_index: self.group_ranges.len(),
            };
        }
    }

    pub fn undo_edits(
        &mut self,
    ) -> impl Clone + ExactSizeIterator<Item = Edit> + DoubleEndedIterator<Item = Edit> {
        self.commit_edits();

        let range = match &mut self.state {
            HistoryState::IterIndex { group_index } => {
                if *group_index > 0 {
                    *group_index -= 1;
                    self.group_ranges[*group_index].clone()
                } else {
                    0..0
                }
            }
            _ => unreachable!(),
        };

        let texts = &self.texts;
        self.edits[range].iter().rev().map(move |e| {
            let mut edit = e.as_edit_ref(texts);
            edit.kind = match edit.kind {
                EditKind::Insert => EditKind::Delete,
                EditKind::Delete => EditKind::Insert,
            };
            edit
        })
    }

    pub fn redo_edits(
        &mut self,
    ) -> impl Clone + ExactSizeIterator<Item = Edit> + DoubleEndedIterator<Item = Edit> {
        self.commit_edits();

        let range = match &mut self.state {
            HistoryState::IterIndex { group_index } => {
                if *group_index < self.group_ranges.len() {
                    let range = self.group_ranges[*group_index].clone();
                    *group_index += 1;
                    range
                } else {
                    0..0
                }
            }
            _ => unreachable!(),
        };

        let texts = &self.texts;
        self.edits[range].iter().map(move |e| e.as_edit_ref(texts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer_position::BufferPosition;

    fn buffer_range(from: (usize, usize), to: (usize, usize)) -> BufferRange {
        BufferRange::between(
            BufferPosition::line_col(from.0 as _, from.1 as _),
            BufferPosition::line_col(to.0 as _, to.1 as _),
        )
    }

    #[test]
    fn commit_edits_on_emtpy_history() {
        let mut history = BufferHistory::new();
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
        let mut history = BufferHistory::new();

        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((1, 0), (1, 1)),
            text: "b",
        });

        assert_eq!(0, history.redo_edits().count());

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("b", edit.text);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("a", edit.text);
        assert!(edits.next().is_none());
        drop(edits);

        let mut edits = history.redo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("a", edit.text);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("b", edit.text);
        assert!(edits.next().is_none());
        drop(edits);

        assert_eq!(0, history.redo_edits().count());

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("b", edit.text);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("a", edit.text);
        assert!(edits.next().is_none());
        drop(edits);

        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((2, 0), (2, 1)),
            text: "c",
        });

        assert_eq!(0, history.redo_edits().count());

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("c", edit.text);
        assert!(edits.next().is_none());
        drop(edits);

        assert_eq!(0, history.undo_edits().count());
    }

    #[test]
    fn compress_insert_insert_edits() {
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 3)),
            text: "abc",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 3), (0, 6)),
            text: "def",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("abcdef", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 6)), edit.range);
        assert!(edits.next().is_none());

        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 3)),
            text: "abc",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 3)),
            text: "def",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("defabc", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 6)), edit.range);
        assert!(edits.next().is_none());
    }

    #[test]
    fn compress_delete_delete_edits() {
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 3)),
            text: "abc",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 3)),
            text: "def",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("abcdef", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 6)), edit.range);
        assert!(edits.next().is_none());

        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 3), (0, 6)),
            text: "abc",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 3)),
            text: "def",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("defabc", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 6)), edit.range);
        assert!(edits.next().is_none());
    }

    #[test]
    fn compress_insert_delete_edits() {
        // -- insert ------
        // -- delete --
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 6)),
            text: "abcdef",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 3)),
            text: "abc",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("def", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 3)), edit.range);
        assert!(edits.next().is_none());

        // ------ insert --
        //     -- delete --
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 6)),
            text: "abcdef",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 3), (0, 6)),
            text: "def",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("abc", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 3)), edit.range);
        assert!(edits.next().is_none());

        // -- insert --
        // -- delete ------
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 3)),
            text: "abc",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 6)),
            text: "abcdef",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("def", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 3)), edit.range);
        assert!(edits.next().is_none());

        //     -- insert --
        // ------ delete --
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 3), (0, 6)),
            text: "def",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 6)),
            text: "abcdef",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("abc", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 3)), edit.range);
        assert!(edits.next().is_none());
    }

    #[test]
    fn compress_multiple_insert_insert_edits() {
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((1, 0), (1, 1)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((1, 1), (1, 2)),
            text: "b",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 1), (0, 2)),
            text: "b",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("ab", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 2)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("ab", edit.text);
        assert_eq!(buffer_range((1, 0), (1, 2)), edit.range);
        assert!(edits.next().is_none());

        //

        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 1), (0, 2)),
            text: "c",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 3), (0, 4)),
            text: "d",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 1), (0, 2)),
            text: "b",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("ab", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 2)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("cd", edit.text);
        assert_eq!(buffer_range((0, 1), (0, 3)), edit.range);
        assert!(edits.next().is_none());

        //

        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 4), (0, 5)),
            text: "d",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 1)),
            text: "b",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 5), (0, 6)),
            text: "c",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("ab", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 2)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("cd", edit.text);
        assert_eq!(buffer_range((0, 4), (0, 6)), edit.range);
        assert!(edits.next().is_none());
    }

    #[test]
    fn compress_multiple_delete_delete_edits() {
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((1, 0), (1, 1)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((1, 0), (1, 1)),
            text: "b",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 1)),
            text: "b",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("ab", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 2)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("ab", edit.text);
        assert_eq!(buffer_range((1, 0), (1, 2)), edit.range);
        assert!(edits.next().is_none());

        //

        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 2), (0, 3)),
            text: "c",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 1), (0, 2)),
            text: "d",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 1)),
            text: "b",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("ab", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 2)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("cd", edit.text);
        assert_eq!(buffer_range((0, 2), (0, 4)), edit.range);
        assert!(edits.next().is_none());

        //

        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 4), (0, 5)),
            text: "d",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 1), (0, 2)),
            text: "b",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 2), (0, 3)),
            text: "c",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("ab", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 2)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("cd", edit.text);
        assert_eq!(buffer_range((0, 3), (0, 5)), edit.range);
        assert!(edits.next().is_none());
    }

    #[test]
    fn compress_multiple_insert_delete_edits() {
        // -- insert --
        // -- delete --
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 1), (0, 3)),
            text: "cd",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 2)),
            text: "ab",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 3), (0, 5)),
            text: "cd",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 2)),
            text: "ab",
        });

        let mut edits = history.undo_edits();
        assert!(edits.next().is_none());

        // -- insert ------
        // -- delete --
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 1), (0, 3)),
            text: "cd",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 2)),
            text: "ab",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 3), (0, 4)),
            text: "c",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("b", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 1)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("d", edit.text);
        assert_eq!(buffer_range((0, 1), (0, 2)), edit.range);
        assert!(edits.next().is_none());

        // ------ insert --
        //     -- delete --
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 1), (0, 3)),
            text: "cd",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 2)),
            text: "ab",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 4), (0, 5)),
            text: "d",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 1), (0, 2)),
            text: "b",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("a", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 1)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("c", edit.text);
        assert_eq!(buffer_range((0, 1), (0, 2)), edit.range);
        assert!(edits.next().is_none());

        // -- insert --
        // -- delete ------
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 2), (0, 3)),
            text: "c",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 3), (0, 5)),
            text: "cd",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 2)),
            text: "ab",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("b", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 1)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("d", edit.text);
        assert_eq!(buffer_range((0, 2), (0, 3)), edit.range);
        assert!(edits.next().is_none());

        //     -- insert --
        // ------ delete --
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 3), (0, 4)),
            text: "d",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 1), (0, 2)),
            text: "b",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 3), (0, 5)),
            text: "cd",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 0), (0, 2)),
            text: "ab",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("a", edit.text);
        assert_eq!(buffer_range((0, 0), (0, 1)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("c", edit.text);
        assert_eq!(buffer_range((0, 2), (0, 3)), edit.range);
        assert!(edits.next().is_none());
    }

    #[test]
    fn no_compress_insert_delete_insert_edits() {
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 0), (0, 1)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 1), (1, 0)),
            text: "\n",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 1), (0, 2)),
            text: "ab",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!(buffer_range((0, 1), (0, 2)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!(buffer_range((0, 1), (1, 0)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!(buffer_range((0, 0), (0, 1)), edit.range);
        assert!(edits.next().is_none());
    }

    #[test]
    fn insert_multiple_deletes_insert() {
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((1, 4), (1, 5)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 4), (0, 5)),
            text: "a",
        });

        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((1, 4), (1, 5)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 4), (0, 5)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((1, 3), (1, 4)),
            text: "x",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 3), (0, 4)),
            text: "x",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((1, 2), (1, 3)),
            text: "x",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 2), (0, 3)),
            text: "x",
        });

        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((1, 2), (1, 3)),
            text: "b",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 2), (0, 3)),
            text: "b",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("b", edit.text);
        assert_eq!(buffer_range((0, 2), (0, 3)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("b", edit.text);
        assert_eq!(buffer_range((1, 2), (1, 3)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("xx", edit.text);
        assert_eq!(buffer_range((0, 2), (0, 4)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("xx", edit.text);
        assert_eq!(buffer_range((1, 2), (1, 4)), edit.range);
        assert!(edits.next().is_none());
    }

    #[test]
    fn insert_delete_insert() {
        let mut history = BufferHistory::new();
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((1, 4), (1, 5)),
            text: "a",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 4), (0, 5)),
            text: "a",
        });

        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((1, 2), (1, 5)),
            text: "xxa",
        });
        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: buffer_range((0, 2), (0, 5)),
            text: "xxa",
        });

        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((1, 2), (1, 3)),
            text: "b",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: buffer_range((0, 2), (0, 3)),
            text: "b",
        });

        let mut edits = history.undo_edits();
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("b", edit.text);
        assert_eq!(buffer_range((0, 2), (0, 3)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Delete, edit.kind);
        assert_eq!("b", edit.text);
        assert_eq!(buffer_range((1, 2), (1, 3)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("xx", edit.text);
        assert_eq!(buffer_range((0, 2), (0, 4)), edit.range);
        let edit = edits.next().unwrap();
        assert_eq!(EditKind::Insert, edit.kind);
        assert_eq!("xx", edit.text);
        assert_eq!(buffer_range((1, 2), (1, 4)), edit.range);
        assert!(edits.next().is_none());
    }

    #[test]
    fn delete_insert_twice() {
        let range01to02 = buffer_range((0, 1), (0, 2));
        let mut history = BufferHistory::new();

        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: range01to02,
            text: "b",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: range01to02,
            text: "1",
        });
        history.commit_edits();

        history.add_edit(Edit {
            kind: EditKind::Delete,
            range: range01to02,
            text: "1",
        });
        history.add_edit(Edit {
            kind: EditKind::Insert,
            range: range01to02,
            text: "2",
        });
        history.commit_edits();

        {
            let mut edits = history.undo_edits();
            let edit1 = edits.next().unwrap();
            let edit2 = edits.next().unwrap();
            assert_eq!(EditKind::Delete, edit1.kind);
            assert_eq!("2", edit1.text);
            assert_eq!(range01to02, edit1.range);
            assert_eq!(EditKind::Insert, edit2.kind);
            assert_eq!("1", edit2.text);
            assert_eq!(range01to02, edit2.range);
            assert!(edits.next().is_none());
        }

        {
            let mut edits = history.undo_edits();
            let edit1 = edits.next().unwrap();
            let edit2 = edits.next().unwrap();
            assert_eq!(EditKind::Delete, edit1.kind);
            assert_eq!("1", edit1.text);
            assert_eq!(range01to02, edit1.range);
            assert_eq!(EditKind::Insert, edit2.kind);
            assert_eq!("b", edit2.text);
            assert_eq!(range01to02, edit2.range);
            assert!(edits.next().is_none());
        }
    }
}
