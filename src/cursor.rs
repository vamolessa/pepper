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

    pub fn iter(&self) -> impl Iterator<Item = &Cursor> {
        self.cursors.iter()
    }

    pub fn change_all<F>(&mut self, mut callback: F)
    where
        F: FnMut(&mut Cursor),
    {
        for cursor in &mut self.cursors {
            callback(cursor);
        }

        self.sort_and_collapse();
    }

    fn sort_and_collapse(&mut self) {
        let main_cursor = self.cursors[self.main_cursor_index];
        self.cursors.sort_by_key(|c| c.position);
        self.main_cursor_index = self
            .cursors
            .binary_search_by(|c| c.position.cmp(&main_cursor.position))
            .unwrap_or(0);
    }
}
