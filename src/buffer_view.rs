use crate::{
    buffer::{Buffer, BufferCollection, BufferHandle, TextRef},
    buffer_position::{BufferOffset, BufferPosition, BufferRange},
    history::EditKind,
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

    pub fn buffer_mut<'a>(&mut self, buffers: &'a mut BufferCollection) -> Option<&'a mut Buffer> {
        Some(&mut buffers[self.buffer_handle])
    }

    fn move_cursor(&mut self, buffers: &BufferCollection, offset: BufferOffset) {
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

    fn commit_edits(&mut self, buffers: &mut BufferCollection) {
        buffers[self.buffer_handle].history.commit_edits();
    }

    fn insert_text(&mut self, buffers: &mut BufferCollection, text: TextRef) -> BufferRange {
        let buffer = &mut buffers[self.buffer_handle];
        let cursor = &mut self.cursor;

        let range = buffer.insert_text(*cursor, text);
        *cursor = cursor.insert(range);
        range
    }

    fn delete_selection(&mut self, buffers: &mut BufferCollection) -> BufferRange {
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
        range
    }
}

#[derive(Default)]
pub struct BufferViewCollection {
    buffer_views: Vec<BufferView>,
}

impl BufferViewCollection {

    pub fn get_mut(&mut self, index: Option<usize>) -> Option<&mut BufferView> {
        Some(&mut self.buffer_views[index?])
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
    ) -> Option<()> {
        self.buffer_views[index?].move_cursor(buffers, offset);
        None
    }

    pub fn commit_edits(
        &mut self,
        buffers: &mut BufferCollection,
        index: Option<usize>,
    ) -> Option<()> {
        self.buffer_views[index?].commit_edits(buffers);
        None
    }

    pub fn insert_text(
        &mut self,
        buffers: &mut BufferCollection,
        index: Option<usize>,
        text: TextRef,
    ) -> Option<()> {
        let (current_view, other_views) = self.current_and_other_buffer_views_mut(index?);
        let range = current_view.insert_text(buffers, text);
        for view in other_views {
            view.cursor = view.cursor.insert(range);
        }
        None
    }

    pub fn delete_selection(
        &mut self,
        buffers: &mut BufferCollection,
        index: Option<usize>,
    ) -> Option<()> {
        let (current_view, other_views) = self.current_and_other_buffer_views_mut(index?);
        let range = current_view.delete_selection(buffers);
        for view in &mut other_views {
            view.cursor = view.cursor.remove(range);
        }
        None
    }

    pub fn undo(&mut self, buffers: &mut BufferCollection, index: Option<usize>) -> Option<()> {
        let (current_view, other_views) = self.current_and_other_buffer_views_mut(index?);

        let buffer = &mut buffers[current_view.buffer_handle];
        for (kind, range) in buffer.undo() {
            match kind {
                EditKind::Insert => {
                    current_view.cursor = range.to;
                    for view in &mut other_views {
                        view.cursor = view.cursor.insert(range);
                    }
                }
                EditKind::Delete => {
                    current_view.cursor = range.from;
                    for view in &mut other_views {
                        view.cursor = view.cursor.remove(range);
                    }
                }
            }
        }
        None
    }

    pub fn redo(&mut self, buffers: &mut BufferCollection, index: Option<usize>) -> Option<()> {
        let (current_view, other_views) = self.current_and_other_buffer_views_mut(index?);

        let buffer = &mut buffers[current_view.buffer_handle];
        for (kind, range) in buffer.redo() {
            match kind {
                EditKind::Insert => {
                    current_view.cursor = range.to;
                    for view in &mut other_views {
                        view.cursor = view.cursor.insert(range);
                    }
                }
                EditKind::Delete => {
                    current_view.cursor = range.from;
                    for view in &mut other_views {
                        view.cursor = view.cursor.remove(range);
                    }
                }
            }
        }
        None
    }
}
