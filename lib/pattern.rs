use std::{convert::From, fmt, ops::AddAssign};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    Pending(PatternState),
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
    pub fn new() -> Self {
        Self {
            ops: vec![Op::Error],
            start_jump: Jump(0),
        }
    }

    pub fn compile(&mut self, pattern: &str) -> Result<(), PatternError> {
        match PatternCompiler::new(&mut self.ops, pattern.as_bytes()).compile() {
            Ok(start_jump) => {
                self.start_jump = start_jump;
                Ok(())
            }
            Err(error) => {
                self.ops.clear();
                self.ops.push(Op::Error);
                self.start_jump = Jump(0);
                Err(error)
            }
        }
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
        let mut bytes = text.as_bytes();
        let ops = &self.ops;
        let mut op_index = state.op_index;

        #[inline]
        fn check_and_jump<F>(bytes: &mut &[u8], okj: Jump, erj: Jump, predicate: F) -> usize
        where
            F: Fn(u8) -> bool,
        {
            if !bytes.is_empty() && predicate(bytes[0]) {
                *bytes = &bytes[1..];
                okj.0 as _
            } else {
                erj.0 as _
            }
        }

        loop {
            match ops[op_index] {
                Op::Ok => {
                    return MatchResult::Ok(unsafe {
                        bytes.as_ptr().offset_from(text.as_bytes().as_ptr())
                    } as usize)
                }
                Op::Error => return MatchResult::Err,
                Op::Reset(jump) => {
                    bytes = text.as_bytes();
                    op_index = jump.0 as _;
                }
                Op::Unwind(jump, len) => {
                    bytes = unsafe {
                        std::slice::from_raw_parts(
                            bytes.as_ptr().offset(-(len.0 as isize)),
                            bytes.len() + len.0 as usize,
                        )
                    };
                    op_index = jump.0 as _;
                }
                Op::EndAnchor(okj, erj) => {
                    if bytes.is_empty() {
                        op_index = okj.0 as _;
                        return match ops[op_index] {
                            Op::Ok => MatchResult::Ok(unsafe {
                                bytes.as_ptr().offset_from(text.as_bytes().as_ptr())
                            } as usize),
                            _ => MatchResult::Pending(PatternState { op_index }),
                        };
                    } else {
                        op_index = erj.0 as _;
                    }
                }
                Op::SkipOne(okj, erj) => op_index = check_and_jump(&mut bytes, okj, erj, |_| true),
                Op::SkipMany(okj, erj, len) => {
                    let len = len.0 as usize;
                    if bytes.len() >= len {
                        bytes = &bytes[len..];
                        op_index = okj.0 as _;
                    } else {
                        op_index = erj.0 as _;
                    }
                }
                Op::Alphabetic(okj, erj) => {
                    op_index = check_and_jump(&mut bytes, okj, erj, |b| b.is_ascii_alphabetic());
                }
                Op::Lower(okj, erj) => {
                    op_index = check_and_jump(&mut bytes, okj, erj, |b| b.is_ascii_lowercase());
                }
                Op::Upper(okj, erj) => {
                    op_index = check_and_jump(&mut bytes, okj, erj, |b| b.is_ascii_uppercase());
                }
                Op::Digit(okj, erj) => {
                    op_index = check_and_jump(&mut bytes, okj, erj, |b| b.is_ascii_digit());
                }
                Op::Alphanumeric(okj, erj) => {
                    op_index = check_and_jump(&mut bytes, okj, erj, |b| b.is_ascii_alphanumeric());
                }
                Op::Byte(okj, erj, byte) => {
                    op_index = check_and_jump(&mut bytes, okj, erj, |b| b == byte)
                }
                Op::Bytes3(okj, erj, bs) => {
                    if bytes.len() >= 3
                        && bytes[0] == bs[0]
                        && bytes[1] == bs[1]
                        && bytes[2] == bs[2]
                    {
                        op_index = okj.0 as _;
                        bytes = &bytes[3..];
                    } else {
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
    Reset(Jump),
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
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const WIDTH: usize = 14;

        fn p(f: &mut fmt::Formatter, name: &str, okj: &Jump, erj: &Jump) -> fmt::Result {
            f.write_fmt(format_args!(
                "{:width$}{} {}",
                name,
                okj.0,
                erj.0,
                width = WIDTH
            ))
        }

        match self {
            Op::Ok => f.write_str("Ok"),
            Op::Error => f.write_str("Error"),
            Op::Reset(jump) => f.write_fmt(format_args!(
                "{:width$} {}",
                "Reset",
                jump.0,
                width = WIDTH - 4,
            )),
            Op::Unwind(jump, len) => f.write_fmt(format_args!(
                "{:width$}[{}] {}",
                "Unwind",
                len.0,
                jump.0,
                width = WIDTH - 4
            )),
            Op::EndAnchor(okj, erj) => p(f, "EndAnchor", okj, erj),
            Op::SkipOne(okj, erj) => p(f, "SkipOne", okj, erj),
            Op::SkipMany(okj, erj, len) => f.write_fmt(format_args!(
                "{:width$}[{}] {} {}",
                "SkipMany",
                len.0,
                okj.0,
                erj.0,
                width = WIDTH - 4
            )),
            Op::Alphabetic(okj, erj) => p(f, "Alphabetic", okj, erj),
            Op::Lower(okj, erj) => p(f, "Lower", okj, erj),
            Op::Upper(okj, erj) => p(f, "Upper", okj, erj),
            Op::Digit(okj, erj) => p(f, "Digit", okj, erj),
            Op::Alphanumeric(okj, erj) => p(f, "Alphanumeric", okj, erj),
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
    pub ops: &'a mut Vec<Op>,
}

impl<'a> PatternCompiler<'a> {
    pub fn new(ops: &'a mut Vec<Op>, bytes: &'a [u8]) -> Self {
        ops.clear();
        Self {
            bytes,
            index: 0,
            start_jump: Jump(2),
            ops,
        }
    }

    pub fn compile(mut self) -> Result<Jump, PatternError> {
        self.ops.push(Op::Error);
        self.ops.push(Op::Ok);
        self.parse_subpatterns()?;
        self.optimize();
        Ok(self.start_jump)
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
        match self.peek() {
            Ok(b) => {
                self.index += 1;
                Ok(b)
            }
            Err(e) => Err(e),
        }
    }

    fn next_is_not(&mut self, byte: u8) -> Result<bool, PatternError> {
        match self.next() {
            Ok(b) => Ok(b != byte),
            Err(e) => Err(e),
        }
    }

    fn parse_subpatterns(&mut self) -> Result<(), PatternError> {
        fn add_reset_jump(compiler: &mut PatternCompiler) -> Jump {
            let jump = (compiler.ops.len() + 2).into();
            compiler.ops.push(Op::Unwind(jump, Length(0)));
            let jump = compiler.ops.len().into();
            compiler.ops.push(Op::Reset(jump));
            jump
        }
        fn patch_reset_jump(compiler: &mut PatternCompiler, reset_jump: Jump) {
            let jump = compiler.ops.len().into();
            if let Op::Reset(j) = &mut compiler.ops[reset_jump.0 as usize] {
                *j = jump;
            } else {
                unreachable!();
            }
        }

        let mut reset_jump = add_reset_jump(self);
        if let Ok(_) = self.next() {
            self.parse_stmt(JumpFrom::Beginning(reset_jump))?;
            while let Ok(_) = self.next() {
                if self.current() == b'|' {
                    self.next()?;
                    self.ops.push(Op::Unwind(Jump(1), Length(0)));
                    patch_reset_jump(self, reset_jump);
                    reset_jump = add_reset_jump(self);
                }
                self.parse_stmt(JumpFrom::Beginning(reset_jump))?;
            }
        }
        self.ops.push(Op::Unwind(Jump(1), Length(0)));
        self.ops[reset_jump.0 as usize] = Op::Unwind(Jump(0), Length(0));
        Ok(())
    }

    fn parse_stmt(&mut self, erj: JumpFrom) -> Result<(), PatternError> {
        match self.current() {
            b'{' => self.parse_repeat_stmt(erj),
            _ => match self.parse_expr(JumpFrom::End(Jump(0)), erj) {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
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

    fn patch_unwind_jump(&mut self, jump: JumpFrom, unwind_jump: Jump) {
        if let JumpFrom::End(mut jump) = jump {
            jump += self.ops.len().into();
            if let Op::Unwind(j, Length(0)) = &mut self.ops[unwind_jump.0 as usize] {
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

    fn parse_repeat_stmt(&mut self, erj: JumpFrom) -> Result<(), PatternError> {
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
            self.jump_at_end(erj);
        }

        self.patch_unwind_jump(JumpFrom::End(Jump(0)), end_jump);

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
            self.patch_unwind_jump(erj, abs_erj);
        } else {
            let abs_erj = self.get_absolute_jump(erj);
            while self.next_is_not(b')')? {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(1)), JumpFrom::End(Jump(0)))?;
                self.ops.push(Op::Unwind(abs_erj, len));
                len += expr_len;
            }
            self.jump_at_end(okj);
            self.patch_unwind_jump(erj, abs_erj);
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
            self.patch_unwind_jump(erj, abs_erj);
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
            self.patch_unwind_jump(okj, abs_okj);
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
                b'|' => Op::Byte(okj, erj, b'|'),
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
            b'|' => return Err(PatternError::Unescaped('|')),
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

    fn remove_jump_at(&mut self, index: usize, mut jump: Jump) {
        self.ops.remove(index);

        if jump.0 as usize > index {
            jump.0 -= 1;
        }

        if self.start_jump.0 as usize > index {
            self.start_jump.0 -= 1;
        } else if self.start_jump.0 as usize == index {
            self.start_jump = jump;
        }

        #[inline]
        fn fix_jump(jump: &mut Jump, index: usize, removed_jump: Jump) {
            if jump.0 as usize > index {
                jump.0 -= 1;
            } else if jump.0 as usize == index {
                *jump = removed_jump;
            }
        }

        for op in self.ops.iter_mut() {
            match op {
                Op::Ok | Op::Error => (),
                Op::Reset(j) | Op::Unwind(j, _) => fix_jump(j, index, jump),
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
                    fix_jump(okj, index, jump);
                    fix_jump(erj, index, jump);
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

        #[inline]
        fn fix_jump(jump: &mut Jump, index: usize) {
            if jump.0 as usize > index {
                jump.0 -= 2;
            }
        }

        for op in self.ops.iter_mut() {
            match op {
                Op::Ok | Op::Error => (),
                Op::Reset(j) | Op::Unwind(j, _) => fix_jump(j, index),
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
                    fix_jump(okj, index);
                    fix_jump(erj, index);
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

        #[inline]
        fn fix_jump(jump: &mut Jump, index: usize) {
            if jump.0 as usize > index {
                jump.0 -= 5;
            }
        }

        for op in self.ops.iter_mut() {
            match op {
                Op::Ok | Op::Error => (),
                Op::Reset(j) | Op::Unwind(j, _) => fix_jump(j, index),
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
                    fix_jump(okj, index);
                    fix_jump(erj, index);
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn try_new_pattern(pattern: &str) -> Result<Pattern, PatternError> {
        let mut p = Pattern::new();
        p.compile(pattern)?;
        Ok(p)
    }

    fn new_pattern(pattern: &str) -> Pattern {
        try_new_pattern(pattern).unwrap()
    }

    #[test]
    fn assert_size() {
        assert_eq!(8, std::mem::size_of::<Op>());
    }

    #[test]
    fn simple_pattern() {
        let p = new_pattern("");
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(0), p.matches("a"));
        assert_eq!(MatchResult::Ok(0), p.matches("z"));
        assert_eq!(MatchResult::Ok(0), p.matches("A"));
        assert_eq!(MatchResult::Ok(0), p.matches("Z"));
        assert_eq!(MatchResult::Ok(0), p.matches("0"));
        assert_eq!(MatchResult::Ok(0), p.matches("9"));
        assert_eq!(MatchResult::Ok(0), p.matches("!"));

        let p = new_pattern("a");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("aa"));
        assert_eq!(MatchResult::Err, p.matches("b"));
        assert_eq!(MatchResult::Err, p.matches(""));

        let p = new_pattern("aa");
        assert_eq!(MatchResult::Ok(2), p.matches("aa"));
        assert_eq!(MatchResult::Ok(2), p.matches("aaa"));
        assert_eq!(MatchResult::Err, p.matches("baa"));

        let p = new_pattern("abc");
        assert_eq!(MatchResult::Ok(3), p.matches("abc"));
        assert_eq!(MatchResult::Ok(3), p.matches("abcd"));
        assert_eq!(MatchResult::Err, p.matches("aabc"));

        let p = new_pattern("%% %$ %. %! %( %) %[ %] %{ %}");
        let matched_text = "% $ . ! ( ) [ ] { }";
        assert_eq!(MatchResult::Ok(matched_text.len()), p.matches(matched_text));

        let p = new_pattern(".");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("z"));
        assert_eq!(MatchResult::Ok(1), p.matches("A"));
        assert_eq!(MatchResult::Ok(1), p.matches("Z"));
        assert_eq!(MatchResult::Ok(1), p.matches("0"));
        assert_eq!(MatchResult::Ok(1), p.matches("9"));
        assert_eq!(MatchResult::Ok(1), p.matches("!"));

        let p = new_pattern("%a");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("z"));
        assert_eq!(MatchResult::Ok(1), p.matches("A"));
        assert_eq!(MatchResult::Ok(1), p.matches("Z"));
        assert_eq!(MatchResult::Err, p.matches("0"));
        assert_eq!(MatchResult::Err, p.matches("9"));
        assert_eq!(MatchResult::Err, p.matches("!"));

        let p = new_pattern("%l");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("A"));
        assert_eq!(MatchResult::Err, p.matches("Z"));
        assert_eq!(MatchResult::Err, p.matches("0"));
        assert_eq!(MatchResult::Err, p.matches("9"));
        assert_eq!(MatchResult::Err, p.matches("!"));

        let p = new_pattern("%u");
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Ok(1), p.matches("A"));
        assert_eq!(MatchResult::Ok(1), p.matches("Z"));
        assert_eq!(MatchResult::Err, p.matches("0"));
        assert_eq!(MatchResult::Err, p.matches("9"));
        assert_eq!(MatchResult::Err, p.matches("!"));

        let p = new_pattern("%d");
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("A"));
        assert_eq!(MatchResult::Err, p.matches("Z"));
        assert_eq!(MatchResult::Ok(1), p.matches("0"));
        assert_eq!(MatchResult::Ok(1), p.matches("9"));
        assert_eq!(MatchResult::Err, p.matches("!"));

        let p = new_pattern("%w");
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
        let p = new_pattern("[abc]");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("b"));
        assert_eq!(MatchResult::Ok(1), p.matches("c"));
        assert_eq!(MatchResult::Err, p.matches("d"));

        let p = new_pattern("z[abc]y");
        assert_eq!(MatchResult::Ok(3), p.matches("zay"));
        assert_eq!(MatchResult::Ok(3), p.matches("zby"));
        assert_eq!(MatchResult::Ok(3), p.matches("zcy"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("zy"));
        assert_eq!(MatchResult::Err, p.matches("zdy"));

        let p = new_pattern("z[a]");
        assert_eq!(MatchResult::Ok(2), p.matches("za"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("zb"));

        let p = new_pattern("z[%l%d]");
        assert_eq!(MatchResult::Ok(2), p.matches("za"));
        assert_eq!(MatchResult::Ok(2), p.matches("zz"));
        assert_eq!(MatchResult::Ok(2), p.matches("z0"));
        assert_eq!(MatchResult::Ok(2), p.matches("z9"));
        assert_eq!(MatchResult::Err, p.matches("z"));
        assert_eq!(MatchResult::Err, p.matches("zA"));
        assert_eq!(MatchResult::Err, p.matches("zZ"));

        let p = new_pattern("[!abc]");
        assert_eq!(MatchResult::Ok(1), p.matches("d"));
        assert_eq!(MatchResult::Ok(1), p.matches("3"));
        assert_eq!(MatchResult::Ok(1), p.matches("@"));
        assert_eq!(MatchResult::Ok(1), p.matches("@a"));
        assert_eq!(MatchResult::Ok(1), p.matches("@b"));
        assert_eq!(MatchResult::Ok(1), p.matches("@c"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("b"));
        assert_eq!(MatchResult::Err, p.matches("c"));

        let p = new_pattern("x[!%w]y");
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
        let p = new_pattern("(abc)");
        assert_eq!(MatchResult::Ok(3), p.matches("abc"));
        assert_eq!(MatchResult::Ok(3), p.matches("abcd"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("ab"));

        let p = new_pattern("z(abc)y");
        assert_eq!(MatchResult::Ok(5), p.matches("zabcy"));
        assert_eq!(MatchResult::Ok(5), p.matches("zabcyd"));
        assert_eq!(MatchResult::Err, p.matches("zay"));
        assert_eq!(MatchResult::Err, p.matches("zaby"));

        let p = new_pattern("z(%u%w)y");
        assert_eq!(MatchResult::Ok(4), p.matches("zA0y"));
        assert_eq!(MatchResult::Ok(4), p.matches("zZay"));
        assert_eq!(MatchResult::Ok(4), p.matches("zA0yA"));
        assert_eq!(MatchResult::Err, p.matches("zaay"));
        assert_eq!(MatchResult::Err, p.matches("z8ay"));

        let p = new_pattern("(!abc)");
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
        let p = new_pattern("{a}");
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(4), p.matches("aaaa"));
        assert_eq!(MatchResult::Ok(0), p.matches("b"));

        let p = new_pattern("{a}b");
        assert_eq!(MatchResult::Ok(2), p.matches("ab"));
        assert_eq!(MatchResult::Ok(3), p.matches("aab"));
        assert_eq!(MatchResult::Ok(5), p.matches("aaaab"));

        let p = new_pattern("a{b}c");
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("ab"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(3), p.matches("abc"));
        assert_eq!(MatchResult::Ok(5), p.matches("abbbc"));

        let p = new_pattern("a{bc}d");
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Ok(2), p.matches("ad"));
        assert_eq!(MatchResult::Ok(3), p.matches("abd"));
        assert_eq!(MatchResult::Ok(3), p.matches("acd"));
        assert_eq!(MatchResult::Ok(4), p.matches("abcd"));
        assert_eq!(MatchResult::Ok(5), p.matches("abcbd"));
        assert_eq!(MatchResult::Ok(6), p.matches("abcbcd"));

        let p = new_pattern("a{b!c}d");
        assert_eq!(MatchResult::Err, p.matches("ad"));
        assert_eq!(MatchResult::Err, p.matches("abd"));
        assert_eq!(MatchResult::Ok(3), p.matches("acd"));
        assert_eq!(MatchResult::Ok(5), p.matches("abbcd"));
    }

    #[test]
    fn end_anchor() {
        let p = new_pattern("a$");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("aa"));

        let p = new_pattern("a$b");
        assert_eq!(
            MatchResult::Pending(PatternState { op_index: 4 }),
            p.matches("a")
        );
        assert_eq!(
            MatchResult::Ok(1),
            p.matches_with_state("b", &PatternState { op_index: 4 })
        );

        let p = new_pattern("a{.!$}b");
        match p.matches("axyz") {
            MatchResult::Pending(state) => {
                assert_eq!(MatchResult::Ok(1), p.matches_with_state("b", &state))
            }
            _ => assert!(false),
        }

        let p = new_pattern("a{b$!c}{c!d}");
        match p.matches("abb") {
            MatchResult::Pending(state) => match p.matches_with_state("bb", &state) {
                MatchResult::Pending(state) => {
                    assert_eq!(MatchResult::Ok(4), p.matches_with_state("bccd", &state));
                }
                _ => assert!(false),
            },
            _ => assert!(false),
        }
    }

    #[test]
    fn complex_pattern() {
        let p = new_pattern("{.!$}");
        assert_eq!(MatchResult::Ok(10), p.matches("things 890"));
        assert_eq!(MatchResult::Ok(1), p.matches("0"));
        assert_eq!(MatchResult::Ok(1), p.matches(" "));

        let p = new_pattern("{[ab%d]!c}");
        assert_eq!(MatchResult::Ok(1), p.matches("c"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(2), p.matches("bc"));
        assert_eq!(MatchResult::Ok(3), p.matches("bac"));
        assert_eq!(MatchResult::Ok(5), p.matches("0b4ac"));
        assert_eq!(MatchResult::Ok(14), p.matches("a1b234ba9bbbbc"));

        let p = new_pattern("%d{[%w_%.]!@}");
        assert_eq!(MatchResult::Ok(6), p.matches("1x4_5@"));
        assert_eq!(MatchResult::Ok(15), p.matches("9xxasd_234.45f@"));

        let p = new_pattern("ab{(!ba)!b}a");
        assert_eq!(MatchResult::Ok(4), p.matches("abba"));
    }

    #[test]
    fn edge_cases() {
        let p = new_pattern("(!(!abc))");
        assert_eq!(MatchResult::Ok(3), p.matches("abc"));
        assert_eq!(MatchResult::Err, p.matches("xyz"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("ab"));
        assert_eq!(MatchResult::Err, p.matches("abz"));

        let p = new_pattern("[![!abc]]");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("b"));
        assert_eq!(MatchResult::Ok(1), p.matches("c"));
        assert_eq!(MatchResult::Err, p.matches("x"));

        let p = new_pattern("()");
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(0), p.matches("x"));
    }

    #[test]
    fn pattern_composition() {
        assert!(matches!(
            try_new_pattern("[(ab)c]"),
            Err(PatternError::GroupWithElementsOfDifferentSize)
        ));

        let p = new_pattern("[(ab)(cd)]");
        assert_eq!(MatchResult::Ok(2), p.matches("ab"));
        assert_eq!(MatchResult::Ok(2), p.matches("cd"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("c"));
        assert_eq!(MatchResult::Err, p.matches("ad"));
        assert_eq!(MatchResult::Err, p.matches("cb"));

        let p = new_pattern("[![(ab)(cd)]]");
        assert_eq!(MatchResult::Ok(2), p.matches("ad"));
        assert_eq!(MatchResult::Ok(2), p.matches("bc"));
        assert_eq!(MatchResult::Err, p.matches("ab"));
        assert_eq!(MatchResult::Err, p.matches("cd"));

        let p = new_pattern("[(ab)(!cd)]");
        assert_eq!(MatchResult::Ok(2), p.matches("ab"));
        assert_eq!(MatchResult::Err, p.matches("b"));
        assert_eq!(MatchResult::Ok(2), p.matches("ax"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(2), p.matches("acd"));
        assert_eq!(MatchResult::Ok(2), p.matches("cb"));

        let p = new_pattern("{(a[!ab])!x!$}");
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Err, p.matches("aca"));
        assert_eq!(MatchResult::Err, p.matches("acab"));
        assert_eq!(MatchResult::Ok(4), p.matches("acax"));

        let p = new_pattern("{[(!ab)(cd)]!$}");
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(2), p.matches("cd"));
        assert_eq!(MatchResult::Err, p.matches("ab"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(4), p.matches("accd"));
    }

    #[test]
    fn multi_subpatterns() {
        let p = new_pattern("a|b");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(1), p.matches("b"));
        assert_eq!(MatchResult::Err, p.matches("c"));
        assert_eq!(MatchResult::Err, p.matches(""));

        let p = new_pattern("ab{(ab)}|c");
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Ok(2), p.matches("ab"));
        assert_eq!(MatchResult::Ok(2), p.matches("aba"));
        assert_eq!(MatchResult::Ok(4), p.matches("abab"));
        assert_eq!(MatchResult::Ok(1), p.matches("c"));
        assert_eq!(MatchResult::Ok(1), p.matches("ca"));
        assert_eq!(MatchResult::Ok(1), p.matches("cab"));
    }

    #[test]
    fn bad_pattern() {
        fn assert_err(expected: PatternError, value: Result<Pattern, PatternError>) {
            match value {
                Ok(_) => assert!(false),
                Err(e) => assert_eq!(expected, e),
            }
        }

        assert_err(PatternError::UnexpectedEndOfPattern, try_new_pattern("("));
        assert_err(PatternError::Unescaped(')'), try_new_pattern(")"));
        assert_err(PatternError::UnexpectedEndOfPattern, try_new_pattern("["));
        assert_err(PatternError::Unescaped(']'), try_new_pattern("]"));
        assert_err(PatternError::EmptyGroup, try_new_pattern("[]"));
        assert_err(PatternError::UnexpectedEndOfPattern, try_new_pattern("{"));
        assert_err(PatternError::Unescaped('}'), try_new_pattern("}"));
        assert_err(PatternError::UnexpectedEndOfPattern, try_new_pattern("%"));
        assert_err(PatternError::Unescaped('!'), try_new_pattern("!"));
        assert_err(PatternError::InvalidEscaping('@'), try_new_pattern("%@"));
        assert_err(PatternError::Unescaped('|'), try_new_pattern("|"));
        assert_err(PatternError::UnexpectedEndOfPattern, try_new_pattern("a|"));
    }
}
