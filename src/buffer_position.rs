use std::{
    cmp::{Ord, Ordering, PartialOrd},
    convert::From,
    ops::{Add, Neg, Sub},
};

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub struct BufferPosition {
    pub column_index: usize,
    pub line_index: usize,
}

impl BufferPosition {
    pub fn insert(&mut self, range: BufferRange) {
        if *self < range.from {
            return;
        }
    }

    pub fn remove(&mut self, range: BufferRange) {
        if *self < range.from {
            return;
        }
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

impl Ord for BufferPosition {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.line_index < other.line_index {
            Ordering::Less
        } else if self.line_index > other.line_index {
            Ordering::Greater
        } else if self.column_index < other.column_index {
            Ordering::Less
        } else if self.column_index > other.column_index {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

impl PartialOrd for BufferPosition {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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
}
