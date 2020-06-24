use std::{
    cmp::{Ord, Ordering, PartialOrd},
    convert::From,
    ops::{Add, Neg, Sub},
};

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct BufferPosition {
    pub line_index: usize,
    pub column_index: usize,
}

impl BufferPosition {
    pub fn line_col(line_index: usize, column_index: usize) -> Self {
        Self {
            line_index,
            column_index,
        }
    }

    pub fn insert(self, range: BufferRange) -> Self {
        if self.line_index < range.from.line_index {
            self
        } else if self.line_index > range.from.line_index {
            Self {
                column_index: self.column_index,
                line_index: self.line_index + range.to.line_index - range.from.line_index,
            }
        } else if self.column_index < range.from.column_index {
            self
        } else {
            Self {
                column_index: self.column_index + range.to.column_index - range.from.column_index,
                line_index: self.line_index + range.to.line_index - range.from.line_index,
            }
        }
    }

    pub fn remove(self, range: BufferRange) -> Self {
        if self.line_index < range.from.line_index {
            self
        } else if self.line_index > range.to.line_index {
            Self {
                column_index: self.column_index,
                line_index: self.line_index - (range.to.line_index - range.from.line_index),
            }
        } else if self.line_index == range.from.line_index
            && self.column_index < range.from.column_index
        {
            self
        } else if self.line_index == range.to.line_index
            && self.column_index > range.to.column_index
        {
            Self {
                column_index: range.from.column_index + self.column_index - range.to.column_index,
                line_index: range.from.line_index,
            }
        } else {
            range.from
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

impl Ord for BufferPosition {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.line_index < other.line_index {
            Ordering::Less
        } else if self.line_index > other.line_index {
            Ordering::Greater
        } else if self.column_index < other.line_index {
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

impl Sub for BufferPosition {
    type Output = BufferOffset;

    fn sub(self, other: Self) -> Self::Output {
        BufferOffset::from(self) - BufferOffset::from(other)
    }
}

#[derive(Default, Copy, Clone)]
pub struct BufferOffset {
    pub line_offset: isize,
    pub column_offset: isize,
}

impl BufferOffset {
    pub fn line_col(line_offset: isize, column_offset: isize) -> Self {
        Self {
            line_offset,
            column_offset,
        }
    }
}

impl From<BufferPosition> for BufferOffset {
    fn from(other: BufferPosition) -> Self {
        Self {
            line_offset: other.line_index as _,
            column_offset: other.column_index as _,
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

#[derive(Debug, Default, Copy, Clone)]
pub struct BufferRange {
    pub from: BufferPosition,
    pub to: BufferPosition,
    __: (),
}

impl BufferRange {
    pub fn between(from: BufferPosition, to: BufferPosition) -> Self {
        let (from, to) = if from <= to { (from, to) } else { (to, from) };
        Self { from, to, __: () }
    }

    pub fn contains(&self, position: BufferPosition) -> bool {
        self.from <= position && position <= self.to
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pos(line_index: usize, column_index: usize) -> BufferPosition {
        BufferPosition {
            line_index,
            column_index,
        }
    }

    #[test]
    fn buffer_position_insert() {
        let pos12 = make_pos(1, 2);
        let pos31 = make_pos(3, 1);
        let pos32 = make_pos(3, 2);
        let pos33 = make_pos(3, 3);
        let pos36 = make_pos(3, 6);
        let pos42 = make_pos(4, 2);
        let pos53 = make_pos(5, 3);
        let pos66 = make_pos(6, 6);
        let range31_33 = BufferRange::between(make_pos(3, 1), make_pos(3, 3));
        let range33_51 = BufferRange::between(make_pos(3, 3), make_pos(5, 1));

        assert_eq!(pos12, pos12.insert(range31_33));
        assert_eq!(make_pos(3, 3), pos31.insert(range31_33));
        assert_eq!(make_pos(3, 4), pos32.insert(range31_33));
        assert_eq!(make_pos(3, 5), pos33.insert(range31_33));
        assert_eq!(pos42, pos42.insert(range31_33));

        assert_eq!(pos12, pos12.insert(range33_51));
        assert_eq!(make_pos(5, 1), pos33.insert(range33_51));
        assert_eq!(make_pos(5, 4), pos36.insert(range33_51));
        assert_eq!(make_pos(6, 2), pos42.insert(range33_51));
        assert_eq!(make_pos(7, 3), pos53.insert(range33_51));
        assert_eq!(make_pos(8, 6), pos66.insert(range33_51));
    }

    #[test]
    fn buffer_position_remove() {
        let pos12 = make_pos(1, 2);
        let pos31 = make_pos(3, 1);
        let pos32 = make_pos(3, 2);
        let pos33 = make_pos(3, 3);
        let pos36 = make_pos(3, 6);
        let pos42 = make_pos(4, 2);
        let pos53 = make_pos(5, 3);
        let pos66 = make_pos(6, 6);
        let range31_33 = BufferRange::between(make_pos(3, 1), make_pos(3, 3));
        let range33_51 = BufferRange::between(make_pos(3, 3), make_pos(5, 1));

        assert_eq!(pos12, pos12.remove(range31_33));
        assert_eq!(make_pos(3, 1), pos31.remove(range31_33));
        assert_eq!(make_pos(3, 1), pos32.remove(range31_33));
        assert_eq!(make_pos(3, 1), pos33.remove(range31_33));
        assert_eq!(pos42, pos42.remove(range31_33));
        assert_eq!(pos53, pos53.remove(range31_33));

        assert_eq!(pos12, pos12.remove(range33_51));
        assert_eq!(make_pos(3, 3), pos33.remove(range33_51));
        assert_eq!(make_pos(3, 3), pos36.remove(range33_51));
        assert_eq!(make_pos(3, 3), pos42.remove(range33_51));
        assert_eq!(make_pos(3, 5), pos53.remove(range33_51));
        assert_eq!(make_pos(4, 6), pos66.remove(range33_51));
    }

    #[test]
    fn buffer_range_contains() {
        let range = BufferRange::between(make_pos(3, 3), make_pos(5, 5));

        assert_eq!(false, range.contains(make_pos(1, 4)));
        assert_eq!(false, range.contains(make_pos(3, 2)));

        assert_eq!(true, range.contains(make_pos(3, 3)));
        assert_eq!(true, range.contains(make_pos(3, 7)));
        assert_eq!(true, range.contains(make_pos(4, 1)));
        assert_eq!(true, range.contains(make_pos(4, 7)));
        assert_eq!(true, range.contains(make_pos(5, 1)));
        assert_eq!(true, range.contains(make_pos(5, 5)));

        assert_eq!(false, range.contains(make_pos(5, 6)));
        assert_eq!(false, range.contains(make_pos(7, 2)));
    }
}
