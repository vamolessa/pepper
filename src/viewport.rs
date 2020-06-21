use std::ops::{Index, IndexMut};

use crate::buffer_position::BufferPosition;

pub struct ViewportCollection {
    viewports: Vec<Viewport>,
}

impl ViewportCollection {
    pub fn new() -> Self {
        Self {
            viewports: vec![Viewport::default()],
        }
    }

    pub fn set_view_size(&mut self, size: (usize, usize)) {
        for viewport in &mut self.viewports {
            viewport.size = size;
        }
    }
}

impl Index<usize> for ViewportCollection {
    type Output = Viewport;
    fn index(&self, index: usize) -> &Self::Output {
        &self.viewports[index]
    }
}

impl IndexMut<usize> for ViewportCollection {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.viewports[index]
    }
}

#[derive(Default)]
pub struct Viewport {
    buffer_view_index: Option<usize>,
    pub size: (usize, usize),
    pub scroll: usize,
}

impl Viewport {
    pub fn buffer_view_index(&self) -> Option<usize> {
        self.buffer_view_index
    }

    pub fn set_buffer_view(&mut self, index: Option<usize>) {
        self.buffer_view_index = index;
        self.scroll = 0;
    }

    pub fn scroll_to_cursor(&mut self, cursor: BufferPosition) {
        if cursor.line_index < self.scroll {
            self.scroll = cursor.line_index;
        } else if cursor.line_index >= self.scroll + self.size.1 {
            self.scroll = cursor.line_index - self.size.1 + 1;
        }
    }
}
