use std::{
    convert::From,
    ops::{Add, Neg, Sub},
};

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub struct BufferPosition {
    pub column_index: usize,
    pub line_index: usize,
}

impl BufferPosition {
    pub fn offset_by(self, offset: BufferOffset) -> Self {
        Self {
            column_index: (self.column_index as isize + offset.column_offset) as _,
            line_index: (self.line_index as isize + offset.line_offset) as _,
        }
    }

    pub fn insert(self, range: BufferRange) -> Self {
        self
    }

    pub fn remove(self, range: BufferRange) -> Self {
        self
    }
}

impl From<BufferOffset> for BufferPosition {
    fn from(other: BufferOffset) -> Self {
        Self {
            column_index: other.column_offset.max(0) as _,
            line_index: other.line_offset.max(0) as _,
        }
    }
}

impl Sub for BufferPosition {
    type Output = BufferOffset;

    fn sub(self, other: Self) -> Self::Output {
        BufferOffset::from(self) - BufferOffset::from(other)
    }
}

#[derive(Default, Copy, Clone)]
pub struct BufferOffset {
    pub column_offset: isize,
    pub line_offset: isize,
}

impl From<BufferPosition> for BufferOffset {
    fn from(other: BufferPosition) -> Self {
        Self {
            column_offset: other.column_index as _,
            line_offset: other.line_index as _,
        }
    }
}

impl Add for BufferOffset {
    type Output = Self;

    fn add(mut self, other: Self) -> Self::Output {
        self.column_offset += other.column_offset;
        self.line_offset += other.line_offset;
        self
    }
}

impl Sub for BufferOffset {
    type Output = Self;

    fn sub(mut self, other: Self) -> Self::Output {
        self.column_offset -= other.column_offset;
        self.line_offset -= other.line_offset;
        self
    }
}

impl Neg for BufferOffset {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            column_offset: -self.column_offset,
            line_offset: -self.line_offset,
        }
    }
}

#[derive(Default, Copy, Clone)]
pub struct BufferRange {
    pub from: BufferPosition,
    pub to: BufferPosition,
    __: (),
}

impl BufferRange {
    pub fn between(from: BufferPosition, to: BufferPosition) -> Self {
        let (from, to) = if from.line_index > to.line_index
            || from.line_index == to.line_index && from.column_index > to.column_index
        {
            (to, from)
        } else {
            (from, to)
        };

        Self { from, to, __: () }
    }

    pub fn from_str_position(position: BufferPosition, text: &str) -> Self {
        let mut line_count = 0;
        let mut last_line_char_count = 0;
        for line in text.lines() {
            line_count += 1;
            last_line_char_count = line.chars().count();
        }
        if text.ends_with('\n') {
            line_count += 1;
            last_line_char_count = 0;
        }

        let to = if line_count > 1 {
            BufferPosition {
                line_index: position.line_index + line_count,
                column_index: last_line_char_count,
            }
        } else {
            BufferPosition {
                line_index: position.line_index,
                column_index: position.column_index + last_line_char_count,
            }
        };

        Self {
            from: position,
            to,
            __: (),
        }
    }
}
