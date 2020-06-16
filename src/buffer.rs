use std::ops::{Index, IndexMut};

use crate::{
    buffer_position::{BufferMovement, BufferPosition, BufferRange},
    undo::{Edit, EditKind, Undo},
};

pub enum Text {
    Char(char),
    String(String),
}

impl Text {
    pub fn as_text_ref<'a>(&'a self) -> TextRef<'a> {
        match self {
            Text::Char(c) => TextRef::Char(*c),
            Text::String(s) => TextRef::Str(&s[..]),
        }
    }
}

pub enum TextRef<'a> {
    Char(char),
    Str(&'a str),
}

impl<'a> TextRef<'a> {
    pub fn to_text(&self) -> Text {
        match self {
            TextRef::Char(c) => Text::Char(*c),
            TextRef::Str(s) => Text::String(s.to_string()),
        }
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

pub struct Buffer {
    lines: Vec<BufferLine>,
    undo: Undo,
}

impl Buffer {
    pub fn from_str(text: &str) -> Self {
        Self {
            lines: text.lines().map(|l| BufferLine::new(l.into())).collect(),
            undo: Undo::new(),
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

    pub fn insert_text(&mut self, position: BufferPosition, text: TextRef) -> BufferMovement {
        let movement = match text {
            TextRef::Char(c) => {
                self.lines[position.line_index]
                    .text
                    .insert(position.column_index, c);
                BufferMovement::ColumnOffset(1)
            }
            TextRef::Str(text) => {
                let split_line = self.lines[position.line_index]
                    .text
                    .split_off(position.column_index);

                let mut line_count = 0;
                let mut lines = text.lines();
                if let Some(line) = lines.next() {
                    self.lines[position.line_index].text.push_str(&line[..]);
                }
                for line in lines {
                    line_count += 1;
                    self.lines
                        .insert(position.line_index, BufferLine::new(line.into()));
                }

                if text.ends_with('\n') {
                    line_count += 1;
                    self.lines
                        .insert(position.line_index, BufferLine::new(split_line));
                    BufferMovement::LineOffsetColumnPosition(line_count, 0)
                } else {
                    let line = &mut self.lines[position.line_index];
                    let column_index = line.char_count();
                    line.text.push_str(&split_line[..]);
                    BufferMovement::LineOffsetColumnPosition(line_count, column_index)
                }
            }
        };

        self.undo
            .push_edit(Edit::new(EditKind::Insert, position, text.to_text()));
        movement
    }

    pub fn delete_range(&mut self, range: BufferRange) {
        let mut text = String::new();
        if range.from.line_index == range.to.line_index {
            text.extend(
                self.lines[range.from.line_index]
                    .text
                    .drain(range.from.column_index..range.to.column_index),
            );
        } else {
            text.extend(
                self.lines[range.from.line_index]
                    .text
                    .drain(range.from.column_index..),
            );
            let lines_range = (range.from.line_index + 1)..range.to.line_index;
            if lines_range.start >= lines_range.end {
                for line in self.lines.drain(lines_range) {
                    text.push_str(&line.text[..]);
                }
            }
            let to_line_index = range.from.line_index + 1;
            if to_line_index < self.lines.len() {
                let to_line = self.lines.remove(to_line_index);
                self.lines[range.from.line_index]
                    .text
                    .push_str(&to_line.text[range.to.column_index..]);
                text.push_str(&to_line.text[..range.to.column_index]);
            }
        }

        self.undo
            .push_edit(Edit::new(EditKind::Delete, range.from, Text::String(text)));
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
