use crate::buffer_position::BufferPosition;

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
        self.update_viewports_positions();
    }

    pub fn next_viewport(&mut self) {
        self.current_viewport_index = (self.current_viewport_index + 1) % self.viewports.len();
    }

    pub fn split_current_viewport(&mut self) {
        if self.viewports.len() >= 4 {
            return;
        }

        let current_viewport = self.current_viewport();
        let new_viewport = Viewport {
            buffer_view_index: current_viewport.buffer_view_index,
            scroll: current_viewport.scroll,
            ..Default::default()
        };
        self.current_viewport_index = self.viewports.len();
        self.viewports.push(new_viewport);
        self.update_viewports_positions();
    }

    pub fn close_current_viewport(&mut self) {
        self.viewports.remove(self.current_viewport_index);

        if self.viewports.len() > 0 {
            self.current_viewport_index -= 1;
        } else {
            self.current_viewport_index = 0;
            self.viewports.push(Viewport::default());
        }

        self.update_viewports_positions();
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

    fn update_viewports_positions(&mut self) {
        self.viewports.truncate(4);
        match self.viewports.len() {
            1 => {
                self.viewports[0].position = (0, 0);
                self.viewports[0].size = self.available_size;
            }
            2 => {
                let half_width = self.available_size.0 / 2;

                self.viewports[0].position = (0, 0);
                self.viewports[0].size = (half_width, self.available_size.1);
                self.viewports[1].position = (half_width, 0);
                self.viewports[1].size = (half_width, self.available_size.1);
            }
            3 => {
                let half_width = self.available_size.0 / 2;
                let half_height = self.available_size.1 / 2;

                self.viewports[0].position = (0, 0);
                self.viewports[0].size = (half_width, self.available_size.1);
                self.viewports[1].position = (half_width, 0);
                self.viewports[1].size = (half_width, half_height);
                self.viewports[2].position = (half_width, half_height);
                self.viewports[2].size = (half_width, half_height);
            }
            4 => {
                let half_width = self.available_size.0 / 2;
                let half_height = self.available_size.1 / 2;

                self.viewports[0].position = (0, 0);
                self.viewports[0].size = (half_width, half_height);
                self.viewports[1].position = (half_width, 0);
                self.viewports[1].size = (half_width, half_height);
                self.viewports[2].position = (0, half_height);
                self.viewports[2].size = (half_width, half_height);
                self.viewports[3].position = (half_width, half_height);
                self.viewports[3].size = (half_width, half_height);
            }
            _ => (),
        }
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
