use std::ops::{Index, IndexMut};

use crate::undo::Undo;

#[derive(Default, Copy, Clone)]
pub struct BufferPosition {
    pub column_index: usize,
    pub line_index: usize,
}

pub struct BufferRange {
    pub from: BufferPosition,
    pub to: BufferPosition,
    __: (),
}

impl BufferRange {
    pub fn new(from: BufferPosition, mut to: BufferPosition) -> Self {
        to = if (to.line_index > from.line_index)
            || (to.line_index == from.line_index && to.column_index >= from.column_index)
        {
            to
        } else {
            from
        };

        Self { from, to, __: () }
    }
}

pub struct BufferLine {
    pub text: String,
}

impl BufferLine {
    pub fn new(text: String) -> Self {
        Self { text }
    }

    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}

#[derive(Default)]
pub struct Buffer {
    lines: Vec<BufferLine>,
    undo: Undo,
}

impl Buffer {
    pub fn from_str(text: &str) -> Self {
        Self {
            lines: text.lines().map(|l| BufferLine::new(l.into())).collect(),
            undo: Default::default(),
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn lines(&self) -> impl Iterator<Item = &BufferLine> {
        self.lines.iter()
    }

    pub fn line(&self, index: usize) -> &BufferLine {
        &self.lines[index]
    }

    pub fn insert_text(&mut self, mut position: BufferPosition, text: &str) -> BufferPosition {
        let split_line = self.lines[position.line_index]
            .text
            .split_off(position.column_index);

        let mut lines = text.lines();
        if let Some(line) = lines.next() {
            self.lines[position.line_index].text.push_str(&line[..]);
        }
        for line in lines {
            position.line_index += 1;
            self.lines
                .insert(position.line_index, BufferLine::new(line.into()));
        }

        if text.ends_with('\n') {
            position.column_index = 0;
            position.line_index += 1;
            self.lines
                .insert(position.line_index, BufferLine::new(split_line));
        } else {
            let line = &mut self.lines[position.line_index];
            position.column_index = line.char_count();
            line.text.push_str(&split_line[..]);
        }

        position
    }

    pub fn delete_range(&mut self, range: BufferRange) {
        if range.from.line_index == range.to.line_index {
            self.lines[range.from.line_index]
                .text
                .drain(range.from.column_index..range.to.column_index);
        } else {
            self.lines[range.from.line_index]
                .text
                .truncate(range.from.column_index);
            let lines_range = (range.from.line_index + 1)..range.to.line_index;
            if lines_range.start >= lines_range.end {
                self.lines.drain(lines_range);
            }
            let to_line_index = range.from.line_index + 1;
            if to_line_index < self.lines.len() {
                let to_line = self.lines.remove(to_line_index);
                self.lines[range.from.line_index]
                    .text
                    .push_str(&to_line.text[range.to.column_index..]);
            }
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
