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

    pub fn as_slice(&self) -> &[Viewport] {
        &self.viewports[..]
    }

    pub fn as_slice_mut(&mut self) -> &mut [Viewport] {
        &mut self.viewports[..]
    }
}

#[derive(Default)]
pub struct Viewport {
    pub buffer_view_index: Option<usize>,
    pub size: (usize, usize),
    pub scroll: usize,
}
