use std::{convert::From, fmt, ops::AddAssign};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    Pending(usize, PatternState),
    Ok(usize),
    Err,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PatternError {
    UnexpectedEndOfPattern,
    Expected(char),
    InvalidEscaping(char),
    Unescaped(char),
    EmptyGroup,
    GroupWithElementsOfDifferentSize,
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::UnexpectedEndOfPattern => write!(f, "unexpected end of pattern"),
            Self::Expected(c) => write!(f, "expected character '{}'", c),
            Self::InvalidEscaping(c) => write!(f, "invalid escaping '%{}'", c),
            Self::Unescaped(c) => write!(f, "unescaped character '{}'", c),
            Self::EmptyGroup => write!(f, "empty pattern group"),
            Self::GroupWithElementsOfDifferentSize => {
                write!(f, "pattern group has elements of different size")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternState {
    op_index: usize,
}

#[derive(Clone)]
pub struct Pattern {
    ops: Vec<Op>,
    start_jump: Jump,
}

impl Pattern {
    pub fn new(pattern: &str) -> Result<Self, PatternError> {
        Ok(PatternCompiler::new(pattern.as_bytes(), Vec::new()).compile()?)
    }

    pub fn matches(&self, text: &str) -> MatchResult {
        self.matches_with_state(
            text,
            &PatternState {
                op_index: self.start_jump.0 as _,
            },
        )
    }

    pub fn matches_with_state(&self, text: &str, state: &PatternState) -> MatchResult {
        let bytes = text.as_bytes();
        let ops = &self.ops;
        let mut op_index = state.op_index;
        let mut bytes_index = 0;

        macro_rules! check {
            ($e:expr, $okj:expr, $erj:expr) => {{
                if bytes_index < bytes.len() && $e {
                    op_index = $okj.0 as _;
                    bytes_index += 1;
                } else {
                    op_index = $erj.0 as _;
                }
            }};
        };

        loop {
            match ops[op_index] {
                Op::Ok => return MatchResult::Ok(bytes_index),
                Op::Error => return MatchResult::Err,
                Op::Unwind(jump, len) => {
                    bytes_index -= len.0 as usize;
                    op_index = jump.0 as _;
                }
                Op::EndAnchor(okj, erj) => {
                    if bytes_index < bytes.len() {
                        op_index = erj.0 as _;
                    } else {
                        op_index = okj.0 as _;
                        return match ops[op_index] {
                            Op::Ok => MatchResult::Ok(bytes_index),
                            _ => MatchResult::Pending(bytes_index, PatternState { op_index }),
                        };
                    }
                }
                Op::SkipOne(okj, erj) => check!(true, okj, erj),
                Op::SkipMany(okj, erj, len) => {
                    let len = len.0 as usize;
                    bytes_index += len;
                    if bytes_index <= bytes.len() {
                        op_index = okj.0 as _;
                    } else {
                        bytes_index -= len;
                        op_index = erj.0 as _;
                    }
                }
                Op::Alphabetic(okj, erj) => {
                    check!(bytes[bytes_index].is_ascii_alphabetic(), okj, erj)
                }
                Op::Lower(okj, erj) => check!(bytes[bytes_index].is_ascii_lowercase(), okj, erj),
                Op::Upper(okj, erj) => check!(bytes[bytes_index].is_ascii_uppercase(), okj, erj),
                Op::Digit(okj, erj) => check!(bytes[bytes_index].is_ascii_digit(), okj, erj),
                Op::Alphanumeric(okj, erj) => {
                    check!(bytes[bytes_index].is_ascii_alphanumeric(), okj, erj)
                }
                Op::Byte(okj, erj, b) => check!(bytes[bytes_index] == b, okj, erj),
                Op::Bytes3(okj, erj, bs) => {
                    let start_index = bytes_index;
                    bytes_index += 3;
                    if bytes_index <= bytes.len() {
                        let slice = &bytes[start_index..bytes_index];
                        if slice[0] == bs[0] && slice[1] == bs[1] && slice[2] == bs[2] {
                            op_index = okj.0 as _;
                        } else {
                            bytes_index = start_index;
                            op_index = erj.0 as _;
                        }
                    } else {
                        bytes_index = start_index;
                        op_index = erj.0 as _;
                    }
                }
            };
        }
    }
}

impl fmt::Debug for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut op_digit_count = 0;
        let mut op_count = self.ops.len();
        while op_count > 0 {
            op_count /= 10;
            op_digit_count += 1;
        }

        f.write_str("Pattern {\n")?;
        for (i, op) in self.ops.iter().enumerate() {
            if i == self.start_jump.0 as _ {
                f.write_fmt(format_args!("  > [{:width$}] ", i, width = op_digit_count))?;
            } else {
                f.write_fmt(format_args!("    [{:width$}] ", i, width = op_digit_count))?;
            }

            fmt::Debug::fmt(op, f)?;
            f.write_str("\n")?;
        }
        f.write_str("}\n")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct Length(u16);
impl From<usize> for Length {
    fn from(value: usize) -> Self {
        Self(value as _)
    }
}
impl AddAssign for Length {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

#[derive(Debug, Clone, Copy)]
struct Jump(u16);
impl From<usize> for Jump {
    fn from(value: usize) -> Self {
        Self(value as _)
    }
}
impl AddAssign for Jump {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

#[derive(Clone, Copy)]
enum JumpFrom {
    Beginning(Jump),
    End(Jump),
}

#[derive(Clone)]
enum Op {
    Ok,
    Error,
    Unwind(Jump, Length),
    EndAnchor(Jump, Jump),
    SkipOne(Jump, Jump),
    SkipMany(Jump, Jump, Length),
    Alphabetic(Jump, Jump),
    Lower(Jump, Jump),
    Upper(Jump, Jump),
    Digit(Jump, Jump),
    Alphanumeric(Jump, Jump),
    Byte(Jump, Jump, u8),
    Bytes3(Jump, Jump, [u8; 3]),
}

impl fmt::Debug for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const WIDTH: usize = 14;
        macro_rules! p {
            ($name:expr, $okj:expr, $erj:expr) => {
                f.write_fmt(format_args!(
                    "{:width$}{} {}",
                    $name,
                    $okj.0,
                    $erj.0,
                    width = WIDTH
                ));
            };
        }

        match self {
            Op::Ok => f.write_str("Ok"),
            Op::Error => f.write_str("Error"),
            Op::Unwind(jump, len) => f.write_fmt(format_args!(
                "{:width$}[{}] {}",
                "Unwind",
                len.0,
                jump.0,
                width = WIDTH - 4
            )),
            Op::EndAnchor(okj, erj) => p!("EndAnchor", okj, erj),
            Op::SkipOne(okj, erj) => p!("SkipOne", okj, erj),
            Op::SkipMany(okj, erj, len) => f.write_fmt(format_args!(
                "{:width$}[{}] {} {}",
                "SkipMany",
                len.0,
                okj.0,
                erj.0,
                width = WIDTH - 4
            )),
            Op::Alphabetic(okj, erj) => p!("Alphabetic", okj, erj),
            Op::Lower(okj, erj) => p!("Lower", okj, erj),
            Op::Upper(okj, erj) => p!("Upper", okj, erj),
            Op::Digit(okj, erj) => p!("Digit", okj, erj),
            Op::Alphanumeric(okj, erj) => p!("Alphanumeric", okj, erj),
            Op::Byte(okj, erj, byte) => f.write_fmt(format_args!(
                "{:width$}'{}' {} {}",
                "Byte",
                *byte as char,
                okj.0,
                erj.0,
                width = WIDTH - 4
            )),
            Op::Bytes3(okj, erj, bytes) => f.write_fmt(format_args!(
                "{:width$}'{}','{}','{}' {} {}",
                "Bytes3",
                bytes[0] as char,
                bytes[1] as char,
                bytes[2] as char,
                okj.0,
                erj.0,
                width = WIDTH - 4
            )),
        }
    }
}

struct PatternCompiler<'a> {
    pub bytes: &'a [u8],
    pub index: usize,
    pub start_jump: Jump,
    pub ops: Vec<Op>,
}

impl<'a> PatternCompiler<'a> {
    pub fn new(bytes: &'a [u8], mut ops_buf: Vec<Op>) -> Self {
        ops_buf.clear();
        Self {
            bytes,
            index: 0,
            start_jump: Jump(1),
            ops: ops_buf,
        }
    }

    pub fn compile(mut self) -> Result<Pattern, PatternError> {
        self.ops.push(Op::Error);
        while let Ok(_) = self.next() {
            self.parse_stmt()?;
        }
        self.ops.push(Op::Ok);
        self.optimize();

        Ok(Pattern {
            ops: self.ops,
            start_jump: self.start_jump,
        })
    }

    fn peek(&self) -> Result<u8, PatternError> {
        if self.index < self.bytes.len() {
            Ok(self.bytes[self.index])
        } else {
            Err(PatternError::UnexpectedEndOfPattern)
        }
    }

    fn current(&self) -> u8 {
        self.bytes[self.index - 1]
    }

    fn assert_current(&self, byte: u8) -> Result<(), PatternError> {
        if self.current() == byte {
            Ok(())
        } else {
            Err(PatternError::Expected(byte as char))
        }
    }

    fn next(&mut self) -> Result<u8, PatternError> {
        self.peek().and_then(|b| {
            self.index += 1;
            Ok(b)
        })
    }

    fn next_is_not(&mut self, byte: u8) -> Result<bool, PatternError> {
        Ok(self.next()? != byte)
    }

    fn parse_stmt(&mut self) -> Result<(), PatternError> {
        match self.current() {
            b'{' => self.parse_repeat_stmt(),
            _ => self
                .parse_expr(JumpFrom::End(Jump(0)), JumpFrom::Beginning(Jump(0)))
                .map(|_| ()),
        }
    }

    fn parse_expr(&mut self, okj: JumpFrom, erj: JumpFrom) -> Result<Length, PatternError> {
        let len = match self.current() {
            b'(' => self.parse_sequence_expr(okj, erj)?,
            b'[' => self.parse_group_expr(okj, erj)?,
            _ => self.parse_class_expr(okj, erj)?,
        };

        Ok(len)
    }

    fn get_absolute_jump(&mut self, jump: JumpFrom) -> Jump {
        match jump {
            JumpFrom::Beginning(jump) => jump,
            JumpFrom::End(_) => {
                let jump = (self.ops.len() + 2).into();
                self.ops.push(Op::Unwind(jump, Length(0)));
                let jump = self.ops.len().into();
                self.ops.push(Op::Unwind(jump, Length(0)));
                jump
            }
        }
    }

    fn patch_jump(&mut self, jump: JumpFrom, abs_jump: Jump) {
        if let JumpFrom::End(mut jump) = jump {
            jump += self.ops.len().into();
            if let Op::Unwind(j, Length(0)) = &mut self.ops[abs_jump.0 as usize] {
                *j = jump;
            } else {
                unreachable!();
            }
        }
    }

    fn jump_at_end(&mut self, jump: JumpFrom) {
        match jump {
            JumpFrom::Beginning(jump) => self.ops.push(Op::Unwind(jump, Length(0))),
            JumpFrom::End(Jump(0)) => (),
            JumpFrom::End(mut jump) => {
                jump += (self.ops.len() + 1).into();
                self.ops.push(Op::Unwind(jump, Length(0)));
            }
        }
    }

    fn skip(&mut self, okj: Jump, erj: Jump, len: Length) {
        match len {
            Length(0) => self.ops.push(Op::Unwind(okj, Length(0))),
            Length(1) => self.ops.push(Op::SkipOne(okj, erj)),
            _ => self.ops.push(Op::SkipMany(okj, erj, len)),
        }
    }

    fn parse_repeat_stmt(&mut self) -> Result<(), PatternError> {
        let start_jump = self.ops.len().into();
        let end_jump = self.get_absolute_jump(JumpFrom::End(Jump(0)));

        let mut has_cancel_pattern = false;
        while self.next_is_not(b'}')? {
            match self.current() {
                b'!' => {
                    self.next()?;
                    self.parse_expr(JumpFrom::Beginning(end_jump), JumpFrom::End(Jump(0)))?;
                    has_cancel_pattern = true;
                }
                _ => {
                    self.parse_expr(JumpFrom::Beginning(start_jump), JumpFrom::End(Jump(0)))?;
                }
            }
        }

        if has_cancel_pattern {
            self.ops.push(Op::Unwind(Jump(0), Length(0)));
        }

        self.patch_jump(JumpFrom::End(Jump(0)), end_jump);

        self.assert_current(b'}')?;
        Ok(())
    }

    fn parse_sequence_expr(
        &mut self,
        okj: JumpFrom,
        erj: JumpFrom,
    ) -> Result<Length, PatternError> {
        let inverse = self.peek()? == b'!';
        let mut len = Length(0);

        if inverse {
            self.next()?;

            let abs_erj = self.get_absolute_jump(erj);
            while self.next_is_not(b')')? {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(2)), JumpFrom::End(Jump(0)))?;
                self.skip(
                    (self.ops.len() + 3).into(),
                    (self.ops.len() + 1).into(),
                    expr_len,
                );
                self.ops.push(Op::Unwind(abs_erj, len));
                len += expr_len;
            }
            self.ops.push(Op::Unwind(abs_erj, len));
            self.jump_at_end(okj);
            self.patch_jump(erj, abs_erj);
        } else {
            let abs_erj = self.get_absolute_jump(erj);
            while self.next_is_not(b')')? {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(1)), JumpFrom::End(Jump(0)))?;
                self.ops.push(Op::Unwind(abs_erj, len));
                len += expr_len;
            }
            self.jump_at_end(okj);
            self.patch_jump(erj, abs_erj);
        }

        self.assert_current(b')')?;
        Ok(len)
    }

    fn parse_group_expr(&mut self, okj: JumpFrom, erj: JumpFrom) -> Result<Length, PatternError> {
        let inverse = self.peek()? == b'!';
        let mut len = None;

        if inverse {
            self.next()?;

            let abs_erj = self.get_absolute_jump(erj);
            while self.next_is_not(b']')? {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(0)), JumpFrom::End(Jump(1)))?;
                self.ops.push(Op::Unwind(abs_erj, expr_len));

                if len.unwrap_or(expr_len).0 != expr_len.0 {
                    return Err(PatternError::GroupWithElementsOfDifferentSize);
                }
                len = Some(expr_len);
            }

            let len = len.ok_or(PatternError::EmptyGroup)?;
            match okj {
                JumpFrom::Beginning(jump) => self.skip(jump, abs_erj, len),
                JumpFrom::End(mut jump) => {
                    jump += (self.ops.len() + 1).into();
                    self.skip(jump, abs_erj, len);
                }
            }
            self.patch_jump(erj, abs_erj);
        } else {
            let abs_okj = self.get_absolute_jump(okj);
            while self.next_is_not(b']')? {
                let expr_len =
                    self.parse_expr(JumpFrom::Beginning(abs_okj), JumpFrom::End(Jump(0)))?;

                if len.unwrap_or(expr_len).0 != expr_len.0 {
                    return Err(PatternError::GroupWithElementsOfDifferentSize);
                }
                len = Some(expr_len);
            }
            self.jump_at_end(erj);
            self.patch_jump(okj, abs_okj);
        }

        self.assert_current(b']')?;
        len.ok_or(PatternError::EmptyGroup)
    }

    fn parse_class_expr(&mut self, okj: JumpFrom, erj: JumpFrom) -> Result<Length, PatternError> {
        let okj = match okj {
            JumpFrom::Beginning(jump) => jump,
            JumpFrom::End(mut jump) => {
                jump += self.ops.len().into();
                jump += 1.into();
                jump
            }
        };
        let erj = match erj {
            JumpFrom::Beginning(jump) => jump,
            JumpFrom::End(mut jump) => {
                jump += self.ops.len().into();
                jump += 1.into();
                jump
            }
        };

        let op = match self.current() {
            b'%' => match self.next()? {
                b'a' => Op::Alphabetic(okj, erj),
                b'l' => Op::Lower(okj, erj),
                b'u' => Op::Upper(okj, erj),
                b'd' => Op::Digit(okj, erj),
                b'w' => Op::Alphanumeric(okj, erj),
                b'%' => Op::Byte(okj, erj, b'%'),
                b'$' => Op::Byte(okj, erj, b'$'),
                b'.' => Op::Byte(okj, erj, b'.'),
                b'!' => Op::Byte(okj, erj, b'!'),
                b'(' => Op::Byte(okj, erj, b'('),
                b')' => Op::Byte(okj, erj, b')'),
                b'[' => Op::Byte(okj, erj, b'['),
                b']' => Op::Byte(okj, erj, b']'),
                b'{' => Op::Byte(okj, erj, b'{'),
                b'}' => Op::Byte(okj, erj, b'}'),
                b => return Err(PatternError::InvalidEscaping(b as char)),
            },
            b'$' => {
                self.ops.push(Op::EndAnchor(okj, erj));
                return Ok(Length(0));
            }
            b'.' => Op::SkipOne(okj, erj),
            b'!' => return Err(PatternError::Unescaped('!')),
            b'(' => return Err(PatternError::Unescaped('(')),
            b')' => return Err(PatternError::Unescaped(')')),
            b'[' => return Err(PatternError::Unescaped('[')),
            b']' => return Err(PatternError::Unescaped(']')),
            b'{' => return Err(PatternError::Unescaped('{')),
            b'}' => return Err(PatternError::Unescaped('}')),
            b => Op::Byte(okj, erj, b),
        };

        self.ops.push(op);
        Ok(Length(1))
    }

    fn optimize(&mut self) {
        let mut i = 0;
        while i < self.ops.len() {
            match &self.ops[i] {
                Op::Byte(_, _, _) => {
                    if !self.try_collapse_bytes3_at(i) {
                        self.try_collapse_sequence3_at(i);
                    }
                    i += 1;
                }
                Op::Unwind(jump, Length(0)) => {
                    let jump = *jump;
                    self.remove_jump_at(i, jump);
                }
                _ => i += 1,
            }
        }
    }

    fn remove_jump_at(&mut self, i: usize, mut jump: Jump) {
        self.ops.remove(i);

        if jump.0 as usize > i {
            jump.0 -= 1;
        }

        if self.start_jump.0 as usize > i {
            self.start_jump.0 -= 1;
        } else if self.start_jump.0 as usize == i {
            self.start_jump = jump;
        }

        macro_rules! fix_jump {
            ($j:ident) => {
                if $j.0 as usize > i {
                    $j.0 -= 1;
                } else if $j.0 as usize == i {
                    *$j = jump;
                }
            };
        }

        for op in &mut self.ops {
            match op {
                Op::Ok | Op::Error => (),
                Op::Unwind(j, _) => fix_jump!(j),
                Op::EndAnchor(okj, erj)
                | Op::SkipOne(okj, erj)
                | Op::SkipMany(okj, erj, _)
                | Op::Alphabetic(okj, erj)
                | Op::Lower(okj, erj)
                | Op::Upper(okj, erj)
                | Op::Digit(okj, erj)
                | Op::Alphanumeric(okj, erj)
                | Op::Byte(okj, erj, _)
                | Op::Bytes3(okj, erj, _) => {
                    fix_jump!(okj);
                    fix_jump!(erj);
                }
            }
        }
    }

    fn try_collapse_bytes3_at(&mut self, index: usize) -> bool {
        if index + 3 > self.ops.len() {
            return false;
        }

        let mut final_okj = None;
        let mut final_erj = None;
        let mut bytes = [0 as u8; 3];

        for i in 0..bytes.len() {
            let op_index = index + i;
            match &self.ops[op_index] {
                Op::Byte(okj, erj, b)
                    if okj.0 as usize == op_index + 1 && final_erj.unwrap_or(*erj).0 == erj.0 =>
                {
                    bytes[i] = *b;
                    final_okj = Some(*okj);
                    final_erj = Some(*erj);
                }
                _ => return false,
            }
        }

        self.ops[index] = Op::Bytes3(final_okj.unwrap(), final_erj.unwrap(), bytes);
        self.ops.drain((index + 1)..(index + 3));

        if self.start_jump.0 as usize > index {
            self.start_jump.0 -= 2;
        }

        macro_rules! fix_jump {
            ($j:ident) => {
                if $j.0 as usize > index {
                    $j.0 -= 2;
                }
            };
        }

        for op in &mut self.ops {
            match op {
                Op::Ok | Op::Error => (),
                Op::Unwind(j, _) => fix_jump!(j),
                Op::EndAnchor(okj, erj)
                | Op::SkipOne(okj, erj)
                | Op::SkipMany(okj, erj, _)
                | Op::Alphabetic(okj, erj)
                | Op::Lower(okj, erj)
                | Op::Upper(okj, erj)
                | Op::Digit(okj, erj)
                | Op::Alphanumeric(okj, erj)
                | Op::Byte(okj, erj, _)
                | Op::Bytes3(okj, erj, _) => {
                    fix_jump!(okj);
                    fix_jump!(erj);
                }
            }
        }

        true
    }

    fn try_collapse_sequence3_at(&mut self, index: usize) -> bool {
        if index + 6 > self.ops.len() {
            return false;
        }

        let mut final_okj = None;
        let mut final_erj = None;
        let mut bytes = [0 as u8; 3];

        for i in 0..bytes.len() {
            let op_index = index + i * 2;
            match &self.ops[op_index] {
                Op::Byte(okj, erj, b)
                    if okj.0 as usize == op_index + 2 && erj.0 as usize == op_index + 1 =>
                {
                    bytes[i] = *b;
                    final_okj = Some(*okj);
                }
                _ => return false,
            }

            let op_index = op_index + 1;
            match &self.ops[op_index] {
                Op::Unwind(jump, len) if len.0 as usize == i => {
                    final_erj = Some(*jump);
                }
                _ => return false,
            }
        }

        self.ops[index] = Op::Bytes3(final_okj.unwrap(), final_erj.unwrap(), bytes);
        self.ops.drain((index + 1)..(index + 6));

        if self.start_jump.0 as usize > index {
            self.start_jump.0 -= 5;
        }

        macro_rules! fix_jump {
            ($j:ident) => {
                if $j.0 as usize > index {
                    $j.0 -= 5;
                }
            };
        }

        for op in &mut self.ops {
            match op {
                Op::Ok | Op::Error => (),
                Op::Unwind(j, _) => fix_jump!(j),
                Op::EndAnchor(okj, erj)
                | Op::SkipOne(okj, erj)
                | Op::SkipMany(okj, erj, _)
                | Op::Alphabetic(okj, erj)
                | Op::Lower(okj, erj)
                | Op::Upper(okj, erj)
                | Op::Digit(okj, erj)
                | Op::Alphanumeric(okj, erj)
                | Op::Byte(okj, erj, _)
                | Op::Bytes3(okj, erj, _) => {
                    fix_jump!(okj);
                    fix_jump!(erj);
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_size() {
        assert_eq!(8, std::mem::size_of::<Op>());
    }

    #[test]
    fn simple_pattern() {
        let p = Pattern::new("").unwrap();
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(0), p.matches("a"));
        assert_eq!(MatchResult::Ok(0), p.matches("z"));
        assert_eq!(MatchResult::Ok(0), p.matches("A"));
        assert_eq!(MatchResult::Ok(0), p.matches("Z"));
        assert_eq!(MatchResult::Ok(0), p.matches("0"));
        assert_eq!(MatchResult::Ok(0), p.matches("9"));
        assert_eq!(MatchResult::Ok(0), p.matches("!"));

        let p = Pattern::new("a").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("aa"));
        assert_eq!(MatchResult::Err, p.matches("b"));
        assert_eq!(MatchResult::Err, p.matches(""));

        let p = Pattern::new("aa").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches("aa"));
        assert_eq!(MatchResult::Ok(2), p.matches("aaa"));
        assert_eq!(MatchResult::Err, p.matches("baa"));

        let p = Pattern::new("abc").unwrap();
        assert_eq!(MatchResult::Ok(3), p.matches("abc"));
        assert_eq!(MatchResult::Ok(3), p.matches("abcd"));
        assert_eq!(MatchResult::Err, p.matches("aabc"));

        let p = Pattern::new("%% %$ %. %! %( %) %[ %] %{ %}").unwrap();
        let matched_text = "% $ . ! ( ) [ ] { }";
        assert_eq!(MatchResult::Ok(matched_text.len()), p.matches(matched_text));

        let p = Pattern::new(".").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("z"));
        assert_eq!(MatchResult::Ok(1), p.matches("A"));
        assert_eq!(MatchResult::Ok(1), p.matches("Z"));
        assert_eq!(MatchResult::Ok(1), p.matches("0"));
        assert_eq!(MatchResult::Ok(1), p.matches("9"));
        assert_eq!(MatchResult::Ok(1), p.matches("!"));

        let p = Pattern::new("%a").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("z"));
        assert_eq!(MatchResult::Ok(1), p.matches("A"));
        assert_eq!(MatchResult::Ok(1), p.matches("Z"));
        assert_eq!(MatchResult::Err, p.matches("0"));
        assert_eq!(MatchResult::Err, p.matches("9"));
        assert_eq!(MatchResult::Err, p.matches("!"));

        let p = Pattern::new("%l").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("A"));
        assert_eq!(MatchResult::Err, p.matches("Z"));
        assert_eq!(MatchResult::Err, p.matches("0"));
        assert_eq!(MatchResult::Err, p.matches("9"));
        assert_eq!(MatchResult::Err, p.matches("!"));

        let p = Pattern::new("%u").unwrap();
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Ok(1), p.matches("A"));
        assert_eq!(MatchResult::Ok(1), p.matches("Z"));
        assert_eq!(MatchResult::Err, p.matches("0"));
        assert_eq!(MatchResult::Err, p.matches("9"));
        assert_eq!(MatchResult::Err, p.matches("!"));

        let p = Pattern::new("%d").unwrap();
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("A"));
        assert_eq!(MatchResult::Err, p.matches("Z"));
        assert_eq!(MatchResult::Ok(1), p.matches("0"));
        assert_eq!(MatchResult::Ok(1), p.matches("9"));
        assert_eq!(MatchResult::Err, p.matches("!"));

        let p = Pattern::new("%w").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("z"));
        assert_eq!(MatchResult::Ok(1), p.matches("A"));
        assert_eq!(MatchResult::Ok(1), p.matches("Z"));
        assert_eq!(MatchResult::Ok(1), p.matches("0"));
        assert_eq!(MatchResult::Ok(1), p.matches("9"));
        assert_eq!(MatchResult::Err, p.matches("!"));
    }

    #[test]
    fn group() {
        let p = Pattern::new("[abc]").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("b"));
        assert_eq!(MatchResult::Ok(1), p.matches("c"));
        assert_eq!(MatchResult::Err, p.matches("d"));

        let p = Pattern::new("z[abc]y").unwrap();
        assert_eq!(MatchResult::Ok(3), p.matches("zay"));
        assert_eq!(MatchResult::Ok(3), p.matches("zby"));
        assert_eq!(MatchResult::Ok(3), p.matches("zcy"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("zy"));
        assert_eq!(MatchResult::Err, p.matches("zdy"));

        let p = Pattern::new("z[a]").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches("za"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("zb"));

        let p = Pattern::new("z[%l%d]").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches("za"));
        assert_eq!(MatchResult::Ok(2), p.matches("zz"));
        assert_eq!(MatchResult::Ok(2), p.matches("z0"));
        assert_eq!(MatchResult::Ok(2), p.matches("z9"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("zA"));
        assert_eq!(MatchResult::Err, p.matches("zZ"));

        let p = Pattern::new("[!abc]").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("d"));
        assert_eq!(MatchResult::Ok(1), p.matches("3"));
        assert_eq!(MatchResult::Ok(1), p.matches("@"));
        assert_eq!(MatchResult::Ok(1), p.matches("@a"));
        assert_eq!(MatchResult::Ok(1), p.matches("@b"));
        assert_eq!(MatchResult::Ok(1), p.matches("@c"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("b"));
        assert_eq!(MatchResult::Err, p.matches("c"));

        let p = Pattern::new("x[!%w]y").unwrap();
        assert_eq!(MatchResult::Err, p.matches("xay"));
        assert_eq!(MatchResult::Err, p.matches("xzy"));
        assert_eq!(MatchResult::Err, p.matches("xAy"));
        assert_eq!(MatchResult::Err, p.matches("xZy"));
        assert_eq!(MatchResult::Err, p.matches("x0y"));
        assert_eq!(MatchResult::Err, p.matches("x9y"));
        assert_eq!(MatchResult::Ok(3), p.matches("x#y"));
    }

    #[test]
    fn sequence() {
        let p = Pattern::new("(abc)").unwrap();
        assert_eq!(MatchResult::Ok(3), p.matches("abc"));
        assert_eq!(MatchResult::Ok(3), p.matches("abcd"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("ab"));

        let p = Pattern::new("z(abc)y").unwrap();
        assert_eq!(MatchResult::Ok(5), p.matches("zabcy"));
        assert_eq!(MatchResult::Ok(5), p.matches("zabcyd"));
        assert_eq!(MatchResult::Err, p.matches("zay"));
        assert_eq!(MatchResult::Err, p.matches("zaby"));

        let p = Pattern::new("z(%u%w)y").unwrap();
        assert_eq!(MatchResult::Ok(4), p.matches("zA0y"));
        assert_eq!(MatchResult::Ok(4), p.matches("zZay"));
        assert_eq!(MatchResult::Ok(4), p.matches("zA0yA"));
        assert_eq!(MatchResult::Err, p.matches("zaay"));
        assert_eq!(MatchResult::Err, p.matches("z8ay"));

        let p = Pattern::new("(!abc)").unwrap();
        assert_eq!(MatchResult::Err, p.matches("abc"));
        assert_eq!(MatchResult::Err, p.matches("abcd"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("ac"));
        assert_eq!(MatchResult::Err, p.matches("ab"));
        assert_eq!(MatchResult::Ok(3), p.matches("abz"));
        assert_eq!(MatchResult::Ok(3), p.matches("ab!"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("7a"));
        assert_eq!(MatchResult::Ok(3), p.matches("7ab"));
    }

    #[test]
    fn repeat() {
        let p = Pattern::new("{a}").unwrap();
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(4), p.matches("aaaa"));
        assert_eq!(MatchResult::Ok(0), p.matches("b"));

        let p = Pattern::new("{a}b").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches("ab"));
        assert_eq!(MatchResult::Ok(3), p.matches("aab"));
        assert_eq!(MatchResult::Ok(5), p.matches("aaaab"));

        let p = Pattern::new("a{b}c").unwrap();
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("ab"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(3), p.matches("abc"));
        assert_eq!(MatchResult::Ok(5), p.matches("abbbc"));

        let p = Pattern::new("a{bc}d").unwrap();
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Ok(2), p.matches("ad"));
        assert_eq!(MatchResult::Ok(3), p.matches("abd"));
        assert_eq!(MatchResult::Ok(3), p.matches("acd"));
        assert_eq!(MatchResult::Ok(4), p.matches("abcd"));
        assert_eq!(MatchResult::Ok(5), p.matches("abcbd"));
        assert_eq!(MatchResult::Ok(6), p.matches("abcbcd"));

        let p = Pattern::new("a{b!c}d").unwrap();
        assert_eq!(MatchResult::Err, p.matches("ad"));
        assert_eq!(MatchResult::Err, p.matches("abd"));
        assert_eq!(MatchResult::Ok(3), p.matches("acd"));
        assert_eq!(MatchResult::Ok(5), p.matches("abbcd"));
    }

    #[test]
    fn end_anchor() {
        let p = Pattern::new("a$").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("aa"));

        let p = Pattern::new("a$b").unwrap();
        assert_eq!(
            MatchResult::Pending(1, PatternState { op_index: 3 }),
            p.matches("a")
        );
        assert_eq!(
            MatchResult::Ok(1),
            p.matches_with_state("b", &PatternState { op_index: 3 })
        );

        let p = Pattern::new("a{.!$}b").unwrap();
        match p.matches("axyz") {
            MatchResult::Pending(4, state) => {
                assert_eq!(MatchResult::Ok(1), p.matches_with_state("b", &state))
            }
            _ => assert!(false),
        }

        let p = Pattern::new("a{b$!c}{c!d}").unwrap();
        match p.matches("abb") {
            MatchResult::Pending(3, state) => match p.matches_with_state("bb", &state) {
                MatchResult::Pending(2, state) => {
                    assert_eq!(MatchResult::Ok(4), p.matches_with_state("bccd", &state));
                }
                _ => assert!(false),
            },
            _ => assert!(false),
        }
    }

    #[test]
    fn complex_pattern() {
        let p = Pattern::new("{.!$}").unwrap();
        assert_eq!(MatchResult::Ok(10), p.matches("things 890"));
        assert_eq!(MatchResult::Ok(1), p.matches("0"));
        assert_eq!(MatchResult::Ok(1), p.matches(" "));

        let p = Pattern::new("{[ab%d]!c}").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("c"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(2), p.matches("bc"));
        assert_eq!(MatchResult::Ok(3), p.matches("bac"));
        assert_eq!(MatchResult::Ok(5), p.matches("0b4ac"));
        assert_eq!(MatchResult::Ok(14), p.matches("a1b234ba9bbbbc"));

        let p = Pattern::new("%d{[%w_%.]!@}").unwrap();
        assert_eq!(MatchResult::Ok(6), p.matches("1x4_5@"));
        assert_eq!(MatchResult::Ok(15), p.matches("9xxasd_234.45f@"));

        let p = Pattern::new("ab{(!ba)!b}a").unwrap();
        assert_eq!(MatchResult::Ok(4), p.matches("abba"));
    }

    #[test]
    fn edge_cases() {
        let p = Pattern::new("(!(!abc))").unwrap();
        assert_eq!(MatchResult::Ok(3), p.matches("abc"));
        assert_eq!(MatchResult::Err, p.matches("xyz"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("ab"));
        assert_eq!(MatchResult::Err, p.matches("abz"));

        let p = Pattern::new("[![!abc]]").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("b"));
        assert_eq!(MatchResult::Ok(1), p.matches("c"));
        assert_eq!(MatchResult::Err, p.matches("x"));

        let p = Pattern::new("()").unwrap();
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(0), p.matches("x"));
    }

    #[test]
    fn pattern_composition() {
        assert!(matches!(
            Pattern::new("[(ab)c]"),
            Err(PatternError::GroupWithElementsOfDifferentSize)
        ));

        let p = Pattern::new("[(ab)(cd)]").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches("ab"));
        assert_eq!(MatchResult::Ok(2), p.matches("cd"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("c"));
        assert_eq!(MatchResult::Err, p.matches("ad"));
        assert_eq!(MatchResult::Err, p.matches("cb"));

        let p = Pattern::new("[![(ab)(cd)]]").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches("ad"));
        assert_eq!(MatchResult::Ok(2), p.matches("bc"));
        assert_eq!(MatchResult::Err, p.matches("ab"));
        assert_eq!(MatchResult::Err, p.matches("cd"));

        let p = Pattern::new("[(ab)(!cd)]").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches("ab"));
        assert_eq!(MatchResult::Err, p.matches("b"));
        assert_eq!(MatchResult::Ok(2), p.matches("ax"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(2), p.matches("acd"));
        assert_eq!(MatchResult::Ok(2), p.matches("cb"));

        let p = Pattern::new("{(a[!ab])!x!$}").unwrap();
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Err, p.matches("aca"));
        assert_eq!(MatchResult::Err, p.matches("acab"));
        assert_eq!(MatchResult::Ok(4), p.matches("acax"));

        let p = Pattern::new("{[(!ab)(cd)]!$}").unwrap();
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(2), p.matches("cd"));
        assert_eq!(MatchResult::Err, p.matches("ab"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(4), p.matches("accd"));
    }

    #[test]
    fn bad_pattern() {
        macro_rules! assert_err {
            ($expected:expr, $value:expr) => {
                match $value {
                    Ok(_) => assert!(false),
                    Err(e) => assert_eq!($expected, e),
                }
            };
        }

        assert_err!(PatternError::UnexpectedEndOfPattern, Pattern::new("("));
        assert_err!(PatternError::Unescaped(')'), Pattern::new(")"));
        assert_err!(PatternError::UnexpectedEndOfPattern, Pattern::new("["));
        assert_err!(PatternError::Unescaped(']'), Pattern::new("]"));
        assert_err!(PatternError::EmptyGroup, Pattern::new("[]"));
        assert_err!(PatternError::UnexpectedEndOfPattern, Pattern::new("{"));
        assert_err!(PatternError::Unescaped('}'), Pattern::new("}"));
        assert_err!(PatternError::UnexpectedEndOfPattern, Pattern::new("%"));
        assert_err!(PatternError::Unescaped('!'), Pattern::new("!"));
        assert_err!(PatternError::InvalidEscaping('@'), Pattern::new("%@"));
    }
}
