use crate::buffer_position::{BufferPosition, BufferRange};

#[derive(Default, Clone, Copy)]
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

    pub fn iter(&self) -> impl Iterator<Item = &Cursor> {
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

    pub fn change_all_from<F>(&mut self, from_index: usize, mut callback: F)
    where
        F: FnMut(&mut Cursor),
    {
        for cursor in &mut self.cursors[from_index..] {
            callback(cursor);
        }

        self.sort_and_merge();
    }

    fn sort_and_merge(&mut self) {
        let main_cursor = self.cursors[self.main_cursor_index];
        self.cursors.sort_by_key(|c| c.position);
        self.main_cursor_index = self
            .cursors
            .binary_search_by(|c| c.position.cmp(&main_cursor.position))
            .unwrap_or(0);
    }
}
