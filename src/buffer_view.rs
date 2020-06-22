use std::ops::{Index, IndexMut};

use crate::{
    buffer::{Buffer, BufferCollection, BufferHandle, TextRef},
    buffer_position::{BufferOffset, BufferPosition, BufferRange},
    history::EditKind,
};

pub struct BufferView {
    pub buffer_handle: BufferHandle,
    pub cursors: Vec<BufferPosition>,
}

impl BufferView {
    pub fn with_handle(buffer_handle: BufferHandle) -> Self {
        Self {
            buffer_handle,
            cursors: vec![BufferPosition::default()],
        }
    }

    pub fn buffer_mut<'a>(&mut self, buffers: &'a mut BufferCollection) -> &'a mut Buffer {
        &mut buffers[self.buffer_handle]
    }

    fn move_cursor(&mut self, buffers: &BufferCollection, offset: BufferOffset) {
        let buffer = &buffers[self.buffer_handle];
        for cursor in &mut self.cursors {
            let mut target = BufferOffset::from(*cursor) + offset;

            target.line_offset = target
                .line_offset
                .min(buffer.content.line_count() as isize - 1)
                .max(0);
            let target_line_len = buffer.content.line(target.line_offset as _).char_count();
            target.column_offset = target.column_offset.min(target_line_len as _).max(0);

            *cursor = target.into();
        }
    }

    fn commit_edits(&mut self, buffers: &mut BufferCollection) {
        buffers[self.buffer_handle].history.commit_edits();
    }

    fn insert_text<'a>(
        &'a mut self,
        buffers: &'a mut BufferCollection,
        text: TextRef<'a>,
    ) -> impl 'a + Iterator<Item = BufferRange> {
        let buffer = &mut buffers[self.buffer_handle];
        self.cursors.iter_mut().map(move |cursor| {
            let range = buffer.insert_text(*cursor, text);
            *cursor = cursor.insert(range);
            range
        })
    }

    fn delete_selection(&mut self, buffers: &mut BufferCollection) -> BufferRange {
        let buffer = &mut buffers[self.buffer_handle];
        for cursor in &mut self.cursors {
            let cursor_line_size = buffer.content.line(cursor.line_index).char_count();
            let mut selection_end = *cursor;
            if selection_end.column_index < cursor_line_size {
                selection_end.column_index += 1;
            } else {
                selection_end.line_index += 1;
                selection_end.column_index = 0;
            }
            let range = BufferRange::between(*cursor, selection_end);

            buffer.delete_range(range);
            *cursor = cursor.remove(range);
            //range
        }
        BufferRange::between(BufferPosition::default(), BufferPosition::default())
    }
}

#[derive(Default)]
pub struct BufferViewCollection {
    buffer_views: Vec<BufferView>,
}

impl BufferViewCollection {
    pub fn len(&self) -> usize {
        self.buffer_views.len()
    }

    pub fn push(&mut self, buffer_view: BufferView) {
        self.buffer_views.push(buffer_view);
    }

    fn current_and_other_buffer_views_mut<'a>(
        &'a mut self,
        index: usize,
    ) -> (&'a mut BufferView, impl Iterator<Item = &'a mut BufferView>) {
        let (before, after) = self.buffer_views.split_at_mut(index);
        let (current, after) = after.split_at_mut(1);
        let current = &mut current[0];
        let current_buffer_handle = current.buffer_handle;

        let iter = before
            .iter_mut()
            .filter(move |v| v.buffer_handle == current_buffer_handle)
            .chain(
                after
                    .iter_mut()
                    .filter(move |v| v.buffer_handle == current_buffer_handle),
            );

        (current, iter)
    }

    pub fn move_cursors(
        &mut self,
        buffers: &mut BufferCollection,
        index: Option<usize>,
        offset: BufferOffset,
    ) {
        if let Some(index) = index {
            self.buffer_views[index].move_cursor(buffers, offset);
        }
    }

    pub fn commit_edits(&mut self, buffers: &mut BufferCollection, index: Option<usize>) {
        if let Some(index) = index {
            self.buffer_views[index].commit_edits(buffers);
        }
    }

    pub fn insert_text(
        &mut self,
        buffers: &mut BufferCollection,
        index: Option<usize>,
        text: TextRef,
    ) {
        let index = match index {
            Some(index) => index,
            None => return,
        };

        let (current_view, other_views) = self.current_and_other_buffer_views_mut(index);
        let ranges = current_view.insert_text(buffers, text);
        for range in ranges {
            for view in other_views {
                for cursor in &mut view.cursors {
                    *cursor = cursor.insert(range);
                }
            }
        }
    }

    pub fn delete_selection(&mut self, buffers: &mut BufferCollection, index: Option<usize>) {
        let index = match index {
            Some(index) => index,
            None => return,
        };

        let (current_view, other_views) = self.current_and_other_buffer_views_mut(index);
        let range = current_view.delete_selection(buffers);
        for view in other_views {
            for cursor in &mut view.cursors {
                *cursor = cursor.remove(range);
            }
        }
    }

    pub fn undo(&mut self, buffers: &mut BufferCollection, index: Option<usize>) {
        let index = match index {
            Some(index) => index,
            None => return,
        };

        let buffer = &mut buffers[self.buffer_views[index].buffer_handle];
        self.apply_edits(index, buffer.undo());
    }

    pub fn redo(&mut self, buffers: &mut BufferCollection, index: Option<usize>) {
        let index = match index {
            Some(index) => index,
            None => return,
        };

        let buffer = &mut buffers[self.buffer_views[index].buffer_handle];
        self.apply_edits(index, buffer.redo());
    }

    pub fn apply_edits(
        &mut self,
        index: usize,
        edits: impl Iterator<Item = (EditKind, BufferRange)>,
    ) {
        for (kind, range) in edits {
            let (current_view, other_views) = self.current_and_other_buffer_views_mut(index);
            match kind {
                EditKind::Insert => {
                    for cursor in &mut current_view.cursors {
                        *cursor = range.to;
                    }
                    for view in other_views {
                        for cursor in &mut view.cursors {
                            *cursor = cursor.insert(range);
                        }
                    }
                }
                EditKind::Delete => {
                    for cursor in &mut current_view.cursors {
                        *cursor = range.from;
                    }
                    for view in other_views {
                        for cursor in &mut view.cursors {
                            *cursor = cursor.remove(range);
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
