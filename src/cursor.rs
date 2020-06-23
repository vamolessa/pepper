use crate::buffer_position::BufferRange;

pub struct CursorCollection {
    cursors: Vec<BufferRange>,
    main_cursor_index: usize,
}

impl CursorCollection {
    pub fn new() -> Self {
        Self {
            cursors: vec![BufferRange::default()],
            main_cursor_index: 0,
        }
    }

    fn sort_and_collapse(&mut self) {
    }
}
