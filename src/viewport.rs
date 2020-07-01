use crate::{
    buffer_position::BufferPosition,
    buffer_view::{BufferViewCollection, BufferViewHandle},
};

const VERTICAL_MARGIN: usize = 2;

pub struct ViewportCollection {
    viewports: [Viewport; 2],
    is_split: bool,
}

impl ViewportCollection {
    pub fn new() -> Self {
        let mut this = Self {
            viewports: Default::default(),
            is_split: false,
        };
        this.viewports[0].is_current = true;
        this
    }

    pub fn set_view_size(&mut self, width: usize, height: usize) {
        self.viewports[0].height = height - VERTICAL_MARGIN;
        self.viewports[1].height = height - VERTICAL_MARGIN;

        if self.is_split {
            let half_width = width / 2;

            self.viewports[0].width = half_width;

            self.viewports[1].x = half_width;
            self.viewports[1].width = width - half_width;
        } else {
            self.viewports[0].width = width;
        }
    }

    pub fn focus_next_viewport(&mut self, buffer_views: &mut BufferViewCollection) {
        if !self.is_split {
            self.is_split = true;
            self.set_view_size(
                self.viewports[0].width,
                self.viewports[0].height + VERTICAL_MARGIN,
            );

            if let Some(buffer_handle) = self.viewports[0].current_buffer_view_handle().map(|h| {
                let buffer_view = buffer_views.get(h).clone();
                buffer_views.add(buffer_view)
            }) {
                self.viewports[1].buffer_view_handles.push(buffer_handle);
            }
        }

        for viewport in &mut self.viewports {
            viewport.is_current = !viewport.is_current;
        }
    }

    pub fn current_viewport(&self) -> &Viewport {
        if self.viewports[0].is_current {
            &self.viewports[0]
        } else {
            &self.viewports[1]
        }
    }

    pub fn current_viewport_mut(&mut self) -> &mut Viewport {
        if self.viewports[0].is_current {
            &mut self.viewports[0]
        } else {
            &mut self.viewports[1]
        }
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
    pub is_current: bool,
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
