use std::cmp::{Ord, Ordering, PartialOrd};

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct BufferPosition {
    pub line_index: usize,
    pub column_byte_index: usize,
}

impl BufferPosition {
    pub const fn line_col(line_index: usize, column_byte_index: usize) -> Self {
        Self {
            line_index,
            column_byte_index,
        }
    }

    pub fn insert(self, range: BufferRange) -> Self {
        if self.line_index < range.from.line_index {
            self
        } else if self.line_index > range.from.line_index {
            Self {
                column_byte_index: self.column_byte_index,
                line_index: self.line_index + range.to.line_index - range.from.line_index,
            }
        } else if self.column_byte_index < range.from.column_byte_index {
            self
        } else {
            Self {
                column_byte_index: self.column_byte_index + range.to.column_byte_index
                    - range.from.column_byte_index,
                line_index: self.line_index + range.to.line_index - range.from.line_index,
            }
        }
    }

    pub fn delete(self, range: BufferRange) -> Self {
        if self.line_index < range.from.line_index {
            self
        } else if self.line_index > range.to.line_index {
            Self {
                column_byte_index: self.column_byte_index,
                line_index: self.line_index - (range.to.line_index - range.from.line_index),
            }
        } else if self.line_index == range.from.line_index
            && self.column_byte_index < range.from.column_byte_index
        {
            self
        } else if self.line_index == range.to.line_index
            && self.column_byte_index > range.to.column_byte_index
        {
            Self {
                column_byte_index: range.from.column_byte_index + self.column_byte_index
                    - range.to.column_byte_index,
                line_index: range.from.line_index,
            }
        } else {
            range.from
        }
    }
}

impl Ord for BufferPosition {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.line_index < other.line_index {
            Ordering::Less
        } else if self.line_index > other.line_index {
            Ordering::Greater
        } else if self.column_byte_index < other.column_byte_index {
            Ordering::Less
        } else if self.column_byte_index > other.column_byte_index {
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

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
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

    fn pos(line_index: usize, column_byte_index: usize) -> BufferPosition {
        BufferPosition::line_col(line_index, column_byte_index)
    }

    #[test]
    fn buffer_position_insert() {
        let pos12 = pos(1, 2);
        let pos31 = pos(3, 1);
        let pos32 = pos(3, 2);
        let pos33 = pos(3, 3);
        let pos36 = pos(3, 6);
        let pos42 = pos(4, 2);
        let pos53 = pos(5, 3);
        let pos66 = pos(6, 6);
        let range31_33 = BufferRange::between(pos(3, 1), pos(3, 3));
        let range33_51 = BufferRange::between(pos(3, 3), pos(5, 1));

        assert_eq!(pos12, pos12.insert(range31_33));
        assert_eq!(pos(3, 3), pos31.insert(range31_33));
        assert_eq!(pos(3, 4), pos32.insert(range31_33));
        assert_eq!(pos(3, 5), pos33.insert(range31_33));
        assert_eq!(pos42, pos42.insert(range31_33));

        assert_eq!(pos12, pos12.insert(range33_51));
        assert_eq!(pos(5, 1), pos33.insert(range33_51));
        assert_eq!(pos(5, 4), pos36.insert(range33_51));
        assert_eq!(pos(6, 2), pos42.insert(range33_51));
        assert_eq!(pos(7, 3), pos53.insert(range33_51));
        assert_eq!(pos(8, 6), pos66.insert(range33_51));
    }

    #[test]
    fn buffer_position_delete() {
        let pos12 = pos(1, 2);
        let pos31 = pos(3, 1);
        let pos32 = pos(3, 2);
        let pos33 = pos(3, 3);
        let pos36 = pos(3, 6);
        let pos42 = pos(4, 2);
        let pos53 = pos(5, 3);
        let pos66 = pos(6, 6);
        let range31_33 = BufferRange::between(pos(3, 1), pos(3, 3));
        let range33_51 = BufferRange::between(pos(3, 3), pos(5, 1));

        assert_eq!(pos12, pos12.delete(range31_33));
        assert_eq!(pos(3, 1), pos31.delete(range31_33));
        assert_eq!(pos(3, 1), pos32.delete(range31_33));
        assert_eq!(pos(3, 1), pos33.delete(range31_33));
        assert_eq!(pos42, pos42.delete(range31_33));
        assert_eq!(pos53, pos53.delete(range31_33));

        assert_eq!(pos12, pos12.delete(range33_51));
        assert_eq!(pos(3, 3), pos33.delete(range33_51));
        assert_eq!(pos(3, 3), pos36.delete(range33_51));
        assert_eq!(pos(3, 3), pos42.delete(range33_51));
        assert_eq!(pos(3, 5), pos53.delete(range33_51));
        assert_eq!(pos(4, 6), pos66.delete(range33_51));
    }

    #[test]
    fn buffer_range_contains() {
        let range = BufferRange::between(pos(3, 3), pos(5, 5));
        assert_eq!(false, range.contains(pos(1, 4)));
        assert_eq!(false, range.contains(pos(3, 2)));

        assert_eq!(true, range.contains(pos(3, 3)));
        assert_eq!(true, range.contains(pos(3, 7)));
        assert_eq!(true, range.contains(pos(4, 1)));
        assert_eq!(true, range.contains(pos(4, 7)));
        assert_eq!(true, range.contains(pos(5, 1)));
        assert_eq!(true, range.contains(pos(5, 5)));

        assert_eq!(false, range.contains(pos(5, 6)));
        assert_eq!(false, range.contains(pos(7, 2)));

        let range = BufferRange::between(pos(2, 0), pos(2, 0));
        assert_eq!(true, range.contains(pos(2, 0)));
        assert_eq!(false, range.contains(pos(2, 1)));

        let range = BufferRange::between(pos(2, 1), pos(2, 1));
        assert_eq!(false, range.contains(pos(2, 0)));
        assert_eq!(true, range.contains(pos(2, 1)));
    }
}
