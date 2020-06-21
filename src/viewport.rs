use std::ops::{Index, IndexMut};

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
    pub buffer_view_index: Option<usize>,
    pub size: (usize, usize),
    pub scroll: usize,
}
