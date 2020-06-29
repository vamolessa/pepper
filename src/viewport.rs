use crate::{
    buffer_position::BufferPosition,
    buffer_view::{BufferViewCollection, BufferViewHandle},
};

enum LayoutElement {
    VerticalSplit,
    HorizontalSplit,
    Leaf(Viewport),
}

pub struct ViewportCollection {
    layout: Vec<LayoutElement>,
    current_viewport_index: usize,
    available_width: usize,
    available_height: usize,
    max_depth: u8,
}

impl ViewportCollection {
    pub fn with_max_depth(max_depth: u8) -> Self {
        let mut this = Self {
            layout: Vec::new(),
            max_depth,
            current_viewport_index: 0,
            available_width: 0,
            available_height: 0,
        };
        this.close_all_viewports();
        this
    }

    pub fn set_view_size(&mut self, width: usize, height: usize) {
        fn traverse(
            layout: &mut [LayoutElement],
            index: usize,
            x: usize,
            y: usize,
            width: usize,
            height: usize,
        ) -> usize {
            match layout[index] {
                LayoutElement::VerticalSplit => {
                    let half_width = width / 2;
                    let index = traverse(layout, index + 1, x, y, half_width, height);
                    let index = traverse(
                        layout,
                        index + 1,
                        x + half_width,
                        y,
                        width - half_width,
                        height,
                    );
                    index
                }
                LayoutElement::HorizontalSplit => {
                    let half_height = height / 2;
                    let index = traverse(layout, index + 1, x, y, width, half_height);
                    let index = traverse(
                        layout,
                        index + 1,
                        x,
                        y + half_height,
                        width,
                        height - half_height,
                    );
                    index
                }
                LayoutElement::Leaf(ref mut viewport) => {
                    viewport.x = x;
                    viewport.y = y;
                    viewport.width = width;
                    viewport.height = height;
                    index
                }
            }
        }

        self.available_width = width;
        self.available_height = height;
        traverse(&mut self.layout[..], 0, 0, 0, width, height);
    }

    pub fn next_viewport(&mut self) {
        self.current_viewport_index += 1;
        for i in self.current_viewport_index..self.layout.len() {
            if let LayoutElement::Leaf(_) = &self.layout[i] {
                self.current_viewport_index = i;
                return;
            }
        }
        for i in 0..self.current_viewport_index {
            if let LayoutElement::Leaf(_) = &self.layout[i] {
                self.current_viewport_index = i;
                return;
            }
        }

        unreachable!();
    }

    pub fn close_all_viewports(&mut self) {
        self.layout.clear();
        self.layout.push(LayoutElement::Leaf(Viewport::default()));
        self.current_viewport_index = 0;
        self.set_view_size(self.available_height, self.available_width);
    }

    pub fn split_current_viewport(&mut self, buffer_views: &mut BufferViewCollection) {
        fn find_element_depth(layout: &[LayoutElement], index: usize) -> u8 {
            fn traverse(
                layout: &[LayoutElement],
                target_index: usize,
                index: usize,
                depth: u8,
            ) -> Option<u8> {
                if index == target_index {
                    return Some(depth);
                }

                match layout[index] {
                    LayoutElement::HorizontalSplit | LayoutElement::VerticalSplit => {
                        match traverse(layout, target_index, index + 1, depth + 1) {
                            Some(depth) => Some(depth),
                            None => traverse(layout, target_index, index + 1, depth + 1),
                        }
                    }
                    LayoutElement::Leaf(_) => None,
                }
            }

            traverse(layout, index, 0, 0).unwrap_or(0)
        }

        if find_element_depth(&self.layout[..], self.current_viewport_index) >= self.max_depth {
            return;
        }

        let current_viewport = match self.layout[self.current_viewport_index] {
            LayoutElement::Leaf(ref mut viewport) => viewport,
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

        let split = if width > height * 2 {
            width /= 2;
            current_viewport.width -= width;
            x += current_viewport.width;
            LayoutElement::VerticalSplit
        } else {
            height = current_viewport.height / 2;
            current_viewport.height -= height;
            y += current_viewport.height;
            LayoutElement::HorizontalSplit
        };

        let new_viewport = Viewport {
            buffer_view_handles,
            scroll: current_viewport.scroll,
            x,
            y,
            width,
            height,
        };

        drop(current_viewport);
        self.layout.insert(self.current_viewport_index, split);
        self.current_viewport_index += 2;
        self.layout.insert(
            self.current_viewport_index,
            LayoutElement::Leaf(new_viewport),
        );
    }

    pub fn close_current_viewport(&mut self, buffer_views: &mut BufferViewCollection) {
        fn find_element_parent_index(layout: &[LayoutElement], index: usize) -> usize {
            enum FindResult {
                Leaf(usize),
                Parent(usize),
            }
            fn traverse(
                layout: &[LayoutElement],
                target_index: usize,
                current_index: usize,
                parent_index: usize,
            ) -> FindResult {
                if current_index == target_index {
                    return FindResult::Parent(parent_index);
                }

                match layout[current_index] {
                    LayoutElement::HorizontalSplit | LayoutElement::VerticalSplit => {
                        match traverse(layout, target_index, current_index + 1, current_index) {
                            FindResult::Leaf(index) => {
                                traverse(layout, target_index, index + 1, current_index)
                            }
                            FindResult::Parent(index) => FindResult::Parent(index),
                        }
                    }
                    LayoutElement::Leaf(_) => FindResult::Leaf(current_index),
                }
            }

            match traverse(layout, index, 0, 0) {
                FindResult::Parent(index) => index,
                _ => unreachable!(),
            }
        }

        if self.current_viewport_index == 0 {
            self.close_all_viewports();
            return;
        }

        let parent_index = find_element_parent_index(&self.layout[..], self.current_viewport_index);
        let previous_viewport = self.layout.remove(self.current_viewport_index);
        self.layout.remove(parent_index);

        if let LayoutElement::Leaf(viewport) = previous_viewport {
            for handle in viewport.buffer_view_handles {
                buffer_views.remove(handle);
            }
        }

        self.current_viewport_index = if parent_index > 0 {
            parent_index - 1
        } else {
            parent_index
        };
        self.next_viewport();
        self.set_view_size(self.available_width, self.available_height);
    }

    pub fn current_viewport(&self) -> &Viewport {
        match self.layout[self.current_viewport_index] {
            LayoutElement::Leaf(ref viewport) => viewport,
            _ => unreachable!(),
        }
    }

    pub fn current_viewport_mut(&mut self) -> &mut Viewport {
        match self.layout[self.current_viewport_index] {
            LayoutElement::Leaf(ref mut viewport) => viewport,
            _ => unreachable!(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Viewport> {
        self.layout.iter().filter_map(|e| match e {
            LayoutElement::Leaf(viewport) => Some(viewport),
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
