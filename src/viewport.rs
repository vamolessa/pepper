use crate::{
    buffer_position::BufferPosition,
    buffer_view::{BufferViewCollection, BufferViewHandle},
};

enum LayoutElement {
    Root,
    VerticalSplit(usize),
    HorizontalSplit(usize),
    Leaf(usize, Viewport),
}

pub struct ViewportCollection {
    layout: Vec<LayoutElement>,
    current_viewport_index: usize,
    max_depth: usize,
}

impl ViewportCollection {
    pub fn with_max_depth(max_depth: usize) -> Self {
        let mut this = Self {
            layout: Vec::new(),
            max_depth,
            current_viewport_index: 1,
        };
        this.close_all_viewports();
        this
    }

    pub fn set_view_size(&mut self, width: usize, height: usize) {
        //
    }

    pub fn next_viewport(&mut self) {
        self.current_viewport_index = (self.current_viewport_index + 1) % self.layout.len();
    }

    pub fn close_all_viewports(&mut self) {
        self.layout.clear();
        self.layout.push(LayoutElement::Root);
        self.layout
            .push(LayoutElement::Leaf(0, Viewport::default()));
    }

    pub fn split_current_viewport(&mut self, buffer_views: &mut BufferViewCollection) {
        {
            let mut depth = 0;
            let mut index = self.current_viewport_index;
            loop {
                match self.layout[index] {
                    LayoutElement::Root => break,
                    LayoutElement::VerticalSplit(parent_index) => index = parent_index,
                    LayoutElement::HorizontalSplit(parent_index) => index = parent_index,
                    LayoutElement::Leaf(parent_index, _) => index = parent_index,
                }
                depth += 1;
            }
            if depth >= self.max_depth {
                return;
            }
        }

        let mut current_viewport = LayoutElement::Root;
        std::mem::swap(
            &mut current_viewport,
            &mut self.layout[self.current_viewport_index],
        );
        let (parent_index, mut current_viewport) = match current_viewport {
            LayoutElement::Leaf(parent_index, viewport) => (parent_index, viewport),
            _ => unreachable!(),
        };

        let buffer_view_handles = match current_viewport
            .current_buffer_view_handle()
            .map(|handle| buffer_views.add(buffer_views.get(handle).clone()))
        {
            Some(index) => vec![index],
            None => Vec::new(),
        };

        let mut x = current_viewport.x;
        let mut y = current_viewport.y;
        let mut width = current_viewport.width;
        let mut height = current_viewport.height;

        if current_viewport.width > current_viewport.height {
            width /= 2;
            current_viewport.width -= width;
            x += current_viewport.width;

            self.layout.push(LayoutElement::VerticalSplit(parent_index));
        } else {
            height = current_viewport.height / 2;
            current_viewport.height -= height;
            y += current_viewport.height;

            self.layout
                .push(LayoutElement::HorizontalSplit(parent_index));
        }

        let parent_index = self.current_viewport_index;
        self.current_viewport_index = self.layout.len();

        self.layout.push(LayoutElement::Leaf(
            parent_index,
            Viewport {
                buffer_view_handles,
                scroll: current_viewport.scroll,
                x,
                y,
                width,
                height,
            },
        ));
        self.layout
            .push(LayoutElement::Leaf(parent_index, current_viewport));
    }

    pub fn close_current_viewport(&mut self, buffer_views: &mut BufferViewCollection) {
        let (current_parent_index, current_viewport) =
            match self.layout.remove(self.current_viewport_index) {
                LayoutElement::Leaf(parent_index, viewport) => (parent_index, viewport),
                _ => unreachable!(),
            };

        for handle in current_viewport.buffer_view_handles {
            buffer_views.remove(handle);
        }

        let mut sibling_index = None;
        for (i, element) in self.layout.iter_mut().enumerate() {
            let parent_index = match element {
                LayoutElement::Root => continue,
                LayoutElement::VerticalSplit(parent_index) => parent_index,
                LayoutElement::HorizontalSplit(parent_index) => parent_index,
                LayoutElement::Leaf(parent_index, _) => parent_index,
            };

            if *parent_index == current_parent_index {
                sibling_index = Some(i);
            }

            if *parent_index > self.current_viewport_index {
                *parent_index -= 1;
            }
        }

        let sibling_index = match sibling_index {
            Some(index) => index,
            None => {
                self.close_all_viewports();
                return;
            }
        };

        let sibling_viewport = match self.layout.remove(sibling_index) {
            LayoutElement::Leaf(_, viewport) => viewport,
            _ => unreachable!(),
        };

        for element in self.layout.iter_mut() {
            let parent_index = match element {
                LayoutElement::Root => continue,
                LayoutElement::VerticalSplit(parent_index) => parent_index,
                LayoutElement::HorizontalSplit(parent_index) => parent_index,
                LayoutElement::Leaf(parent_index, _) => parent_index,
            };

            if *parent_index > sibling_index {
                *parent_index -= 1;
            }
        }

        let parent_index = match self.layout[current_parent_index] {
            LayoutElement::HorizontalSplit(parent_index) => parent_index,
            LayoutElement::VerticalSplit(parent_index) => parent_index,
            _ => unreachable!(),
        };

        self.layout[current_parent_index] = LayoutElement::Leaf(parent_index, sibling_viewport);
        self.current_viewport_index = current_parent_index;
    }

    pub fn current_viewport(&self) -> &Viewport {
        match self.layout[self.current_viewport_index] {
            LayoutElement::Leaf(_, ref viewport) => viewport,
            _ => unreachable!(),
        }
    }

    pub fn current_viewport_mut(&mut self) -> &mut Viewport {
        match self.layout[self.current_viewport_index] {
            LayoutElement::Leaf(_, ref mut viewport) => viewport,
            _ => unreachable!(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Viewport> {
        self.layout.iter().filter_map(|e| match e {
            LayoutElement::Leaf(_, viewport) => Some(viewport),
            _ => None,
        })
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
