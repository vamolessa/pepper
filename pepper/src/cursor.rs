use std::{
    fmt,
    ops::{Drop, Index, IndexMut, RangeFrom, RangeFull},
};

use crate::{
    buffer::{BufferContent, CharDisplayDistances},
    buffer_position::{BufferPosition, BufferRange},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub anchor: BufferPosition,
    pub position: BufferPosition,
}

impl Cursor {
    pub const fn zero() -> Self {
        Self {
            anchor: BufferPosition::zero(),
            position: BufferPosition::zero(),
        }
    }

    pub fn to_range(self) -> BufferRange {
        BufferRange::between(self.anchor, self.position)
    }

    pub fn to_range_and_direction(self) -> (BufferRange, bool) {
        BufferRange::between_with_direction(self.anchor, self.position)
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

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.anchor, self.position)
    }
}

#[derive(Clone)]
pub struct CursorCollection {
    cursors: Vec<Cursor>,
    saved_display_distances: Vec<u32>,
    main_cursor_index: u32,
}

impl CursorCollection {
    pub fn new() -> Self {
        Self {
            cursors: vec![Cursor::zero()],
            saved_display_distances: vec![0],
            main_cursor_index: 0,
        }
    }

    pub fn main_cursor_index(&self) -> usize {
        self.main_cursor_index as _
    }

    pub fn main_cursor(&self) -> &Cursor {
        &self.cursors[self.main_cursor_index as usize]
    }

    pub fn mut_guard(&mut self) -> CursorCollectionMutGuard {
        CursorCollectionMutGuard {
            inner: self,
            clear_display_distances: true,
            set_main_cursor_near_position: None,
        }
    }

    fn sort_cursors(&mut self, main_cursor_position: BufferPosition) {
        self.cursors.sort_unstable_by_key(|c| c.to_range().from);
        self.main_cursor_index = match self
            .cursors
            .binary_search_by_key(&main_cursor_position, |c| c.position)
        {
            Ok(i) => i as _,
            Err(i) => i.min(self.cursors.len() - 1) as _,
        };
    }

    fn merge_sorted_cursors(&mut self) {
        fn to_cursor(range: BufferRange, forward: bool) -> Cursor {
            if forward {
                Cursor {
                    anchor: range.from,
                    position: range.to,
                }
            } else {
                Cursor {
                    anchor: range.to,
                    position: range.from,
                }
            }
        }

        let ptr_range = self.cursors.as_mut_ptr_range();
        let start_ptr = ptr_range.start;
        let end_ptr = ptr_range.end;

        let mut write_ptr = start_ptr;
        let mut read_ptr = unsafe { write_ptr.add(1) };
        let mut write_i = 0;

        let (mut range, mut forward) = unsafe { write_ptr.read() }.to_range_and_direction();
        while read_ptr != end_ptr {
            let other_cursor = unsafe { read_ptr.read() };
            let (other_range, other_forward) = other_cursor.to_range_and_direction();

            if other_range.from <= range.to {
                if write_i < self.main_cursor_index as _ {
                    self.main_cursor_index -= 1;
                }

                range.to = range.to.max(other_range.to);
            } else {
                let cursor = to_cursor(range, forward);
                let store_ptr = write_ptr;
                write_i += 1;

                range = other_range;
                forward = other_forward;

                unsafe { write_ptr = write_ptr.add(1) };
                unsafe { store_ptr.write(cursor) };
            }

            unsafe { read_ptr = read_ptr.add(1) };
        }

        let cursor = to_cursor(range, forward);
        unsafe { write_ptr.write(cursor) };

        let len = unsafe { write_ptr.add(1).offset_from(start_ptr) as _ };
        self.cursors.truncate(len);
    }
}

impl Index<usize> for CursorCollection {
    type Output = Cursor;
    fn index(&self, index: usize) -> &Self::Output {
        &self.cursors[index as usize]
    }
}
impl Index<RangeFull> for CursorCollection {
    type Output = [Cursor];
    fn index(&self, _: RangeFull) -> &Self::Output {
        &self.cursors
    }
}
impl Index<RangeFrom<usize>> for CursorCollection {
    type Output = [Cursor];
    fn index(&self, index: RangeFrom<usize>) -> &Self::Output {
        &self.cursors[index.start..]
    }
}

pub struct CursorCollectionMutGuard<'a> {
    inner: &'a mut CursorCollection,
    clear_display_distances: bool,
    set_main_cursor_near_position: Option<BufferPosition>,
}

impl<'a> CursorCollectionMutGuard<'a> {
    pub fn clear(&mut self) {
        self.inner.cursors.clear();
    }

    pub fn add(&mut self, cursor: Cursor) {
        self.inner.main_cursor_index = self.inner.cursors.len() as _;
        self.inner.cursors.push(cursor);
    }

    pub fn swap_remove(&mut self, index: usize) -> Cursor {
        self.inner.main_cursor_index = 0;
        self.inner.cursors.swap_remove(index)
    }

    pub fn save_display_distances(&mut self, buffer: &BufferContent, tab_size: u8) {
        self.clear_display_distances = false;
        if self.inner.saved_display_distances.is_empty() {
            for c in &self.inner.cursors {
                let line = &buffer.lines()[c.position.line_index as usize].as_str()
                    [..c.position.column_byte_index as usize];
                let distance = CharDisplayDistances::new(line, tab_size)
                    .last()
                    .map(|d| d.distance)
                    .unwrap_or(0);

                self.inner.saved_display_distances.push(distance);
            }
        }
    }

    pub fn get_saved_display_distance(&self, index: usize) -> Option<u32> {
        self.inner.saved_display_distances.get(index).cloned()
    }

    pub fn main_cursor_index(&mut self) -> usize {
        self.inner.main_cursor_index as usize
    }

    pub fn set_main_cursor_index(&mut self, index: usize) {
        self.inner.main_cursor_index = self.inner.cursors.len().saturating_sub(1).min(index) as _;
    }

    pub fn main_cursor(&mut self) -> &mut Cursor {
        &mut self.inner.cursors[self.inner.main_cursor_index as usize]
    }

    pub fn set_main_cursor_near_position(&mut self, position: BufferPosition) {
        self.set_main_cursor_near_position = Some(position);
    }
}

impl<'a> Index<usize> for CursorCollectionMutGuard<'a> {
    type Output = Cursor;
    fn index(&self, index: usize) -> &Self::Output {
        &self.inner.cursors[index as usize]
    }
}
impl<'a> Index<RangeFull> for CursorCollectionMutGuard<'a> {
    type Output = [Cursor];
    fn index(&self, _: RangeFull) -> &Self::Output {
        &self.inner.cursors
    }
}
impl<'a> Index<RangeFrom<usize>> for CursorCollectionMutGuard<'a> {
    type Output = [Cursor];
    fn index(&self, index: RangeFrom<usize>) -> &Self::Output {
        &self.inner.cursors[index.start..]
    }
}

impl<'a> IndexMut<usize> for CursorCollectionMutGuard<'a> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.inner.cursors[index as usize]
    }
}
impl<'a> IndexMut<RangeFull> for CursorCollectionMutGuard<'a> {
    fn index_mut(&mut self, _: RangeFull) -> &mut Self::Output {
        &mut self.inner.cursors
    }
}
impl<'a> IndexMut<RangeFrom<usize>> for CursorCollectionMutGuard<'a> {
    fn index_mut(&mut self, index: RangeFrom<usize>) -> &mut Self::Output {
        &mut self.inner.cursors[index.start..]
    }
}

impl<'a> Drop for CursorCollectionMutGuard<'a> {
    fn drop(&mut self) {
        if self.inner.cursors.is_empty() {
            self.inner.cursors.push(Cursor::zero());
            self.inner.main_cursor_index = 0;
        }

        let main_cursor_position = match self.set_main_cursor_near_position {
            Some(position) => position,
            None => self.inner.cursors[self.inner.main_cursor_index as usize].position,
        };
        self.inner.sort_cursors(main_cursor_position);
        self.inner.merge_sorted_cursors();

        if self.clear_display_distances {
            self.inner.saved_display_distances.clear();
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
        assert_eq!(BufferPosition::zero(), cursor.position);
        assert_eq!(BufferPosition::zero(), cursor.anchor);
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

        cursors.mut_guard()[0] = Cursor {
            anchor: BufferPosition::line_col(0, 0),
            position: BufferPosition::line_col(0, 0),
        };
        assert_eq!(1, cursors[..].len());
        assert_eq!(0, cursors.main_cursor_index());

        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(1, 0),
            position: BufferPosition::line_col(1, 0),
        });
        assert_eq!(2, cursors[..].len());
        assert_eq!(1, cursors.main_cursor_index());

        cursors.mut_guard().add(Cursor {
            anchor: BufferPosition::line_col(2, 0),
            position: BufferPosition::line_col(2, 0),
        });
        assert_eq!(3, cursors[..].len());
        assert_eq!(2, cursors.main_cursor_index());

        {
            let mut cursors = cursors.mut_guard();
            for c in &mut cursors[..] {
                if c.position.line_index > 0 {
                    c.position.line_index -= 1;
                }
                c.anchor = c.position;
            }
        }
        assert_eq!(2, cursors[..].len());
        assert_eq!(1, cursors.main_cursor_index());

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
