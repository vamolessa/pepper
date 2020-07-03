use crate::{
    buffer::{BufferCollection, BufferHandle, TextRef},
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

    pub fn move_cursors(
        &mut self,
        buffers: &BufferCollection,
        offset: BufferOffset,
        collapse_anchors: bool,
    ) {
        let buffer = match buffers.get(self.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

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
        if let Some(buffer) = buffers.get_mut(self.buffer_handle) {
            buffer.history.commit_edits();
        }
    }
}

#[derive(Eq, PartialEq)]
pub struct BufferViewHandle(usize);

#[derive(Default)]
pub struct BufferViewCollection {
    buffer_views: Vec<Option<BufferView>>,
    free_slots: Vec<BufferViewHandle>,
    temp_ranges: Vec<BufferRange>,
}

impl BufferViewCollection {
    pub fn add(&mut self, buffer_view: BufferView) -> BufferViewHandle {
        if let Some(handle) = self.free_slots.pop() {
            self.buffer_views[handle.0] = Some(buffer_view);
            handle
        } else {
            let index = self.buffer_views.len();
            self.buffer_views.push(Some(buffer_view));
            BufferViewHandle(index)
        }
    }

    pub fn _remove(&mut self, handle: BufferViewHandle) {
        self.buffer_views[handle.0] = None;
        self.free_slots.push(handle);
    }

    pub fn get(&self, handle: &BufferViewHandle) -> &BufferView {
        self.buffer_views[handle.0].as_ref().unwrap()
    }

    pub fn get_mut(&mut self, handle: &BufferViewHandle) -> &mut BufferView {
        self.buffer_views[handle.0].as_mut().unwrap()
    }

    pub fn insert_text(
        &mut self,
        buffers: &mut BufferCollection,
        handle: &BufferViewHandle,
        text: TextRef,
    ) {
        let current_view = match &mut self.buffer_views[handle.0] {
            Some(view) => view,
            None => return,
        };
        let buffer = match buffers.get_mut(current_view.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        self.temp_ranges.clear();
        for cursor in current_view.cursors[..].iter().rev() {
            let range = buffer.insert_text(cursor.position, text);
            self.temp_ranges.push(range);
        }

        for view in self.buffer_views.iter_mut().flatten() {
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

    pub fn remove_in_selection(
        &mut self,
        buffers: &mut BufferCollection,
        handle: &BufferViewHandle,
    ) {
        let current_view = match &mut self.buffer_views[handle.0] {
            Some(view) => view,
            None => return,
        };
        let buffer = match buffers.get_mut(current_view.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        self.temp_ranges.clear();
        for cursor in current_view.cursors[..].iter().rev() {
            let range = cursor.range();
            buffer.remove_range(range);
            self.temp_ranges.push(range);
        }

        for view in self.buffer_views.iter_mut().flatten() {
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

    pub fn undo(&mut self, buffers: &mut BufferCollection, handle: &BufferViewHandle) {
        if let Some(buffer) = self.buffer_views[handle.0]
            .as_mut()
            .and_then(|view| buffers.get_mut(view.buffer_handle))
        {
            self.apply_edits(handle.0, buffer.undo());
        }
    }

    pub fn redo(&mut self, buffers: &mut BufferCollection, handle: &BufferViewHandle) {
        if let Some(buffer) = self.buffer_views[handle.0]
            .as_mut()
            .and_then(|view| buffers.get_mut(view.buffer_handle))
        {
            self.apply_edits(handle.0, buffer.redo());
        }
    }

    fn apply_edits(&mut self, index: usize, edits: impl Iterator<Item = (EditKind, BufferRange)>) {
        for (kind, range) in edits {
            match kind {
                EditKind::Insert => {
                    self.buffer_views[index]
                        .as_mut()
                        .unwrap()
                        .cursors
                        .change_all(|cs| {
                            for c in cs {
                                c.anchor = range.to;
                                c.position = range.to;
                            }
                        });
                    for (i, view) in self.buffer_views.iter_mut().flatten().enumerate() {
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
                    self.buffer_views[index]
                        .as_mut()
                        .unwrap()
                        .cursors
                        .change_all(|cs| {
                            for c in cs {
                                c.anchor = range.from;
                                c.position = range.from;
                            }
                        });
                    for (i, view) in self.buffer_views.iter_mut().flatten().enumerate() {
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
