use std::{
    cmp::{Ord, PartialOrd, Ordering},
    ops::{Index, IndexMut, Range}
};

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub struct BufferPosition {
    pub column_index: usize,
    pub line_index: usize,
}
impl Ord for BufferPosition {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.line_index.cmp(&other.line_index) {
            Ordering::Equal => self.column_index.cmp(&other.column_index),
            ordering => ordering,
        }
    }
}
impl PartialOrd for BufferPosition {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
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

    pub fn insert_text(&mut self, mut position: BufferPosition, text: &str) -> BufferPosition {
        let split_line = self.lines[position.line_index].split_off(position.column_index);

        let mut lines = text.lines();
        if let Some(line) = lines.next() {
            self.lines[position.line_index].push_str(line);
        }
        for line in lines {
            position.line_index += 1;
            self.lines.insert(position.line_index, line.into());
        }

        if text.ends_with('\n') {
            position.column_index = 0;
            position.line_index += 1;
            self.lines.insert(position.line_index, split_line);
        } else {
            let line = &mut self.lines[position.line_index];
            position.column_index = line.len();
            line.push_str(&split_line[..]);
        }

        position
    }

    pub fn delete_range(&mut self, range: Range<BufferPosition>) {
        if range.start.line_index == range.end.line_index {
            self.lines[range.start.line_index].drain(range.start.column_index..range.end.column_index);
        } else {
            self.lines[range.start.line_index].truncate(range.start.column_index);
            let lines_range = (range.start.line_index + 1)..(range.end.line_index - 1);
            if lines_range.start >= lines_range.end {
                self.lines.drain(lines_range);
            }
            self.lines[range.start.line_index + 1].drain(..range.end.column_index);
        }
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
