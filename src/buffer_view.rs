use crate::buffer::{Buffer, BufferCollection, BufferHandle};

#[derive(Default)]
pub struct Cursor {
    pub line_index: usize,
    pub column_index: usize,
}

pub struct BufferView {
    pub buffer_handle: BufferHandle,
    pub cursor: Cursor,
    pub size: (u16, u16),
    pub scroll: u16
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

    pub fn buffer<'a>(&self, buffers: &'a BufferCollection) -> &'a Buffer {
        &buffers[self.buffer_handle]
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor.column_index > 0 {
            self.cursor.column_index -= 1;
        }
    }

    pub fn move_cursor_down(&mut self, buffers: &BufferCollection) {
        let buffer = self.buffer(buffers);
        if self.cursor.line_index < buffer.lines.len() - 1 {
            self.cursor.line_index += 1;
            let line_len = buffer.lines[self.cursor.line_index].chars().count();
            self.cursor.column_index = self.cursor.column_index.min(line_len);
        }
    }

    pub fn move_cursor_up(&mut self, buffers: &BufferCollection) {
        if self.cursor.line_index > 0 {
            self.cursor.line_index -= 1;
            let line_len = self.buffer(buffers).lines[self.cursor.line_index]
                .chars()
                .count();
            self.cursor.column_index = self.cursor.column_index.min(line_len);
        }
    }

    pub fn move_cursor_right(&mut self, buffers: &BufferCollection) {
        let line_len = self.buffer(buffers).lines[self.cursor.line_index]
            .chars()
            .count();
        if self.cursor.column_index < line_len {
            self.cursor.column_index += 1;
        }
    }
}
