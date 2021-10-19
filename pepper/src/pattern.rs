use std::{convert::TryInto, fmt, num::TryFromIntError, ops::Range, str::Chars};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    Pending(PatternState),
    Ok(usize),
    Err,
}

#[derive(Debug)]
pub enum PatternError {
    UnexpectedEndOfPattern,
    Expected(char),
    InvalidEscaping(char),
    Unescaped(char),
    EmptyGroup,
    GroupWithElementsOfDifferentSize,
    PatternTooLong,
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
            Self::PatternTooLong => write!(f, "pattern is too long"),
        }
    }
}
impl From<TryFromIntError> for PatternError {
    fn from(_: TryFromIntError) -> Self {
        Self::PatternTooLong
    }
}

pub struct PatternEscaper<'a> {
    chars: Chars<'a>,
    pending_char: Option<char>,
}
impl<'a> PatternEscaper<'a> {
    pub fn escape(text: &'a str) -> Self {
        Self {
            chars: text.chars(),
            pending_char: None,
        }
    }
}
impl<'a> Iterator for PatternEscaper<'a> {
    type Item = char;
    fn next(&mut self) -> Option<Self::Item> {
        match self.pending_char.take() {
            Some(c) => Some(c),
            None => match self.chars.next() {
                Some(
                    c @ ('%' | '^' | '$' | '.' | '!' | '(' | ')' | '[' | ']' | '{' | '}' | '|'),
                ) => {
                    self.pending_char = Some(c);
                    Some('%')
                }
                c => c,
            },
        }
    }
}

pub struct MatchIndices<'pattern, 'text> {
    pattern: &'pattern Pattern,
    text: &'text str,
    index: usize,
    anchor: Option<char>,
}
impl<'pattern, 'text> Iterator for MatchIndices<'pattern, 'text> {
    type Item = Range<usize>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(anchor) = self.anchor {
                match self.text[self.index..].find(anchor) {
                    Some(i) => self.index += i,
                    None => {
                        self.index = self.text.len();
                        return None;
                    }
                }
            }

            match self.pattern.matches(self.text, self.index) {
                MatchResult::Ok(index) if index > self.index => {
                    let from = self.index;
                    self.index = index;
                    return Some(from..self.index);
                }
                _ => self.index += self.text[self.index..].chars().next()?.len_utf8(),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternState {
    op_jump: Jump,
}

struct OpsSlice<'a>(&'a [Op]);
impl<'a> OpsSlice<'a> {
    #[cfg(debug_assertions)]
    pub fn at(&self, jump: Jump) -> &Op {
        &self.0[jump.0 as usize]
    }

    #[cfg(not(debug_assertions))]
    pub fn at(&self, jump: Jump) -> &Op {
        unsafe { self.0.get_unchecked(jump.0 as usize) }
    }
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

    pub fn clear(&mut self) {
        self.ops.clear();
        self.ops.push(Op::Error);
        self.start_jump = Jump(0);
    }

    pub fn compile(&mut self, pattern: &str) -> Result<(), PatternError> {
        match PatternCompiler::new(&mut self.ops, pattern).compile() {
            Ok(start_jump) => {
                self.start_jump = start_jump;
                Ok(())
            }
            Err(error) => {
                self.clear();
                Err(error)
            }
        }
    }

    pub fn compile_searcher(&mut self, pattern: &str) -> Result<(), PatternError> {
        let (is_literal, ignore_case, pattern) = match pattern.as_bytes() {
            [b'f', b'/', ..] => (true, true, &pattern[2..]),
            [b'F', b'/', ..] => (true, false, &pattern[2..]),
            [b'p', b'/', ..] => (false, true, &pattern[2..]),
            [b'P', b'/', ..] => (false, false, &pattern[2..]),
            _ => (
                true,
                !pattern.chars().any(|c| c.is_ascii_uppercase()),
                pattern,
            ),
        };

        if is_literal {
            self.ops.clear();
            self.ops.push(Op::Error);

            let mut pattern = pattern;
            let mut buf = [0; OP_STRING_LEN];
            let mut len;
            loop {
                len = match pattern
                    .char_indices()
                    .map(|(i, c)| i + c.len_utf8())
                    .take_while(|&len| len < buf.len())
                    .last()
                {
                    Some(len) => len,
                    None => break,
                };
                buf[..len].copy_from_slice(pattern[..len].as_bytes());
                pattern = &pattern[len..];
                self.ops.push(Op::String(
                    Jump((self.ops.len() + 1) as _),
                    Jump(0),
                    len as _,
                    buf,
                ));
            }
            self.ops.push(Op::Ok);
            self.start_jump = Jump(1);
        } else {
            self.compile(pattern)?;
        }

        if ignore_case {
            self.ignore_case();
        }

        Ok(())
    }

    pub fn ignore_case(&mut self) {
        for op in &mut self.ops {
            match *op {
                Op::Char(okj, erj, c) => *op = Op::CharCaseInsensitive(okj, erj, c),
                Op::String(okj, erj, len, bytes) => {
                    *op = Op::StringCaseInsensitive(okj, erj, len, bytes)
                }
                _ => (),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        let ops = OpsSlice(&self.ops);
        matches!(ops.at(self.start_jump), Op::Ok | Op::Error)
    }

    pub fn search_anchor(&self) -> Option<char> {
        let ops = OpsSlice(&self.ops);
        let (c, erj) = match ops.at(self.start_jump) {
            Op::Error => return Some('\0'),
            &Op::Char(_, erj, c) => (c, erj),
            &Op::String(_, erj, len, bytes) => {
                let s = unsafe { std::str::from_utf8_unchecked(&bytes[..len as usize]) };
                let c = s.chars().next()?;
                (c, erj)
            }
            _ => return None,
        };

        match ops.at(erj) {
            Op::Error => Some(c),
            _ => None,
        }
    }

    pub fn match_indices<'pattern, 'text>(
        &'pattern self,
        text: &'text str,
        anchor: Option<char>,
    ) -> MatchIndices<'pattern, 'text> {
        MatchIndices {
            pattern: self,
            text,
            index: 0,
            anchor,
        }
    }

    pub fn matches(&self, text: &str, index: usize) -> MatchResult {
        self.matches_with_state(
            text,
            index,
            PatternState {
                op_jump: self.start_jump,
            },
        )
    }

    pub fn matches_with_state(&self, text: &str, index: usize, state: PatternState) -> MatchResult {
        let mut chars = text[index..].chars();
        let ops = OpsSlice(&self.ops);
        let mut op_jump = state.op_jump;

        fn offset(text: &str, chars: &Chars) -> usize {
            chars.as_str().as_ptr() as usize - text.as_ptr() as usize
        }

        fn check_and_jump<F>(chars: &mut Chars, okj: Jump, erj: Jump, predicate: F) -> Jump
        where
            F: Fn(char) -> bool,
        {
            let previous_state = chars.clone();
            match chars.next() {
                Some(c) if predicate(c) => okj,
                _ => {
                    *chars = previous_state;
                    erj
                }
            }
        }

        loop {
            match ops.at(op_jump) {
                Op::Ok => return MatchResult::Ok(offset(text, &chars)),
                Op::Error => return MatchResult::Err,
                &Op::Reset(jump) => {
                    chars = text[index..].chars();
                    op_jump = jump;
                }
                &Op::Unwind(jump, len) => {
                    let len = (len.0 - 1) as _;
                    let offset = offset(text, &chars);
                    chars = match text[..offset].char_indices().rev().nth(len) {
                        Some((i, _)) => text[i..].chars(),
                        None => unreachable!(),
                    };
                    op_jump = jump;
                }
                &Op::BeginningAnchor(okj, erj) => {
                    op_jump = match index {
                        0 => okj,
                        _ => erj,
                    };
                }
                &Op::EndingAnchor(okj, erj) => {
                    if chars.as_str().is_empty() {
                        op_jump = okj;
                        return match ops.at(op_jump) {
                            Op::Ok => MatchResult::Ok(offset(text, &chars)),
                            _ => MatchResult::Pending(PatternState { op_jump }),
                        };
                    } else {
                        op_jump = erj;
                    }
                }
                &Op::WordBoundary(okj, erj) => {
                    let rest = chars.as_str();
                    let previous_char = text[..text.len() - rest.len()]
                        .chars()
                        .next_back()
                        .or_else(|| text[..index].chars().next_back());
                    let current_char = rest.chars().next();
                    let at_boundary = match previous_char.zip(current_char) {
                        Some((p, c)) => {
                            !p.is_ascii_alphanumeric() && p != '_'
                                || !c.is_ascii_alphanumeric() && c != '_'
                        }
                        None => true,
                    };
                    op_jump = if at_boundary { okj } else { erj };
                }
                &Op::SkipOne(okj, erj) => op_jump = check_and_jump(&mut chars, okj, erj, |_| true),
                &Op::SkipMany(okj, erj, len) => {
                    let len = (len.0 - 1) as _;
                    op_jump = match chars.nth(len) {
                        Some(_) => okj,
                        None => erj,
                    };
                }
                &Op::Alphabetic(okj, erj) => {
                    op_jump = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_alphabetic());
                }
                &Op::Lower(okj, erj) => {
                    op_jump = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_lowercase());
                }
                &Op::Upper(okj, erj) => {
                    op_jump = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_uppercase());
                }
                &Op::Digit(okj, erj) => {
                    op_jump = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_digit());
                }
                &Op::Alphanumeric(okj, erj) => {
                    op_jump = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_alphanumeric());
                }
                &Op::Char(okj, erj, ch) => {
                    op_jump = check_and_jump(&mut chars, okj, erj, |c| c == ch)
                }
                &Op::CharCaseInsensitive(okj, erj, ch) => {
                    op_jump = check_and_jump(&mut chars, okj, erj, |c| c.eq_ignore_ascii_case(&ch))
                }
                &Op::String(okj, erj, len, bytes) => {
                    let len = len as usize;
                    let bytes = &bytes[..len];
                    let text_bytes = chars.as_str().as_bytes();
                    op_jump = if text_bytes.len() >= len && &text_bytes[..len] == bytes {
                        chars = chars.as_str()[len..].chars();
                        okj
                    } else {
                        erj
                    }
                }
                &Op::StringCaseInsensitive(okj, erj, len, bytes) => {
                    let len = len as usize;
                    let bytes = &bytes[..len];
                    let text_bytes = chars.as_str().as_bytes();
                    op_jump = if text_bytes.len() >= len
                        && text_bytes[..len].eq_ignore_ascii_case(bytes)
                    {
                        chars = chars.as_str()[len..].chars();
                        okj
                    } else {
                        erj
                    }
                }
            }
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
                write!(f, "  > [{:width$}] ", i, width = op_digit_count)?;
            } else {
                write!(f, "    [{:width$}] ", i, width = op_digit_count)?;
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
impl Length {
    pub fn add(&mut self, other: Self) -> Result<(), PatternError> {
        match self.0.checked_add(other.0) {
            Some(result) => {
                self.0 = result;
                Ok(())
            }
            None => Err(PatternError::UnexpectedEndOfPattern),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Jump(u16);
impl Jump {
    pub fn add(&mut self, other: Self) -> Result<(), PatternError> {
        match self.0.checked_add(other.0) {
            Some(result) => {
                self.0 = result;
                Ok(())
            }
            None => Err(PatternError::UnexpectedEndOfPattern),
        }
    }
}

#[derive(Clone, Copy)]
enum JumpFrom {
    Beginning(Jump),
    End(Jump),
}

const OP_STRING_LEN: usize = 10;
const _ASSERT_OP_SIZE: [(); 16] = [(); std::mem::size_of::<Op>()];

#[derive(Clone)]
enum Op {
    Ok,
    Error,
    Reset(Jump),
    Unwind(Jump, Length),
    BeginningAnchor(Jump, Jump),
    EndingAnchor(Jump, Jump),
    WordBoundary(Jump, Jump),
    SkipOne(Jump, Jump),
    SkipMany(Jump, Jump, Length),
    Alphabetic(Jump, Jump),
    Lower(Jump, Jump),
    Upper(Jump, Jump),
    Digit(Jump, Jump),
    Alphanumeric(Jump, Jump),
    Char(Jump, Jump, char),
    CharCaseInsensitive(Jump, Jump, char),
    String(Jump, Jump, u8, [u8; OP_STRING_LEN]),
    StringCaseInsensitive(Jump, Jump, u8, [u8; OP_STRING_LEN]),
}

impl fmt::Debug for Op {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const WIDTH: usize = 14;

        fn p(f: &mut fmt::Formatter, name: &str, okj: Jump, erj: Jump) -> fmt::Result {
            write!(f, "{:width$}{} {}", name, okj.0, erj.0, width = WIDTH)
        }

        match self {
            Op::Ok => f.write_str("Ok"),
            Op::Error => f.write_str("Error"),
            Op::Reset(jump) => write!(f, "{:width$} {}", "Reset", jump.0, width = WIDTH - 4,),
            Op::Unwind(jump, len) => write!(
                f,
                "{:width$}[{}] {}",
                "Unwind",
                len.0,
                jump.0,
                width = WIDTH - 4
            ),
            &Op::BeginningAnchor(okj, erj) => p(f, "BeginningAnchor", okj, erj),
            &Op::EndingAnchor(okj, erj) => p(f, "EndAnchor", okj, erj),
            &Op::WordBoundary(okj, erj) => p(f, "WordBoundary", okj, erj),
            &Op::SkipOne(okj, erj) => p(f, "SkipOne", okj, erj),
            &Op::SkipMany(okj, erj, len) => write!(
                f,
                "{:width$}[{}] {} {}",
                "SkipMany",
                len.0,
                okj.0,
                erj.0,
                width = WIDTH - 4
            ),
            &Op::Alphabetic(okj, erj) => p(f, "Alphabetic", okj, erj),
            &Op::Lower(okj, erj) => p(f, "Lower", okj, erj),
            &Op::Upper(okj, erj) => p(f, "Upper", okj, erj),
            &Op::Digit(okj, erj) => p(f, "Digit", okj, erj),
            &Op::Alphanumeric(okj, erj) => p(f, "Alphanumeric", okj, erj),
            &Op::Char(okj, erj, c) => write!(
                f,
                "{:width$}'{}' {} {}",
                "Char",
                c,
                okj.0,
                erj.0,
                width = WIDTH - 4
            ),
            &Op::CharCaseInsensitive(okj, erj, c) => write!(
                f,
                "{:width$}'{}' {} {}",
                "CharCaseInsensitive",
                c,
                okj.0,
                erj.0,
                width = WIDTH - 4
            ),
            &Op::String(okj, erj, len, bytes) => write!(
                f,
                "{:width$}'{}' {} {}",
                "String",
                std::str::from_utf8(&bytes[..len as usize]).unwrap(),
                okj.0,
                erj.0,
                width = WIDTH - 4
            ),
            &Op::StringCaseInsensitive(okj, erj, len, bytes) => write!(
                f,
                "{:width$}'{}' {} {}",
                "StringCaseInsensitive",
                std::str::from_utf8(&bytes[..len as usize]).unwrap(),
                okj.0,
                erj.0,
                width = WIDTH - 4
            ),
        }
    }
}

struct PatternCompiler<'a> {
    pub text: Chars<'a>,
    pub current_char: char,
    pub start_jump: Jump,
    pub ops: &'a mut Vec<Op>,
}

impl<'a> PatternCompiler<'a> {
    pub fn new(ops: &'a mut Vec<Op>, text: &'a str) -> Self {
        ops.clear();
        Self {
            text: text.chars(),
            current_char: '\0',
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

    fn assert_current(&self, c: char) -> Result<(), PatternError> {
        if self.current_char == c {
            Ok(())
        } else {
            Err(PatternError::Expected(c))
        }
    }

    fn next(&mut self) -> Result<char, PatternError> {
        match self.text.next() {
            Some(c) => {
                self.current_char = c;
                Ok(c)
            }
            None => Err(PatternError::UnexpectedEndOfPattern),
        }
    }

    fn next_is(&mut self, c: char) -> Result<bool, PatternError> {
        match self.next() {
            Ok(ch) => Ok(ch == c),
            Err(e) => Err(e),
        }
    }

    fn parse_subpatterns(&mut self) -> Result<(), PatternError> {
        fn add_reset_jump(compiler: &mut PatternCompiler) -> Result<Jump, PatternError> {
            let jump = Jump((compiler.ops.len() + 2).try_into()?);
            compiler.ops.push(Op::Unwind(jump, Length(0)));
            let jump = Jump(compiler.ops.len().try_into()?);
            compiler.ops.push(Op::Reset(jump));
            Ok(jump)
        }
        fn patch_reset_jump(
            compiler: &mut PatternCompiler,
            reset_jump: Jump,
        ) -> Result<(), PatternError> {
            let jump = Jump(compiler.ops.len().try_into()?);
            if let Op::Reset(j) = &mut compiler.ops[reset_jump.0 as usize] {
                *j = jump;
            } else {
                unreachable!();
            }
            Ok(())
        }

        let mut reset_jump = add_reset_jump(self)?;
        if self.next().is_ok() {
            self.parse_stmt(JumpFrom::Beginning(reset_jump))?;
            while self.next().is_ok() {
                if self.current_char == '|' {
                    self.next()?;
                    self.ops.push(Op::Unwind(Jump(1), Length(0)));
                    patch_reset_jump(self, reset_jump)?;
                    reset_jump = add_reset_jump(self)?;
                }
                self.parse_stmt(JumpFrom::Beginning(reset_jump))?;
            }
        }
        self.ops.push(Op::Unwind(Jump(1), Length(0)));
        self.ops[reset_jump.0 as usize] = Op::Unwind(Jump(0), Length(0));
        Ok(())
    }

    fn parse_stmt(&mut self, erj: JumpFrom) -> Result<(), PatternError> {
        match self.current_char {
            '{' => self.parse_repeat_stmt(erj),
            _ => match self.parse_expr(JumpFrom::End(Jump(0)), erj) {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
        }
    }

    fn parse_expr(&mut self, okj: JumpFrom, erj: JumpFrom) -> Result<Length, PatternError> {
        let len = match self.current_char {
            '(' => self.parse_sequence_expr(okj, erj)?,
            '[' => self.parse_group_expr(okj, erj)?,
            _ => self.parse_class_expr(okj, erj)?,
        };

        Ok(len)
    }

    fn get_absolute_jump(&mut self, jump: JumpFrom) -> Result<Jump, PatternError> {
        match jump {
            JumpFrom::Beginning(jump) => Ok(jump),
            JumpFrom::End(_) => {
                let jump = Jump((self.ops.len() + 2).try_into()?);
                self.ops.push(Op::Unwind(jump, Length(0)));
                let jump = Jump(self.ops.len().try_into()?);
                self.ops.push(Op::Unwind(jump, Length(0)));
                Ok(jump)
            }
        }
    }

    fn patch_unwind_jump(&mut self, jump: JumpFrom, unwind_jump: Jump) -> Result<(), PatternError> {
        if let JumpFrom::End(mut jump) = jump {
            jump.add(Jump(self.ops.len().try_into()?))?;
            if let Op::Unwind(j, Length(0)) = &mut self.ops[unwind_jump.0 as usize] {
                *j = jump;
            } else {
                unreachable!();
            }
        }
        Ok(())
    }

    fn jump_at_end(&mut self, jump: JumpFrom) -> Result<(), PatternError> {
        match jump {
            JumpFrom::Beginning(jump) => self.ops.push(Op::Unwind(jump, Length(0))),
            JumpFrom::End(Jump(0)) => (),
            JumpFrom::End(mut jump) => {
                jump.add(Jump((self.ops.len() + 1).try_into()?))?;
                self.ops.push(Op::Unwind(jump, Length(0)));
            }
        }
        Ok(())
    }

    fn skip(&mut self, okj: Jump, erj: Jump, len: Length) {
        match len {
            Length(0) => self.ops.push(Op::Unwind(okj, Length(0))),
            Length(1) => self.ops.push(Op::SkipOne(okj, erj)),
            _ => self.ops.push(Op::SkipMany(okj, erj, len)),
        }
    }

    fn parse_repeat_stmt(&mut self, erj: JumpFrom) -> Result<(), PatternError> {
        let start_jump = Jump(self.ops.len().try_into()?);
        let end_jump = self.get_absolute_jump(JumpFrom::End(Jump(0)))?;

        let mut has_cancel_pattern = false;
        while !self.next_is('}')? {
            match self.current_char {
                '!' => {
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
            self.jump_at_end(erj)?;
        }

        self.patch_unwind_jump(JumpFrom::End(Jump(0)), end_jump)?;

        self.assert_current('}')?;
        Ok(())
    }

    fn parse_sequence_expr(
        &mut self,
        okj: JumpFrom,
        erj: JumpFrom,
    ) -> Result<Length, PatternError> {
        let previous_state = self.text.clone();
        let mut len = Length(0);

        if self.next()? == '!' {
            let abs_erj = self.get_absolute_jump(erj)?;
            while !self.next_is(')')? {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(2)), JumpFrom::End(Jump(0)))?;
                self.skip(
                    Jump((self.ops.len() + 3).try_into()?),
                    Jump((self.ops.len() + 1).try_into()?),
                    expr_len,
                );
                self.ops.push(Op::Unwind(abs_erj, len));
                len.add(expr_len)?;
            }
            self.ops.push(Op::Unwind(abs_erj, len));
            self.jump_at_end(okj)?;
            self.patch_unwind_jump(erj, abs_erj)?;
        } else {
            self.text = previous_state;
            let abs_erj = self.get_absolute_jump(erj)?;
            while !self.next_is(')')? {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(1)), JumpFrom::End(Jump(0)))?;
                self.ops.push(Op::Unwind(abs_erj, len));
                len.add(expr_len)?;
            }
            self.jump_at_end(okj)?;
            self.patch_unwind_jump(erj, abs_erj)?;
        }

        self.assert_current(')')?;
        Ok(len)
    }

    fn parse_group_expr(&mut self, okj: JumpFrom, erj: JumpFrom) -> Result<Length, PatternError> {
        let previous_state = self.text.clone();
        let mut len = None;

        if self.next()? == '!' {
            let abs_erj = self.get_absolute_jump(erj)?;
            while !self.next_is(']')? {
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
                    jump.add(Jump((self.ops.len() + 1).try_into()?))?;
                    self.skip(jump, abs_erj, len);
                }
            }
            self.patch_unwind_jump(erj, abs_erj)?;
        } else {
            self.text = previous_state;
            let abs_okj = self.get_absolute_jump(okj)?;
            while !self.next_is(']')? {
                let expr_len =
                    self.parse_expr(JumpFrom::Beginning(abs_okj), JumpFrom::End(Jump(0)))?;

                if len.unwrap_or(expr_len).0 != expr_len.0 {
                    return Err(PatternError::GroupWithElementsOfDifferentSize);
                }
                len = Some(expr_len);
            }
            self.jump_at_end(erj)?;
            self.patch_unwind_jump(okj, abs_okj)?;
        }

        self.assert_current(']')?;
        len.ok_or(PatternError::EmptyGroup)
    }

    fn parse_class_expr(&mut self, okj: JumpFrom, erj: JumpFrom) -> Result<Length, PatternError> {
        let okj = match okj {
            JumpFrom::Beginning(jump) => jump,
            JumpFrom::End(mut jump) => {
                jump.add(Jump(self.ops.len().try_into()?))?;
                jump.add(Jump(1))?;
                jump
            }
        };
        let erj = match erj {
            JumpFrom::Beginning(jump) => jump,
            JumpFrom::End(mut jump) => {
                jump.add(Jump(self.ops.len().try_into()?))?;
                jump.add(Jump(1))?;
                jump
            }
        };

        let op = match self.current_char {
            '%' => match self.next()? {
                'a' => Op::Alphabetic(okj, erj),
                'l' => Op::Lower(okj, erj),
                'u' => Op::Upper(okj, erj),
                'd' => Op::Digit(okj, erj),
                'w' => Op::Alphanumeric(okj, erj),
                'b' => {
                    self.ops.push(Op::WordBoundary(okj, erj));
                    return Ok(Length(0));
                }
                '%' => Op::Char(okj, erj, '%'),
                '^' => Op::Char(okj, erj, '^'),
                '$' => Op::Char(okj, erj, '$'),
                '.' => Op::Char(okj, erj, '.'),
                '!' => Op::Char(okj, erj, '!'),
                '(' => Op::Char(okj, erj, '('),
                ')' => Op::Char(okj, erj, ')'),
                '[' => Op::Char(okj, erj, '['),
                ']' => Op::Char(okj, erj, ']'),
                '{' => Op::Char(okj, erj, '{'),
                '}' => Op::Char(okj, erj, '}'),
                '|' => Op::Char(okj, erj, '|'),
                c => return Err(PatternError::InvalidEscaping(c)),
            },
            '^' => {
                self.ops.push(Op::BeginningAnchor(okj, erj));
                return Ok(Length(0));
            }
            '$' => {
                self.ops.push(Op::EndingAnchor(okj, erj));
                return Ok(Length(0));
            }
            '.' => Op::SkipOne(okj, erj),
            '!' => return Err(PatternError::Unescaped('!')),
            '(' => return Err(PatternError::Unescaped('(')),
            ')' => return Err(PatternError::Unescaped(')')),
            '[' => return Err(PatternError::Unescaped('[')),
            ']' => return Err(PatternError::Unescaped(']')),
            '{' => return Err(PatternError::Unescaped('{')),
            '}' => return Err(PatternError::Unescaped('}')),
            '|' => return Err(PatternError::Unescaped('|')),
            c => Op::Char(okj, erj, c),
        };

        self.ops.push(op);
        Ok(Length(1))
    }

    fn optimize(&mut self) {
        let mut i = 0;
        while i < self.ops.len() {
            match &self.ops[i] {
                Op::Char(_, _, _) => {
                    if !self.try_collapse_chars_at(i) {
                        self.try_collapse_sequence_at(i);
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

        fn fix_jump(jump: &mut Jump, index: usize, removed_jump: Jump) {
            if jump.0 as usize > index {
                jump.0 -= 1;
            } else if jump.0 as usize == index {
                *jump = removed_jump;
            }
        }

        fix_jump(&mut self.start_jump, index, jump);

        for op in self.ops.iter_mut() {
            match op {
                Op::Ok | Op::Error => (),
                Op::Reset(j) | Op::Unwind(j, _) => fix_jump(j, index, jump),
                Op::BeginningAnchor(okj, erj)
                | Op::EndingAnchor(okj, erj)
                | Op::WordBoundary(okj, erj)
                | Op::SkipOne(okj, erj)
                | Op::SkipMany(okj, erj, _)
                | Op::Alphabetic(okj, erj)
                | Op::Lower(okj, erj)
                | Op::Upper(okj, erj)
                | Op::Digit(okj, erj)
                | Op::Alphanumeric(okj, erj)
                | Op::Char(okj, erj, _)
                | Op::CharCaseInsensitive(okj, erj, _)
                | Op::String(okj, erj, _, _)
                | Op::StringCaseInsensitive(okj, erj, _, _) => {
                    fix_jump(okj, index, jump);
                    fix_jump(erj, index, jump);
                }
            }
        }
    }

    fn try_collapse_chars_at(&mut self, index: usize) -> bool {
        let (c, mut okj, erj) = match self.ops[index] {
            Op::Char(okj, erj, c) => (c, okj, erj),
            _ => return false,
        };
        let mut bytes = [0; OP_STRING_LEN];
        let mut len = c.encode_utf8(&mut bytes).len();

        let mut op_index = index + 1;
        while op_index < self.ops.len() {
            let (oj, ej, c) = match self.ops[op_index] {
                Op::Char(oj, ej, c) => (oj, ej, c),
                _ => break,
            };
            if op_index + 1 != oj.0 as _ || erj.0 != ej.0 || len + c.len_utf8() > OP_STRING_LEN {
                break;
            }

            len += c.encode_utf8(&mut bytes[len..]).len();
            okj = oj;
            op_index += 1;
        }

        let from = index + 1;
        let to = op_index;
        if from == to {
            return false;
        }

        self.ops[index] = Op::String(okj, erj, len as _, bytes);
        self.ops.drain(from..to);

        fn fix_jump(jump: &mut Jump, index: usize, fix: u16) {
            if jump.0 as usize > index {
                jump.0 -= fix;
            }
        }

        let fix = (len - 1) as _;
        fix_jump(&mut self.start_jump, index, fix);

        for op in self.ops.iter_mut() {
            match op {
                Op::Ok | Op::Error => (),
                Op::Reset(j) | Op::Unwind(j, _) => fix_jump(j, index, fix),
                Op::BeginningAnchor(okj, erj)
                | Op::EndingAnchor(okj, erj)
                | Op::WordBoundary(okj, erj)
                | Op::SkipOne(okj, erj)
                | Op::SkipMany(okj, erj, _)
                | Op::Alphabetic(okj, erj)
                | Op::Lower(okj, erj)
                | Op::Upper(okj, erj)
                | Op::Digit(okj, erj)
                | Op::Alphanumeric(okj, erj)
                | Op::Char(okj, erj, _)
                | Op::CharCaseInsensitive(okj, erj, _)
                | Op::String(okj, erj, _, _)
                | Op::StringCaseInsensitive(okj, erj, _, _) => {
                    fix_jump(okj, index, fix);
                    fix_jump(erj, index, fix);
                }
            }
        }

        true
    }

    fn try_collapse_sequence_at(&mut self, index: usize) {
        let mut bytes = [0; OP_STRING_LEN];
        let mut len = 0;

        let mut jumps = None;

        let mut sequence_len = 0;
        let mut op_index = index;
        while op_index + 1 < self.ops.len() {
            let (okj, erj, c) = match self.ops[op_index] {
                Op::Char(okj, erj, c) => (okj, erj, c),
                _ => break,
            };
            if op_index + 2 != okj.0 as _
                || op_index + 1 != erj.0 as _
                || len + c.len_utf8() > OP_STRING_LEN
            {
                break;
            }

            let (jump, count) = match self.ops[op_index + 1] {
                Op::Unwind(jump, count) => (jump, count),
                _ => break,
            };
            if sequence_len != count.0 as _ {
                break;
            }

            len += c.encode_utf8(&mut bytes[len..]).len();
            jumps = Some((okj, jump));
            sequence_len += 1;
            op_index += 2;
        }

        if sequence_len <= 1 {
            return;
        }

        let (okj, erj) = match jumps {
            Some(jumps) => jumps,
            None => return,
        };

        self.ops[index] = Op::String(okj, erj, len as _, bytes);
        self.ops.drain(index + 1..op_index);

        fn fix_jump(jump: &mut Jump, index: usize, fix: u16) {
            if jump.0 as usize > index {
                jump.0 -= fix;
            }
        }

        let fix = (sequence_len * 2 - 1) as _;
        fix_jump(&mut self.start_jump, index, fix);

        for op in self.ops.iter_mut() {
            match op {
                Op::Ok | Op::Error => (),
                Op::Reset(j) | Op::Unwind(j, _) => fix_jump(j, index, fix),
                Op::BeginningAnchor(okj, erj)
                | Op::EndingAnchor(okj, erj)
                | Op::WordBoundary(okj, erj)
                | Op::SkipOne(okj, erj)
                | Op::SkipMany(okj, erj, _)
                | Op::Alphabetic(okj, erj)
                | Op::Lower(okj, erj)
                | Op::Upper(okj, erj)
                | Op::Digit(okj, erj)
                | Op::Alphanumeric(okj, erj)
                | Op::Char(okj, erj, _)
                | Op::CharCaseInsensitive(okj, erj, _)
                | Op::String(okj, erj, _, _)
                | Op::StringCaseInsensitive(okj, erj, _, _) => {
                    fix_jump(okj, index, fix);
                    fix_jump(erj, index, fix);
                }
            }
        }
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
    fn search_anchor() {
        assert_eq!(None, new_pattern("").search_anchor());
        assert_eq!(Some('a'), new_pattern("a").search_anchor());
        assert_eq!(Some('a'), new_pattern("abc").search_anchor());
        assert_eq!(Some('a'), new_pattern("(abc)").search_anchor());
        assert_eq!(None, new_pattern(".").search_anchor());
        assert_eq!(None, new_pattern("%w").search_anchor());
        assert_eq!(None, new_pattern("%d").search_anchor());
        assert_eq!(Some('%'), new_pattern("%%").search_anchor());
        assert_eq!(None, new_pattern("[abc]").search_anchor());
        assert_eq!(None, new_pattern("{abc}").search_anchor());
        assert_eq!(None, new_pattern("abc|def").search_anchor());
    }

    #[test]
    fn simple_pattern() {
        let p = new_pattern("");
        assert_eq!(MatchResult::Ok(0), p.matches("", 0));
        assert_eq!(MatchResult::Ok(0), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(0), p.matches("z", 0));
        assert_eq!(MatchResult::Ok(0), p.matches("A", 0));
        assert_eq!(MatchResult::Ok(0), p.matches("Z", 0));
        assert_eq!(MatchResult::Ok(0), p.matches("0", 0));
        assert_eq!(MatchResult::Ok(0), p.matches("9", 0));
        assert_eq!(MatchResult::Ok(0), p.matches("!", 0));

        let p = new_pattern("a");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("aa", 0));
        assert_eq!(MatchResult::Err, p.matches("b", 0));
        assert_eq!(MatchResult::Err, p.matches("", 0));

        let p = new_pattern("aa");
        assert_eq!(MatchResult::Ok(2), p.matches("aa", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("aaa", 0));
        assert_eq!(MatchResult::Err, p.matches("baa", 0));

        let p = new_pattern("abc");
        assert_eq!(MatchResult::Ok(3), p.matches("abc", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("abcd", 0));
        assert_eq!(MatchResult::Err, p.matches("aabc", 0));

        let p = new_pattern("%% %$ %. %! %( %) %[ %] %{ %}");
        let matched_text = "% $ . ! ( ) [ ] { }";
        assert_eq!(
            MatchResult::Ok(matched_text.len()),
            p.matches(matched_text, 0)
        );

        let p = new_pattern(".");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("z", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("A", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("Z", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("0", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("9", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("!", 0));

        let p = new_pattern("%a");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("z", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("A", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("Z", 0));
        assert_eq!(MatchResult::Err, p.matches("0", 0));
        assert_eq!(MatchResult::Err, p.matches("9", 0));
        assert_eq!(MatchResult::Err, p.matches("!", 0));

        let p = new_pattern("%l");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("z", 0));
        assert_eq!(MatchResult::Err, p.matches("A", 0));
        assert_eq!(MatchResult::Err, p.matches("Z", 0));
        assert_eq!(MatchResult::Err, p.matches("0", 0));
        assert_eq!(MatchResult::Err, p.matches("9", 0));
        assert_eq!(MatchResult::Err, p.matches("!", 0));

        let p = new_pattern("%u");
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Err, p.matches("z", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("A", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("Z", 0));
        assert_eq!(MatchResult::Err, p.matches("0", 0));
        assert_eq!(MatchResult::Err, p.matches("9", 0));
        assert_eq!(MatchResult::Err, p.matches("!", 0));

        let p = new_pattern("%d");
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Err, p.matches("z", 0));
        assert_eq!(MatchResult::Err, p.matches("A", 0));
        assert_eq!(MatchResult::Err, p.matches("Z", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("0", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("9", 0));
        assert_eq!(MatchResult::Err, p.matches("!", 0));

        let p = new_pattern("%w");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("z", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("A", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("Z", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("0", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("9", 0));
        assert_eq!(MatchResult::Err, p.matches("!", 0));

        let p = new_pattern("abcdefghij");
        assert_eq!(MatchResult::Ok(10), p.matches("abcdefghij", 0));

        let p = new_pattern("abcdefghijk");
        assert_eq!(MatchResult::Ok(11), p.matches("abcdefghijk", 0));

        let p = new_pattern("abcdefghijklmnopqrstuvwxyz");
        assert_eq!(
            MatchResult::Ok(26),
            p.matches("abcdefghijklmnopqrstuvwxyz", 0)
        );
    }

    #[test]
    fn group() {
        let p = new_pattern("[abc]");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("b", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("c", 0));
        assert_eq!(MatchResult::Err, p.matches("d", 0));

        let p = new_pattern("z[abc]y");
        assert_eq!(MatchResult::Ok(3), p.matches("zay", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("zby", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("zcy", 0));
        assert_eq!(MatchResult::Err, p.matches("z", 0));
        assert_eq!(MatchResult::Err, p.matches("zy", 0));
        assert_eq!(MatchResult::Err, p.matches("zdy", 0));

        let p = new_pattern("z[a]");
        assert_eq!(MatchResult::Ok(2), p.matches("za", 0));
        assert_eq!(MatchResult::Err, p.matches("z", 0));
        assert_eq!(MatchResult::Err, p.matches("zb", 0));

        let p = new_pattern("z[%l%d]");
        assert_eq!(MatchResult::Ok(2), p.matches("za", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("zz", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("z0", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("z9", 0));
        assert_eq!(MatchResult::Err, p.matches("z", 0));
        assert_eq!(MatchResult::Err, p.matches("zA", 0));
        assert_eq!(MatchResult::Err, p.matches("zZ", 0));

        let p = new_pattern("[!abc]");
        assert_eq!(MatchResult::Ok(1), p.matches("d", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("3", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("@", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("@a", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("@b", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("@c", 0));
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Err, p.matches("b", 0));
        assert_eq!(MatchResult::Err, p.matches("c", 0));

        let p = new_pattern("x[!%w]y");
        assert_eq!(MatchResult::Err, p.matches("xay", 0));
        assert_eq!(MatchResult::Err, p.matches("xzy", 0));
        assert_eq!(MatchResult::Err, p.matches("xAy", 0));
        assert_eq!(MatchResult::Err, p.matches("xZy", 0));
        assert_eq!(MatchResult::Err, p.matches("x0y", 0));
        assert_eq!(MatchResult::Err, p.matches("x9y", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("x#y", 0));
    }

    #[test]
    fn sequence() {
        let p = new_pattern("(abc)");
        assert_eq!(MatchResult::Ok(3), p.matches("abc", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("abcd", 0));
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Err, p.matches("ab", 0));

        let p = new_pattern("z(abc)y");
        assert_eq!(MatchResult::Ok(5), p.matches("zabcy", 0));
        assert_eq!(MatchResult::Ok(5), p.matches("zabcyd", 0));
        assert_eq!(MatchResult::Err, p.matches("zay", 0));
        assert_eq!(MatchResult::Err, p.matches("zaby", 0));

        let p = new_pattern("z(%u%w)y");
        assert_eq!(MatchResult::Ok(4), p.matches("zA0y", 0));
        assert_eq!(MatchResult::Ok(4), p.matches("zZay", 0));
        assert_eq!(MatchResult::Ok(4), p.matches("zA0yA", 0));
        assert_eq!(MatchResult::Err, p.matches("zaay", 0));
        assert_eq!(MatchResult::Err, p.matches("z8ay", 0));

        let p = new_pattern("(!abc)");
        assert_eq!(MatchResult::Err, p.matches("abc", 0));
        assert_eq!(MatchResult::Err, p.matches("abcd", 0));
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Err, p.matches("ac", 0));
        assert_eq!(MatchResult::Err, p.matches("ab", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("abz", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("ab!", 0));
        assert_eq!(MatchResult::Err, p.matches("z", 0));
        assert_eq!(MatchResult::Err, p.matches("7a", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("7ab", 0));

        let p = new_pattern("(abcdefghij)");
        assert_eq!(MatchResult::Ok(10), p.matches("abcdefghij", 0));

        let p = new_pattern("(abcdefghijk)");
        assert_eq!(MatchResult::Ok(11), p.matches("abcdefghijk", 0));

        let p = new_pattern("(abcdefghijklmnopqrstuvwxyz)");
        assert_eq!(
            MatchResult::Ok(26),
            p.matches("abcdefghijklmnopqrstuvwxyz", 0)
        );
    }

    #[test]
    fn repeat() {
        let p = new_pattern("{a}");
        assert_eq!(MatchResult::Ok(0), p.matches("", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(4), p.matches("aaaa", 0));
        assert_eq!(MatchResult::Ok(0), p.matches("b", 0));

        let p = new_pattern("{a}b");
        assert_eq!(MatchResult::Ok(2), p.matches("ab", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("aab", 0));
        assert_eq!(MatchResult::Ok(5), p.matches("aaaab", 0));

        let p = new_pattern("a{b}c");
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Err, p.matches("ab", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("ac", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("abc", 0));
        assert_eq!(MatchResult::Ok(5), p.matches("abbbc", 0));

        let p = new_pattern("a{bc}d");
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("ad", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("abd", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("acd", 0));
        assert_eq!(MatchResult::Ok(4), p.matches("abcd", 0));
        assert_eq!(MatchResult::Ok(5), p.matches("abcbd", 0));
        assert_eq!(MatchResult::Ok(6), p.matches("abcbcd", 0));

        let p = new_pattern("a{b!c}d");
        assert_eq!(MatchResult::Err, p.matches("ad", 0));
        assert_eq!(MatchResult::Err, p.matches("abd", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("acd", 0));
        assert_eq!(MatchResult::Ok(5), p.matches("abbcd", 0));
    }

    #[test]
    fn middle_text_matching() {
        let p = new_pattern("abc");
        assert_eq!(MatchResult::Ok(4), p.matches("_abc", 1));
        assert_eq!(MatchResult::Ok(6), p.matches("abcabc", 3));

        let p = new_pattern("a|b");
        assert_eq!(MatchResult::Ok(2), p.matches("_a", 1));
        assert_eq!(MatchResult::Ok(2), p.matches("_b", 1));

        let p = new_pattern("/*{!(*/).}");
        assert_eq!(MatchResult::Ok(7), p.matches("a /* */", 2));
    }

    #[test]
    fn beginning_anchor() {
        let p = new_pattern("^abc");
        assert_eq!(MatchResult::Ok(3), p.matches("abc", 0));
        assert_eq!(MatchResult::Err, p.matches("_abc", 1));
    }

    #[test]
    fn ending_anchor() {
        let p = new_pattern("a$");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Err, p.matches("aa", 0));

        let p = new_pattern("a$b");
        assert_eq!(
            MatchResult::Pending(PatternState { op_jump: Jump(4) }),
            p.matches("a", 0)
        );
        assert_eq!(
            MatchResult::Ok(1),
            p.matches_with_state("b", 0, PatternState { op_jump: Jump(4) })
        );

        let p = new_pattern("a{.!$}b");
        match p.matches("axyz", 0) {
            MatchResult::Pending(state) => {
                assert_eq!(MatchResult::Ok(1), p.matches_with_state("b", 0, state))
            }
            _ => assert!(false),
        }

        let p = new_pattern("a[b(c$)]");
        assert_eq!(MatchResult::Ok(2), p.matches("ab", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("ac", 0));

        let p = new_pattern("a{b$!c}{c!d}");
        match p.matches("abb", 0) {
            MatchResult::Pending(state) => match p.matches_with_state("bb", 0, state) {
                MatchResult::Pending(state) => {
                    assert_eq!(MatchResult::Ok(4), p.matches_with_state("bccd", 0, state));
                }
                _ => assert!(false),
            },
            _ => assert!(false),
        }
    }

    #[test]
    fn word_boundary() {
        let p = new_pattern("%babc%b");
        assert_eq!(MatchResult::Ok(3), p.matches("abc", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("abc.", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("abc,def", 0));
        assert_eq!(MatchResult::Err, p.matches("abcd", 0));
        assert_eq!(MatchResult::Ok(4), p.matches(",abc,", 1));
        assert_eq!(MatchResult::Err, p.matches("xabc,", 1));
    }

    #[test]
    fn complex_pattern() {
        let p = new_pattern("{.!$}");
        assert_eq!(MatchResult::Ok(10), p.matches("things 890", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("0", 0));
        assert_eq!(MatchResult::Ok(1), p.matches(" ", 0));

        let p = new_pattern("{[ab%d]!c}");
        assert_eq!(MatchResult::Ok(1), p.matches("c", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("ac", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("bc", 0));
        assert_eq!(MatchResult::Ok(3), p.matches("bac", 0));
        assert_eq!(MatchResult::Ok(5), p.matches("0b4ac", 0));
        assert_eq!(MatchResult::Ok(14), p.matches("a1b234ba9bbbbc", 0));

        let p = new_pattern("%d{[%w_%.]!@}");
        assert_eq!(MatchResult::Ok(6), p.matches("1x4_5@", 0));
        assert_eq!(MatchResult::Ok(15), p.matches("9xxasd_234.45f@", 0));

        let p = new_pattern("ab{(!ba)!b}a");
        assert_eq!(MatchResult::Ok(4), p.matches("abba", 0));
    }

    #[test]
    fn edge_cases() {
        let p = new_pattern("(!(!abc))");
        assert_eq!(MatchResult::Ok(3), p.matches("abc", 0));
        assert_eq!(MatchResult::Err, p.matches("xyz", 0));
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Err, p.matches("ab", 0));
        assert_eq!(MatchResult::Err, p.matches("abz", 0));

        let p = new_pattern("[![!abc]]");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("b", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("c", 0));
        assert_eq!(MatchResult::Err, p.matches("x", 0));

        let p = new_pattern("()");
        assert_eq!(MatchResult::Ok(0), p.matches("", 0));
        assert_eq!(MatchResult::Ok(0), p.matches("x", 0));
    }

    #[test]
    fn pattern_composition() {
        assert!(matches!(
            try_new_pattern("[(ab)c]"),
            Err(PatternError::GroupWithElementsOfDifferentSize)
        ));

        let p = new_pattern("[(ab)(cd)]");
        assert_eq!(MatchResult::Ok(2), p.matches("ab", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("cd", 0));
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Err, p.matches("c", 0));
        assert_eq!(MatchResult::Err, p.matches("ad", 0));
        assert_eq!(MatchResult::Err, p.matches("cb", 0));

        let p = new_pattern("[![(ab)(cd)]]");
        assert_eq!(MatchResult::Ok(2), p.matches("ad", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("bc", 0));
        assert_eq!(MatchResult::Err, p.matches("ab", 0));
        assert_eq!(MatchResult::Err, p.matches("cd", 0));

        let p = new_pattern("[(ab)(!cd)]");
        assert_eq!(MatchResult::Ok(2), p.matches("ab", 0));
        assert_eq!(MatchResult::Err, p.matches("b", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("ax", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("ac", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("acd", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("cb", 0));

        let p = new_pattern("{(a[!ab])!x!$}");
        assert_eq!(MatchResult::Ok(0), p.matches("", 0));
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("ac", 0));
        assert_eq!(MatchResult::Err, p.matches("aca", 0));
        assert_eq!(MatchResult::Err, p.matches("acab", 0));
        assert_eq!(MatchResult::Ok(4), p.matches("acax", 0));

        let p = new_pattern("{[(!ab)(cd)]!$}");
        assert_eq!(MatchResult::Ok(0), p.matches("", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("cd", 0));
        assert_eq!(MatchResult::Err, p.matches("ab", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("ac", 0));
        assert_eq!(MatchResult::Ok(4), p.matches("accd", 0));
    }

    #[test]
    fn multi_subpatterns() {
        let p = new_pattern("a|b");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("b", 0));
        assert_eq!(MatchResult::Err, p.matches("c", 0));
        assert_eq!(MatchResult::Err, p.matches("", 0));

        let p = new_pattern("ab{(ab)}|c");
        assert_eq!(MatchResult::Err, p.matches("a", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("ab", 0));
        assert_eq!(MatchResult::Ok(2), p.matches("aba", 0));
        assert_eq!(MatchResult::Ok(4), p.matches("abab", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("c", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("ca", 0));
        assert_eq!(MatchResult::Ok(1), p.matches("cab", 0));
    }

    #[test]
    fn utf8() {
        let p = new_pattern("[açé]");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok('ç'.len_utf8()), p.matches("ç", 0));
        assert_eq!(MatchResult::Ok('é'.len_utf8()), p.matches("é", 0));

        let p = new_pattern(".");
        assert_eq!(MatchResult::Ok(1), p.matches("a", 0));
        assert_eq!(MatchResult::Ok('ç'.len_utf8()), p.matches("ç", 0));
        assert_eq!(MatchResult::Ok('é'.len_utf8()), p.matches("é", 0));
    }

    #[test]
    fn bad_pattern() {
        assert!(matches!(
            try_new_pattern("("),
            Err(PatternError::UnexpectedEndOfPattern)
        ));
        assert!(matches!(
            try_new_pattern(")"),
            Err(PatternError::Unescaped(')'))
        ));
        assert!(matches!(
            try_new_pattern("["),
            Err(PatternError::UnexpectedEndOfPattern)
        ));
        assert!(matches!(
            try_new_pattern("]"),
            Err(PatternError::Unescaped(']'))
        ));
        assert!(matches!(
            try_new_pattern("[]"),
            Err(PatternError::EmptyGroup)
        ));
        assert!(matches!(
            try_new_pattern("{"),
            Err(PatternError::UnexpectedEndOfPattern)
        ));
        assert!(matches!(
            try_new_pattern("}"),
            Err(PatternError::Unescaped('}'))
        ));
        assert!(matches!(
            try_new_pattern("%"),
            Err(PatternError::UnexpectedEndOfPattern)
        ));
        assert!(matches!(
            try_new_pattern("!"),
            Err(PatternError::Unescaped('!'))
        ));
        assert!(matches!(
            try_new_pattern("%@"),
            Err(PatternError::InvalidEscaping('@'))
        ));
        assert!(matches!(
            try_new_pattern("|"),
            Err(PatternError::Unescaped('|'))
        ));
        assert!(matches!(
            try_new_pattern("a|"),
            Err(PatternError::UnexpectedEndOfPattern)
        ));
    }
}
