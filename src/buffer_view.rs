use crate::{
    buffer::{BufferCollection, BufferHandle, TextRef},
    buffer_position::{BufferOffset, BufferPosition, BufferRange},
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
            .min(buffer.line_count() as isize - 1)
            .max(0);
        let target_line_len = buffer.line(target.line_offset as _).char_count();
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

        let movement = buffer.insert_text(*cursor, text);
        *cursor = cursor.move_by(movement);
    }

    pub fn delete_selection(&mut self, buffers: &mut BufferCollection) {
        let buffer = &mut buffers[self.buffer_handle];
        let cursor = &mut self.cursor;

        let cursor_line_size = buffer.line(cursor.line_index).char_count();
        let mut selection_end = *cursor;
        if selection_end.column_index < cursor_line_size {
            selection_end.column_index += 1;
        } else {
            selection_end.line_index += 1;
            selection_end.column_index = 0;
        }

        buffer.delete_range(BufferRange::between(*cursor, selection_end));
    }

    pub fn undo(&mut self, buffers: &mut BufferCollection) {}

    pub fn redo(&mut self, buffers: &mut BufferCollection) {}
}
