use crate::{
    buffer_position::BufferPosition,
    buffer_view::{BufferViewCollection, BufferViewHandle},
};

pub struct ViewportCollection {
    viewports: [Viewport; 2],
    is_split: bool,

    current_viewport_index: usize,
}

impl ViewportCollection {
    pub fn new() -> Self {
        Self {
            viewports: [Viewport::default(), Viewport::default()],
            is_split: false,
            current_viewport_index: 0,
        }
    }

    pub fn set_view_size(&mut self, width: usize, height: usize) {
        self.viewports[0].height = height;
        self.viewports[1].height = height;

        if self.is_split {
            let half_width = width / 2;

            self.viewports[0].width = half_width;

            self.viewports[1].x = half_width;
            self.viewports[1].width = width - half_width;
        } else {
            self.viewports[0].width = width;
        }
    }

    pub fn next_viewport(&mut self, buffer_views: &mut BufferViewCollection) {
        if !self.is_split {
            self.is_split = true;
            self.set_view_size(self.viewports[0].width, self.viewports[0].height);

            if let Some(buffer_handle) = self.viewports[0].current_buffer_view_handle().map(|h| {
                let buffer_view = buffer_views.get(h).clone();
                buffer_views.add(buffer_view)
            }) {
                self.viewports[1].buffer_view_handles.push(buffer_handle);
            }
        }

        self.current_viewport_index += 1;
        self.current_viewport_index %= self.viewports.len();
    }

    pub fn current_viewport(&self) -> &Viewport {
        &self.viewports[self.current_viewport_index]
    }

    pub fn current_viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewports[self.current_viewport_index]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Viewport> {
        if self.is_split {
            self.viewports[..].iter()
        } else {
            self.viewports[..1].iter()
        }
    }
}

#[derive(Default)]
pub struct Viewport {
    buffer_view_handles: Vec<BufferViewHandle>,
    pub scroll: usize,

    pub x: usize,
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
