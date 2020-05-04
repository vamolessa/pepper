use std::ops::{Index, IndexMut};

#[derive(Default, Copy, Clone)]
pub struct Cursor {
    pub column_index: u16,
    pub line_index: u16,
}

#[derive(Default)]
pub struct Buffer {
    lines: Vec<String>,
}

impl Buffer {
    pub fn from_str(text: &str) -> Self {
        Self {
            lines: text.lines().map(Into::into).collect(),
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn lines(&self) -> impl Iterator<Item = &String> {
        self.lines.iter()
    }

    pub fn line(&self, index: usize) -> &String {
        &self.lines[index]
    }

    pub fn break_line(&mut self, cursor: Cursor) {
        let line = &mut self.lines[cursor.line_index as usize];
        let new_line = line.split_off(cursor.column_index as usize);
        self.lines.insert(cursor.line_index as usize + 1, new_line);
    }

    pub fn insert_text(&mut self, cursor: Cursor, text: &str) {
        self.lines[cursor.line_index as usize].insert_str(cursor.column_index as usize, text);
    }
}

#[derive(Clone, Copy)]
pub struct BufferHandle(usize);

#[derive(Default)]
pub struct BufferCollection {
    buffers: Vec<Buffer>,
    free_slots: Vec<BufferHandle>,
}

impl BufferCollection {
    pub fn add(&mut self, buffer: Buffer) -> BufferHandle {
        if let Some(handle) = self.free_slots.pop() {
            self.buffers[handle.0] = buffer;
            handle
        } else {
            let index = self.buffers.len();
            self.buffers.push(buffer);
            BufferHandle(index)
        }
    }
}

impl Index<BufferHandle> for BufferCollection {
    type Output = Buffer;
    fn index(&self, handle: BufferHandle) -> &Self::Output {
        &self.buffers[handle.0]
    }
}

impl IndexMut<BufferHandle> for BufferCollection {
    fn index_mut(&mut self, handle: BufferHandle) -> &mut Self::Output {
        &mut self.buffers[handle.0]
    }
}
