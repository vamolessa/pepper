use crate::buffer::{BufferCollection, BufferHandle, Cursor};

pub struct BufferView {
    pub buffer_handle: BufferHandle,
    pub cursor: Cursor,
    pub size: (u16, u16),
    pub scroll: u16,
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

    pub fn move_cursor(&mut self, buffers: &BufferCollection, offset: (i16, i16)) {
        let buffer = &buffers[self.buffer_handle];
        let cursor = &mut self.cursor;

        let mut target = (
            cursor.column_index as i16 + offset.0,
            cursor.line_index as i16 + offset.1,
        );

        target.1 = target.1.min(buffer.line_count() as i16 - 1).max(0);
        let target_line_len = buffer.line(target.1 as usize).chars().count();
        target.0 = target.0.min(target_line_len as i16).max(0);

        cursor.column_index = target.0 as u16;
        cursor.line_index = target.1 as u16;

        if cursor.line_index < self.scroll {
            self.scroll = cursor.line_index;
        } else if cursor.line_index >= self.scroll + self.size.1 {
            self.scroll = cursor.line_index - self.size.1 + 1;
        }
    }

    pub fn break_line(&mut self, buffers: &mut BufferCollection) {
        let buffer = &mut buffers[self.buffer_handle];
        let cursor = &mut self.cursor;

        buffer.break_line(*cursor);
        cursor.line_index += 1;
        cursor.column_index = 0;
    }

    pub fn insert_text(&mut self, buffers: &mut BufferCollection, text: &str) {
        let buffer = &mut buffers[self.buffer_handle];
        let cursor = &mut self.cursor;

        buffer.insert_text(*cursor, text);
        cursor.column_index += text.chars().count() as u16;
    }
}
