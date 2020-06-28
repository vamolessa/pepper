use crate::buffer_position::BufferPosition;

pub enum ViewportOperation {
    NextViewport,
    SplitVertical,
}

pub struct ViewportCollection {
    viewports: Vec<Viewport>,
    current_viewport_index: usize,
    available_size: (usize, usize),
}

impl ViewportCollection {
    pub fn new() -> Self {
        Self {
            viewports: vec![Viewport::default()],
            current_viewport_index: 0,
            available_size: (0, 0),
        }
    }

    pub fn set_view_size(&mut self, size: (usize, usize)) {
        self.available_size = size;
    }

    pub fn handle_operation(&mut self, operation: ViewportOperation) {
        match operation {
            ViewportOperation::NextViewport => {
                self.current_viewport_index =
                    (self.current_viewport_index + 1) % self.viewports.len();
            }
            ViewportOperation::SplitVertical => {
                let current_viewport = self.current_viewport();
                let new_viewport = Viewport {
                    buffer_view_index: current_viewport.buffer_view_index,
                    scroll: current_viewport.scroll,
                    ..Default::default()
                };
                self.viewports.push(new_viewport);
            }
        }
    }

    pub fn current_viewport(&self) -> &Viewport {
        &self.viewports[self.current_viewport_index]
    }

    pub fn current_viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewports[self.current_viewport_index]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Viewport> {
        self.viewports.iter()
    }
}

#[derive(Default)]
pub struct Viewport {
    buffer_view_index: Option<usize>,
    pub position: (usize, usize),
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
