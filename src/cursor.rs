use std::{
    ops::{Drop, Index, IndexMut},
    slice::SliceIndex,
};

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    script::ScriptValue,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub anchor: BufferPosition,
    pub position: BufferPosition,
}

impl Cursor {
    pub fn as_range(&self) -> BufferRange {
        BufferRange::between(self.anchor, self.position)
    }

    pub fn insert(&mut self, range: BufferRange) {
        self.anchor = self.anchor.insert(range);
        self.position = self.position.insert(range);
    }

    pub fn delete(&mut self, range: BufferRange) {
        self.anchor = self.anchor.delete(range);
        self.position = self.position.delete(range);
    }
}

impl_from_script!(Cursor, value => match &value {
    ScriptValue::Object(o) => {
        Some(Cursor {
            anchor: BufferPosition::line_col(o.get("anchor_line")?, o.get("anchor_column")?),
            position: BufferPosition::line_col(o.get("position_line")?, o.get("position_column")?),
        })
    }
    _ => None,
});
impl_to_script!(Cursor, (self, engine) => {
    let cursor = engine.create_object()?;
    cursor.set("anchor_line", self.anchor.line_index)?;
    cursor.set("anchor_column", self.anchor.column_byte_index)?;
    cursor.set("position_line", self.position.line_index)?;
    cursor.set("position_column", self.position.column_byte_index)?;
    ScriptValue::Object(cursor)
});

#[derive(Clone)]
pub struct CursorCollection {
    cursors: Vec<Cursor>,
    saved_column_byte_indices: Vec<usize>,
    main_cursor_index: usize,
}

impl CursorCollection {
    pub fn new() -> Self {
        Self {
            cursors: vec![Cursor::default()],
            saved_column_byte_indices: Vec::new(),
            main_cursor_index: 0,
        }
    }

    pub fn main_cursor_index(&self) -> usize {
        self.main_cursor_index
    }

    pub fn main_cursor(&self) -> &Cursor {
        &self.cursors[self.main_cursor_index]
    }

    pub fn mut_guard(&mut self) -> CursorCollectionMutGuard {
        CursorCollectionMutGuard {
            inner: self,
            clear_column_byte_indices: true,
        }
    }

    fn sort_and_merge(&mut self) {
        let main_cursor = self.cursors[self.main_cursor_index];
        self.cursors.sort_by_key(|c| c.as_range().from);
        self.main_cursor_index = self
            .cursors
            .binary_search_by_key(&main_cursor.position, |c| c.position)
            .unwrap_or(0);

        let mut i = 0;
        while i < self.cursors.len() {
            let mut range = self.cursors[i].as_range();
            for j in ((i + 1)..self.cursors.len()).rev() {
                let other_range = self.cursors[j].as_range();
                if range.from <= other_range.from && other_range.from <= range.to {
                    range.to = range.to.max(other_range.to);
                    self.cursors.remove(j);
                    if j <= self.main_cursor_index {
                        self.main_cursor_index -= 1;
                    }
                }
            }

            self.cursors[i] = if self.cursors[i].anchor <= self.cursors[i].position {
                Cursor {
                    anchor: range.from,
                    position: range.to,
                }
            } else {
                Cursor {
                    anchor: range.to,
                    position: range.from,
                }
            };

            i += 1;
        }
    }
}

impl<Idx> Index<Idx> for CursorCollection
where
    Idx: SliceIndex<[Cursor]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.cursors[index]
    }
}

pub struct CursorCollectionMutGuard<'a> {
    inner: &'a mut CursorCollection,
    clear_column_byte_indices: bool,
}

impl<'a> CursorCollectionMutGuard<'a> {
    pub fn clear(&mut self) {
        self.inner.cursors.clear();
    }

    pub fn add(&mut self, cursor: Cursor) {
        self.inner.main_cursor_index = self.inner.cursors.len();
        self.inner.cursors.push(cursor);
    }

    pub fn save_column_byte_indices(&mut self) {
        self.clear_column_byte_indices = false;

        if self.inner.saved_column_byte_indices.is_empty() {
            self.inner.saved_column_byte_indices.clear();
            for c in &self.inner.cursors {
                self.inner
                    .saved_column_byte_indices
                    .push(c.position.column_byte_index);
            }
        }
    }

    pub fn get_saved_column_byte_index(&self, index: usize) -> Option<usize> {
        self.inner.saved_column_byte_indices.get(index).cloned()
    }

    pub fn set_main_cursor_index(&mut self, index: usize) {
        self.inner.main_cursor_index = index;
    }

    pub fn main_cursor(&mut self) -> &mut Cursor {
        &mut self.inner.cursors[self.inner.main_cursor_index]
    }
}

impl<'a, Idx> Index<Idx> for CursorCollectionMutGuard<'a>
where
    Idx: SliceIndex<[Cursor]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        &self.inner.cursors[index]
    }
}

impl<'a, Idx> IndexMut<Idx> for CursorCollectionMutGuard<'a>
where
    Idx: SliceIndex<[Cursor]>,
{
    fn index_mut(&mut self, index: Idx) -> &mut Self::Output {
        &mut self.inner.cursors[index]
    }
}

impl<'a> Drop for CursorCollectionMutGuard<'a> {
    fn drop(&mut self) {
        if self.inner.cursors.len() == 0 {
            self.inner.cursors.push(Cursor::default());
        }

        self.inner.sort_and_merge();

        if self.clear_column_byte_indices {
            self.inner.saved_column_byte_indices.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_cursor() {
        let mut cursors = CursorCollection::new();
        assert_eq!(1, cursors[..].len());
        let main_cursor = *cursors.main_cursor();
        cursors.mut_guard().add(main_cursor);
        let mut cursors = cursors[..].iter();
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(0, 0), cursor.position);
        assert_eq!(BufferPosition::line_col(0, 0), cursor.anchor);
        assert!(cursors.next().is_none());

        let mut cursors = CursorCollection::new();
        let mut cursors_mut = cursors.mut_guard();
        cursors_mut[0].anchor = BufferPosition::line_col(2, 3);
        cursors_mut[0].position = cursors_mut[0].anchor;
        drop(cursors_mut);
        assert_eq!(1, cursors[..].len());
        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(2, 2),
            position: BufferPosition::line_col(2, 4),
        });
        let mut cursors = cursors[..].iter();
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(2, 2), cursor.anchor);
        assert_eq!(BufferPosition::line_col(2, 4), cursor.position);
        assert!(cursors.next().is_none());

        let mut cursors = CursorCollection::new();
        let mut cursors_mut = cursors.mut_guard();
        cursors_mut[0].anchor = BufferPosition::line_col(2, 2);
        cursors_mut[0].position = BufferPosition::line_col(2, 4);
        drop(cursors_mut);
        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(2, 2),
            position: BufferPosition::line_col(2, 2),
        });
        let mut cursors = cursors[..].iter();
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(2, 2), cursor.anchor);
        assert_eq!(BufferPosition::line_col(2, 4), cursor.position);
        assert!(cursors.next().is_none());

        let mut cursors = CursorCollection::new();
        let mut cursors_mut = cursors.mut_guard();
        cursors_mut[0].anchor = BufferPosition::line_col(2, 2);
        cursors_mut[0].position = BufferPosition::line_col(2, 3);
        drop(cursors_mut);
        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(2, 4),
            position: BufferPosition::line_col(2, 3),
        });
        let mut cursors = cursors[..].iter();
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(2, 2), cursor.anchor);
        assert_eq!(BufferPosition::line_col(2, 4), cursor.position);
        assert!(cursors.next().is_none());

        let mut cursors = CursorCollection::new();
        let mut cursors_mut = cursors.mut_guard();
        cursors_mut[0].anchor = BufferPosition::line_col(2, 4);
        cursors_mut[0].position = BufferPosition::line_col(2, 3);
        drop(cursors_mut);
        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(2, 3),
            position: BufferPosition::line_col(2, 2),
        });
        let mut cursors = cursors[..].iter();
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(2, 4), cursor.anchor);
        assert_eq!(BufferPosition::line_col(2, 2), cursor.position);
        assert!(cursors.next().is_none());
    }

    #[test]
    fn no_merge_cursor() {
        let mut cursors = CursorCollection::new();
        let mut cursors_mut = cursors.mut_guard();
        cursors_mut[0].anchor = BufferPosition::line_col(1, 0);
        cursors_mut[0].position = BufferPosition::line_col(1, 0);
        drop(cursors_mut);
        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(2, 0),
            position: BufferPosition::line_col(2, 0),
        });
        let mut cursors = cursors[..].iter();
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(1, 0), cursor.anchor);
        assert_eq!(BufferPosition::line_col(1, 0), cursor.position);
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(2, 0), cursor.anchor);
        assert_eq!(BufferPosition::line_col(2, 0), cursor.position);
        assert!(cursors.next().is_none());

        let mut cursors = CursorCollection::new();
        let mut cursors_mut = cursors.mut_guard();
        cursors_mut[0].anchor = BufferPosition::line_col(3, 2);
        cursors_mut[0].position = BufferPosition::line_col(3, 2);
        drop(cursors_mut);
        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(2, 2),
            position: BufferPosition::line_col(2, 2),
        });
        let mut cursors = cursors[..].iter();
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(2, 2), cursor.anchor);
        assert_eq!(BufferPosition::line_col(2, 2), cursor.position);
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(3, 2), cursor.anchor);
        assert_eq!(BufferPosition::line_col(3, 2), cursor.position);
        assert!(cursors.next().is_none());
    }

    #[test]
    fn move_and_merge_cursors() {
        let mut cursors = CursorCollection::new();
        let mut cursors_mut = cursors.mut_guard();
        cursors_mut[0].anchor = BufferPosition::line_col(0, 0);
        cursors_mut[0].position = BufferPosition::line_col(0, 0);
        drop(cursors_mut);
        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(1, 0),
            position: BufferPosition::line_col(1, 0),
        });
        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(2, 0),
            position: BufferPosition::line_col(2, 0),
        });
        assert_eq!(3, cursors[..].len());
        let mut cursors_mut = cursors.mut_guard();
        for c in &mut cursors_mut[..] {
            if c.position.line_index > 0 {
                c.position.line_index -= 1;
            }
            c.anchor = c.position;
        }
        drop(cursors_mut);
        let cursor = cursors.main_cursor();
        assert_eq!(BufferPosition::line_col(1, 0), cursor.anchor);
        assert_eq!(BufferPosition::line_col(1, 0), cursor.position);
        let mut cursors = cursors[..].iter();
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(0, 0), cursor.anchor);
        assert_eq!(BufferPosition::line_col(0, 0), cursor.position);
        let cursor = cursors.next().unwrap();
        assert_eq!(BufferPosition::line_col(1, 0), cursor.anchor);
        assert_eq!(BufferPosition::line_col(1, 0), cursor.position);
        assert!(cursors.next().is_none());
    }
}
