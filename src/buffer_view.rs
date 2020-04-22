use crate::buffer::{Buffer, BufferCollection, BufferHandle};

#[derive(Default, Copy, Clone)]
pub struct Cursor {
    pub line_index: u16,
    pub column_index: u16,
}

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
        if (self.cursor.line_index as usize) < buffer.lines.len() - 1 {
            self.cursor.line_index += 1;
            let line_len = buffer.lines[self.cursor.line_index as usize]
                .chars()
                .count();
            self.cursor.column_index = self.cursor.column_index.min(line_len as u16);
            self.frame_cursor(self.cursor);
        }
    }

    pub fn move_cursor_up(&mut self, buffers: &BufferCollection) {
        if self.cursor.line_index > 0 {
            self.cursor.line_index -= 1;
            let line_len = self.buffer(buffers).lines[self.cursor.line_index as usize]
                .chars()
                .count();
            self.cursor.column_index = self.cursor.column_index.min(line_len as u16);
            self.frame_cursor(self.cursor);
        }
    }

    pub fn move_cursor_right(&mut self, buffers: &BufferCollection) {
        let line_index = self.cursor.line_index as usize;
        let line_len = self.buffer(buffers).lines[line_index].chars().count();
        if line_index < line_len {
            self.cursor.column_index += 1;
            self.frame_cursor(self.cursor);
        }
    }

    fn frame_cursor(&mut self, cursor: Cursor) {
        if cursor.line_index < self.scroll {
            self.scroll = cursor.line_index;
        } else if cursor.line_index >= self.scroll + self.size.1 {
            self.scroll = cursor.line_index - self.size.1 + 1;
        }
    }
}
