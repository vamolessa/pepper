use crate::{
    buffer_position::BufferPosition,
    buffer_view::{BufferViewCollection, BufferViewHandle},
};

pub struct ViewportCollection {
    viewports: [Viewport; 2],
    viewport_count: usize,

    current_viewport_index: usize,
    available_width: usize,
    available_height: usize,
}

impl ViewportCollection {
    pub fn new() -> Self {
        Self {
            viewports: [Viewport::default(), Viewport::default()],
            viewport_count: 1,
            current_viewport_index: 0,
            available_width: 0,
            available_height: 0,
        }
    }

    pub fn set_view_size(&mut self, width: usize, height: usize) {
        self.available_width = width;
        self.available_height = height;

        if self.viewport_count > 1 {
            let half_width = width / 2;

            self.viewports[0].width = half_width;
            self.viewports[0].height = height;

            self.viewports[1].x = half_width;
            self.viewports[1].width = width - half_width;
            self.viewports[1].height = height;
        } else {
            self.viewports[0].width = width;
            self.viewports[0].height = height;
        }
    }

    pub fn next_viewport(&mut self, buffer_views: &mut BufferViewCollection) {
        if self.viewport_count == 1 {
            self.viewport_count = 2;
            self.set_view_size(self.available_width, self.available_height);

            if let Some(buffer_handle) = self.viewports[0].current_buffer_view_handle().map(|h| {
                let buffer_view = buffer_views.get(h).clone();
                buffer_views.add(buffer_view)
            }) {
                self.viewports[1].buffer_view_handles.push(buffer_handle);
            }
        }

        self.current_viewport_index += 1;
        self.current_viewport_index %= self.viewport_count;
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
    buffer_view_handles: Vec<BufferViewHandle>,
    pub scroll: usize,

    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Viewport {
    pub fn current_buffer_view_handle(&self) -> Option<&BufferViewHandle> {
        self.buffer_view_handles.first()
    }

    pub fn set_current_buffer_view_handle(&mut self, handle: BufferViewHandle) {
        if let Some(index_position) = self.buffer_view_handles.iter().position(|h| *h == handle) {
            self.buffer_view_handles.swap(0, index_position);
        } else {
            let last_index = self.buffer_view_handles.len();
            self.buffer_view_handles.push(handle);
            self.buffer_view_handles.swap(0, last_index);
        }

        self.scroll = 0;
    }

    pub fn scroll_to_cursor(&mut self, cursor: BufferPosition) {
        if cursor.line_index < self.scroll {
            self.scroll = cursor.line_index;
        } else if cursor.line_index >= self.scroll + self.height {
            self.scroll = cursor.line_index - self.height + 1;
        }
    }
}
