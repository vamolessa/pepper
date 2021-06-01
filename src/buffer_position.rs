use std::{
    cmp::{Ord, Ordering, PartialOrd},
    fmt,
    str::FromStr,
};

pub type BufferPositionIndex = u32;

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub struct BufferPosition {
    pub line_index: BufferPositionIndex,
    pub column_byte_index: BufferPositionIndex,
}

impl BufferPosition {
    pub const fn zero() -> Self {
        Self {
            line_index: 0,
            column_byte_index: 0,
        }
    }

    pub const fn line_col(
        line_index: BufferPositionIndex,
        column_byte_index: BufferPositionIndex,
    ) -> Self {
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

impl fmt::Debug for BufferPosition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "BufferPosition(line: {}, col: {})",
            self.line_index, self.column_byte_index,
        )
    }
}

impl fmt::Display for BufferPosition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{},{}", self.line_index, self.column_byte_index)
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

impl FromStr for BufferPosition {
    type Err = ();
    fn from_str(s: &str) -> Result<BufferPosition, Self::Err> {
        #[inline]
        fn is_non_ascii_digit(c: char) -> bool {
            !c.is_ascii_digit()
        }

        let i = s.find(is_non_ascii_digit).unwrap_or(s.len());
        let (line, s) = s.split_at(i);
        let line = match line.parse::<BufferPositionIndex>() {
            Ok(line) => line.saturating_sub(1),
            Err(_) => return Err(()),
        };

        let mut chars = s.chars();
        if !matches!(chars.next(), Some(',')) {
            return Ok(BufferPosition::line_col(line, 0));
        }
        let s = chars.as_str();

        let i = s.find(is_non_ascii_digit).unwrap_or(s.len());
        let column = match s[..i].parse::<BufferPositionIndex>() {
            Ok(n) => n.saturating_sub(1),
            Err(_) => return Ok(BufferPosition::line_col(line, 0)),
        };
        Ok(BufferPosition::line_col(line, column))
    }
}

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub struct BufferRange {
    pub from: BufferPosition,
    pub to: BufferPosition,
    __: (),
}

impl BufferRange {
    pub const fn zero() -> Self {
        Self {
            from: BufferPosition::zero(),
            to: BufferPosition::zero(),
            __: (),
        }
    }

    pub fn between(from: BufferPosition, to: BufferPosition) -> Self {
        let (from, to) = if from <= to { (from, to) } else { (to, from) };
        Self { from, to, __: () }
    }
}

impl fmt::Debug for BufferRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "BufferRange(line: {}, col: {} => line: {}, col: {})",
            self.from.line_index,
            self.from.column_byte_index,
            self.to.line_index,
            self.to.column_byte_index
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(
        line_index: BufferPositionIndex,
        column_byte_index: BufferPositionIndex,
    ) -> BufferPosition {
        BufferPosition::line_col(line_index, column_byte_index)
    }

    #[test]
    fn buffer_position_comparison() {
        assert!(pos(0, 0) < pos(0, 9));
        assert!(pos(0, 0) < pos(0, 14));
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
    fn buffer_position_parsing() {
        assert_eq!(Err(()), "".parse::<BufferPosition>());
        assert_eq!(Err(()), ",".parse::<BufferPosition>());
        assert_eq!(Err(()), "a,".parse::<BufferPosition>());
        assert_eq!(Err(()), ",b".parse::<BufferPosition>());
        assert_eq!(Err(()), "a,b".parse::<BufferPosition>());

        assert_eq!(Ok(pos(0, 0)), "0".parse());
        assert_eq!(Ok(pos(0, 0)), "1".parse());
        assert_eq!(Ok(pos(1, 0)), "2".parse());
        assert_eq!(Ok(pos(98, 0)), "99".parse());

        assert_eq!(Ok(pos(0, 0)), "0,0".parse());
        assert_eq!(Ok(pos(0, 0)), "1,1".parse());
        assert_eq!(Ok(pos(3, 1)), "4,2".parse());
        assert_eq!(Ok(pos(98, 98)), "99,99".parse());

        assert_eq!(Ok(pos(3, 0)), "4,".parse());
        assert_eq!(Ok(pos(3, 0)), "4,x".parse());
        assert_eq!(Ok(pos(3, 8)), "4,9xx".parse());
        assert_eq!(Ok(pos(3, 8)), "4,9,xx".parse());
    }
}
