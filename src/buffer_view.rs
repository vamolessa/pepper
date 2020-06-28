use std::ops::{Index, IndexMut};

use crate::{
    buffer::{Buffer, BufferCollection, BufferHandle, TextRef},
    buffer_position::{BufferOffset, BufferRange},
    cursor::CursorCollection,
    history::EditKind,
};

#[derive(Clone)]
pub struct BufferView {
    pub buffer_handle: BufferHandle,
    pub cursors: CursorCollection,
}

impl BufferView {
    pub fn with_handle(buffer_handle: BufferHandle) -> Self {
        Self {
            buffer_handle,
            cursors: CursorCollection::new(),
        }
    }

    pub fn buffer<'a>(&self, buffers: &'a BufferCollection) -> &'a Buffer {
        &buffers[self.buffer_handle]
    }

    pub fn move_cursors(
        &mut self,
        buffers: &BufferCollection,
        offset: BufferOffset,
        collapse_anchors: bool,
    ) {
        let buffer = &buffers[self.buffer_handle];
        self.cursors.change_all(|cs| {
            for c in cs {
                let mut target = BufferOffset::from(c.position) + offset;

                target.line_offset = target
                    .line_offset
                    .min(buffer.content.line_count() as isize - 1)
                    .max(0);
                let target_line_len = buffer.content.line(target.line_offset as _).char_count();
                target.column_offset = target.column_offset.min(target_line_len as _);

                c.position = target.into();
                if collapse_anchors {
                    c.anchor = c.position;
                }
            }
        });
    }

    pub fn commit_edits(&self, buffers: &mut BufferCollection) {
        buffers[self.buffer_handle].history.commit_edits();
    }
}

#[derive(Default)]
pub struct BufferViewCollection {
    buffer_views: Vec<BufferView>,
    temp_ranges: Vec<BufferRange>,
}

impl BufferViewCollection {
    pub fn add(&mut self, buffer_view: BufferView) -> usize {
        let index = self.buffer_views.len();
        self.buffer_views.push(buffer_view);
        index
    }

    pub fn insert_text(&mut self, buffers: &mut BufferCollection, index: usize, text: TextRef) {
        let current_view = &mut self.buffer_views[index];
        let buffer = &mut buffers[current_view.buffer_handle];

        self.temp_ranges.clear();
        for cursor in current_view.cursors.iter().rev() {
            let range = buffer.insert_text(cursor.position, text);
            self.temp_ranges.push(range);
        }

        for view in &mut self.buffer_views {
            let ranges = &self.temp_ranges;
            view.cursors.change_all(|cs| {
                for c in cs {
                    for range in ranges.iter() {
                        c.insert(*range);
                    }
                }
            });
        }
    }

    pub fn remove_in_selection(&mut self, buffers: &mut BufferCollection, index: usize) {
        let current_view = &mut self.buffer_views[index];
        let buffer = &mut buffers[current_view.buffer_handle];

        self.temp_ranges.clear();
        for cursor in current_view.cursors.iter().rev() {
            let range = cursor.range();
            buffer.remove_range(range);
            self.temp_ranges.push(range);
        }

        for view in &mut self.buffer_views {
            let ranges = &self.temp_ranges;
            view.cursors.change_all(|cs| {
                for c in cs {
                    for range in ranges.iter() {
                        c.remove(*range);
                    }
                }
            });
        }
    }

    pub fn undo(&mut self, buffers: &mut BufferCollection, index: usize) {
        let buffer = &mut buffers[self.buffer_views[index].buffer_handle];
        self.apply_edits(index, buffer.undo());
    }

    pub fn redo(&mut self, buffers: &mut BufferCollection, index: usize) {
        let buffer = &mut buffers[self.buffer_views[index].buffer_handle];
        self.apply_edits(index, buffer.redo());
    }

    fn apply_edits(&mut self, index: usize, edits: impl Iterator<Item = (EditKind, BufferRange)>) {
        for (kind, range) in edits {
            match kind {
                EditKind::Insert => {
                    self.buffer_views[index].cursors.change_all(|cs| {
                        for c in cs {
                            c.anchor = range.to;
                            c.position = range.to;
                        }
                    });
                    for (i, view) in self.buffer_views.iter_mut().enumerate() {
                        if i != index {
                            view.cursors.change_all(|cs| {
                                for c in cs {
                                    c.insert(range);
                                }
                            });
                        }
                    }
                }
                EditKind::Remove => {
                    self.buffer_views[index].cursors.change_all(|cs| {
                        for c in cs {
                            c.anchor = range.from;
                            c.position = range.from;
                        }
                    });
                    for (i, view) in self.buffer_views.iter_mut().enumerate() {
                        if i != index {
                            view.cursors.change_all(|cs| {
                                for c in cs {
                                    c.remove(range);
                                }
                            });
                        }
                    }
                }
            }
        }
    }
}

impl Index<usize> for BufferViewCollection {
    type Output = BufferView;
    fn index(&self, index: usize) -> &BufferView {
        &self.buffer_views[index]
    }
}

impl IndexMut<usize> for BufferViewCollection {
    fn index_mut(&mut self, index: usize) -> &mut BufferView {
        &mut self.buffer_views[index]
    }
}
