use crate::{
    buffer::{BufferCollection, BufferHandle, TextRef},
    buffer_position::{BufferOffset, BufferPosition, BufferRange},
    history::EditKind,
    viewport::ViewportCollection,
};

pub struct BufferView {
    pub buffer_handle: BufferHandle,
    pub cursor: BufferPosition,
    pub size: (usize, usize),
    pub scroll: usize,
}

impl BufferView {
    pub fn with_handle(buffer_handle: BufferHandle) -> Self {
        Self {
            buffer_handle,
            cursor: Default::default(),
            size: Default::default(),
            scroll: 0,
        }
    }

    pub fn move_cursor(&mut self, buffers: &BufferCollection, offset: BufferOffset) {
        let buffer = &buffers[self.buffer_handle];
        let cursor = &mut self.cursor;

        let mut target = BufferOffset::from(*cursor) + offset;

        target.line_offset = target
            .line_offset
            .min(buffer.content.line_count() as isize - 1)
            .max(0);
        let target_line_len = buffer.content.line(target.line_offset as _).char_count();
        target.column_offset = target.column_offset.min(target_line_len as _).max(0);

        *cursor = target.into();

        if cursor.line_index < self.scroll {
            self.scroll = cursor.line_index;
        } else if cursor.line_index >= self.scroll + self.size.1 {
            self.scroll = cursor.line_index - self.size.1 + 1;
        }
    }

    pub fn insert_text(&mut self, buffers: &mut BufferCollection, text: TextRef) {
        let buffer = &mut buffers[self.buffer_handle];
        let cursor = &mut self.cursor;

        let range = buffer.insert_text(*cursor, text);
        *cursor = cursor.insert(range);
    }

    pub fn delete_selection(&mut self, buffers: &mut BufferCollection) {
        let buffer = &mut buffers[self.buffer_handle];
        let cursor = &mut self.cursor;

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
    }

    pub fn commit_edits(&mut self, buffers: &mut BufferCollection) {
        buffers[self.buffer_handle].history.commit_edits();
    }

    pub fn undo(&mut self, buffers: &mut BufferCollection) {
        let buffer = &mut buffers[self.buffer_handle];
        for (kind, range) in buffer.undo() {
            match kind {
                EditKind::Insert => self.cursor = range.to,
                EditKind::Delete => self.cursor = range.from,
            }
        }
    }

    pub fn redo(&mut self, buffers: &mut BufferCollection) {
        let buffer = &mut buffers[self.buffer_handle];
        for (kind, range) in buffer.redo() {
            match kind {
                EditKind::Insert => self.cursor = range.to,
                EditKind::Delete => self.cursor = range.from,
            }
        }
    }
}

pub struct BufferViews<'a> {
    viewports: &'a mut ViewportCollection,
    viewport_index: usize,
}

impl<'a> BufferViews<'a> {
    pub fn from_viewports(
        viewports: &'a mut ViewportCollection,
        current_viewport_index: usize,
    ) -> Self {
        Self {
            viewports,
            viewport_index: current_viewport_index,
        }
    }

    pub fn current_buffer_view_mut(&mut self) -> &mut BufferView {
        let viewport = &mut self.viewports[self.viewport_index];
        viewport.current_buffer_view_mut()
    }

    pub fn current_buffer_views_iter_mut(
        &'a mut self,
    ) -> (&'a mut BufferView, impl Iterator<Item = &'a mut BufferView>) {
        let (before, after) = self.viewports.slice_mut().split_at_mut(self.viewport_index);
        let (current, after) = after.split_at_mut(1);
        let current_buffer_view = current[0].current_buffer_view_mut();
        let current_buffer_handle = current_buffer_view.buffer_handle;

        let iter = before
            .iter_mut()
            .flat_map(move |v| {
                v.buffer_views_mut()
                    .filter(move |b| b.buffer_handle == current_buffer_handle)
            })
            .chain(after.iter_mut().flat_map(move |v| {
                v.buffer_views_mut()
                    .filter(move |b| b.buffer_handle == current_buffer_handle)
            }));

        (current_buffer_view, iter)
    }
}
