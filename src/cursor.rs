use crate::buffer_position::{BufferPosition, BufferRange};

#[derive(Debug, Default, Clone, Copy)]
pub struct Cursor {
    pub position: BufferPosition,
    pub anchor: BufferPosition,
}

impl Cursor {
    pub fn range(&self) -> BufferRange {
        BufferRange::between(self.anchor, self.position)
    }

    pub fn insert(&mut self, range: BufferRange) {
        self.position = self.position.insert(range);
        self.anchor = self.anchor.insert(range);
    }

    pub fn remove(&mut self, range: BufferRange) {
        self.position = self.position.remove(range);
        self.anchor = self.anchor.remove(range);
    }
}

pub struct CursorCollection {
    cursors: Vec<Cursor>,
    main_cursor_index: usize,
}

impl CursorCollection {
    pub fn new() -> Self {
        Self {
            cursors: vec![Cursor::default()],
            main_cursor_index: 0,
        }
    }

    pub fn main_cursor(&self) -> &Cursor {
        &self.cursors[self.main_cursor_index]
    }

    pub fn add_cursor(&mut self, cursor: Cursor) {
        self.main_cursor_index = self.cursors.len();
        self.cursors.push(cursor);
        self.sort_and_merge();
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Cursor> {
        self.cursors.iter()
    }

    pub fn collapse_anchors(&mut self) {
        for cursor in &mut self.cursors {
            cursor.anchor = cursor.position;
        }
    }

    pub fn swap_positions_and_anchors(&mut self) {
        for cursor in &mut self.cursors {
            std::mem::swap(&mut cursor.anchor, &mut cursor.position);
        }
    }

    pub fn change_all<F>(&mut self, callback: F)
    where
        F: FnOnce(&mut [Cursor]),
    {
        callback(&mut self.cursors[..]);
        self.sort_and_merge();
    }

    fn sort_and_merge(&mut self) {
        {
            let mut i = 0;
            while i < self.cursors.len() {
                let mut range = self.cursors[i].range();
                for j in (0..self.cursors.len()).rev() {
                    if j == i {
                        continue;
                    }

                    let other_range = self.cursors[j].range();

                    if range.contains(other_range.from) {
                        range.to = range.to.max(other_range.to);
                    } else if range.contains(other_range.to) {
                        range.from = range.from.max(other_range.from);
                    } else {
                        continue;
                    }

                    if self.main_cursor_index == j {
                        self.main_cursor_index = i;
                    }

                    self.cursors.remove(j);
                }

                self.cursors[i] = if self.cursors[i].position < self.cursors[i].anchor {
                    Cursor {
                        position: range.from,
                        anchor: range.to,
                    }
                } else {
                    Cursor {
                        position: range.to,
                        anchor: range.from,
                    }
                };

                i += 1;
            }
        }

        let main_cursor = self.cursors[self.main_cursor_index];
        self.cursors.sort_by_key(|c| c.position);
        self.main_cursor_index = self
            .cursors
            .binary_search_by(|c| c.position.cmp(&main_cursor.position))
            .unwrap_or(0);
    }
}
