use std::ops::{Index, IndexMut};

use crate::buffer_view::BufferView;

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
            viewport.set_size(size);
        }
    }

    pub fn slice_mut(&mut self) -> &mut [Viewport] {
        &mut self.viewports[..]
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
    buffer_views: Vec<BufferView>,
    current_buffer_view: usize,
}

impl Viewport {
    pub fn set_size(&mut self, size: (usize, usize)) {
        for view in &mut self.buffer_views {
            view.size = size;
        }
    }

    pub fn add_buffer_view(&mut self, buffer_view: BufferView) {
        self.current_buffer_view = self.buffer_views.len();
        self.buffer_views.push(buffer_view);
    }

    pub fn current_buffer_view(&self) -> &BufferView {
        &self.buffer_views[self.current_buffer_view]
    }

    pub fn current_buffer_view_mut(&mut self) -> &mut BufferView {
        &mut self.buffer_views[self.current_buffer_view]
    }

    pub fn buffer_views_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut BufferView> {
        self.buffer_views.iter_mut()
    }
}
