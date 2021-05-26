use std::{convert::From, fmt, ops::AddAssign, str::Chars};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    Pending(PatternState),
    Ok(usize),
    Err,
}

#[derive(Debug)]
pub enum PatternErrorKind {
    UnexpectedEndOfPattern,
    Expected(char),
    InvalidEscaping(char),
    Unescaped(char),
    EmptyGroup,
    GroupWithElementsOfDifferentSize,
}
impl fmt::Display for PatternErrorKind {
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

#[derive(Debug)]
pub struct PatternError<'a> {
    pub pattern: &'a str,
    pub kind: PatternErrorKind,
}
impl<'a> fmt::Display for PatternError<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid pattern '{}': {}", self.pattern, self.kind)
    }
}

pub struct MatchIndices<'pattern, 'text> {
    pattern: &'pattern Pattern,
    text: &'text str,
    index: usize,
    anchor: Option<char>,
}
impl<'pattern, 'text> Iterator for MatchIndices<'pattern, 'text> {
    type Item = (usize, &'text str);
    fn next(&mut self) -> Option<Self::Item> {
        #[inline]
        fn next_char(iter: &mut MatchIndices) -> Option<()> {
            let mut chars = iter.text.chars();
            let c = chars.next()?;
            iter.text = chars.as_str();
            iter.index += c.len_utf8();
            Some(())
        }

        loop {
            if let Some(anchor) = self.anchor {
                match self.text.find(anchor) {
                    Some(i) => {
                        self.text = &self.text[i..];
                        self.index += i;
                    }
                    None => {
                        self.text = "";
                        return None;
                    }
                }
            }
            match self.pattern.matches(self.text) {
                MatchResult::Ok(0) => next_char(self)?,
                MatchResult::Ok(len) => {
                    let result = (self.index, &self.text[..len]);
                    self.text = &self.text[len..];
                    self.index += len;
                    return Some(result);
                }
                _ => next_char(self)?,
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

    pub fn clear(&mut self) {
        self.ops.clear();
        self.ops.push(Op::Error);
        self.start_jump = Jump(0);
    }

    pub fn compile<'a>(&mut self, pattern: &'a str) -> Result<(), PatternError<'a>> {
        match PatternCompiler::new(&mut self.ops, pattern).compile() {
            Ok(start_jump) => {
                self.start_jump = start_jump;
                Ok(())
            }
            Err(kind) => {
                self.clear();
                Err(PatternError { pattern, kind })
            }
        }
    }

    pub fn compile_searcher<'a>(&mut self, pattern: &'a str) -> Result<(), PatternError<'a>> {
        let (ignore_case, pattern) = match pattern.strip_prefix('_') {
            Some(pattern) => (false, pattern),
            None => {
                let ignore_case = pattern.chars().all(char::is_lowercase);
                (ignore_case, pattern)
            }
        };
        match pattern.strip_prefix('%') {
            Some(pattern) => self.compile(pattern)?,
            None => {
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
            }
        }
        if ignore_case {
            self.ignore_case();
        }
        Ok(())
    }

    pub fn ignore_case(&mut self) {
        for op in &mut self.ops {
            match op {
                &mut Op::Char(okj, erj, c) => *op = Op::CharCaseInsensitive(okj, erj, c),
                &mut Op::String(okj, erj, len, bytes) => {
                    *op = Op::StringCaseInsensitive(okj, erj, len, bytes)
                }
                _ => (),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.ops[self.start_jump.0 as usize], Op::Ok | Op::Error)
    }

    pub fn search_anchor(&self) -> Option<char> {
        let (c, erj) = match self.ops[self.start_jump.0 as usize] {
            Op::Error => return Some('\0'),
            Op::Char(_, erj, c) => (c, erj),
            Op::String(_, erj, len, bytes) => {
                let s = std::str::from_utf8(&bytes[..len as usize]).unwrap();
                let c = s.chars().next()?;
                (c, erj)
            }
            _ => return None,
        };

        match self.ops[erj.0 as usize] {
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

    pub fn matches(&self, text: &str) -> MatchResult {
        self.matches_with_state(
            text,
            PatternState {
                op_index: self.start_jump.0 as _,
            },
        )
    }

    pub fn matches_with_state(&self, text: &str, state: PatternState) -> MatchResult {
        let mut chars = text.chars();
        let ops = &self.ops;
        let mut op_index = state.op_index;

        #[inline]
        fn index(text: &str, chars: &Chars) -> usize {
            chars.as_str().as_ptr() as usize - text.as_ptr() as usize
        }

        #[inline]
        fn check_and_jump<F>(chars: &mut Chars, okj: Jump, erj: Jump, predicate: F) -> usize
        where
            F: Fn(char) -> bool,
        {
            let previous_state = chars.clone();
            match chars.next() {
                Some(c) if predicate(c) => okj.0 as _,
                _ => {
                    *chars = previous_state;
                    erj.0 as _
                }
            }
        }

        loop {
            match ops[op_index] {
                Op::Ok => return MatchResult::Ok(index(text, &chars)),
                Op::Error => return MatchResult::Err,
                Op::Reset(jump) => {
                    chars = text.chars();
                    op_index = jump.0 as _;
                }
                Op::Unwind(jump, len) => {
                    let len = (len.0 - 1) as _;
                    let index = index(text, &chars);
                    chars = match text[..index].char_indices().rev().nth(len) {
                        Some((i, _)) => text[i..].chars(),
                        None => unreachable!(),
                    };
                    op_index = jump.0 as _;
                }
                Op::EndAnchor(okj, erj) => {
                    if chars.as_str().is_empty() {
                        op_index = okj.0 as _;
                        return match ops[op_index] {
                            Op::Ok => MatchResult::Ok(index(text, &chars)),
                            _ => MatchResult::Pending(PatternState { op_index }),
                        };
                    } else {
                        op_index = erj.0 as _;
                    }
                }
                Op::SkipOne(okj, erj) => op_index = check_and_jump(&mut chars, okj, erj, |_| true),
                Op::SkipMany(okj, erj, len) => {
                    let len = (len.0 - 1) as _;
                    op_index = match chars.nth(len) {
                        Some(_) => okj.0 as _,
                        None => erj.0 as _,
                    };
                }
                Op::Alphabetic(okj, erj) => {
                    op_index = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_alphabetic());
                }
                Op::Lower(okj, erj) => {
                    op_index = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_lowercase());
                }
                Op::Upper(okj, erj) => {
                    op_index = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_uppercase());
                }
                Op::Digit(okj, erj) => {
                    op_index = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_digit());
                }
                Op::Alphanumeric(okj, erj) => {
                    op_index = check_and_jump(&mut chars, okj, erj, |c| c.is_ascii_alphanumeric());
                }
                Op::Char(okj, erj, ch) => {
                    op_index = check_and_jump(&mut chars, okj, erj, |c| c == ch)
                }
                Op::CharCaseInsensitive(okj, erj, ch) => {
                    op_index = check_and_jump(&mut chars, okj, erj, |c| c.eq_ignore_ascii_case(&ch))
                }
                Op::String(okj, erj, len, bytes) => {
                    let len = len as usize;
                    let bytes = &bytes[..len];
                    let text_bytes = chars.as_str().as_bytes();
                    op_index = if text_bytes.len() >= len && &text_bytes[..len] == bytes {
                        chars = chars.as_str()[len..].chars();
                        okj.0 as _
                    } else {
                        erj.0 as _
                    }
                }
                Op::StringCaseInsensitive(okj, erj, len, bytes) => {
                    let len = len as usize;
                    let bytes = &bytes[..len];
                    let text_bytes = chars.as_str().as_bytes();
                    op_index = if text_bytes.len() >= len
                        && text_bytes[..len].eq_ignore_ascii_case(bytes)
                    {
                        chars = chars.as_str()[len..].chars();
                        okj.0 as _
                    } else {
                        erj.0 as _
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

const OP_STRING_LEN: usize = 10;

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
    Char(Jump, Jump, char),
    CharCaseInsensitive(Jump, Jump, char),
    String(Jump, Jump, u8, [u8; OP_STRING_LEN]),
    StringCaseInsensitive(Jump, Jump, u8, [u8; OP_STRING_LEN]),
}

impl fmt::Debug for Op {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const WIDTH: usize = 14;

        fn p(f: &mut fmt::Formatter, name: &str, okj: Jump, erj: Jump) -> fmt::Result {
            f.write_fmt(format_args!(
                "{:width$}{} {}",
                name,
                okj.0,
                erj.0,
                width = WIDTH
            ))
        }

        match self {
            &Op::Ok => f.write_str("Ok"),
            &Op::Error => f.write_str("Error"),
            &Op::Reset(jump) => f.write_fmt(format_args!(
                "{:width$} {}",
                "Reset",
                jump.0,
                width = WIDTH - 4,
            )),
            &Op::Unwind(jump, len) => f.write_fmt(format_args!(
                "{:width$}[{}] {}",
                "Unwind",
                len.0,
                jump.0,
                width = WIDTH - 4
            )),
            &Op::EndAnchor(okj, erj) => p(f, "EndAnchor", okj, erj),
            &Op::SkipOne(okj, erj) => p(f, "SkipOne", okj, erj),
            &Op::SkipMany(okj, erj, len) => f.write_fmt(format_args!(
                "{:width$}[{}] {} {}",
                "SkipMany",
                len.0,
                okj.0,
                erj.0,
                width = WIDTH - 4
            )),
            &Op::Alphabetic(okj, erj) => p(f, "Alphabetic", okj, erj),
            &Op::Lower(okj, erj) => p(f, "Lower", okj, erj),
            &Op::Upper(okj, erj) => p(f, "Upper", okj, erj),
            &Op::Digit(okj, erj) => p(f, "Digit", okj, erj),
            &Op::Alphanumeric(okj, erj) => p(f, "Alphanumeric", okj, erj),
            &Op::Char(okj, erj, c) => f.write_fmt(format_args!(
                "{:width$}'{}' {} {}",
                "Char",
                c,
                okj.0,
                erj.0,
                width = WIDTH - 4
            )),
            &Op::CharCaseInsensitive(okj, erj, c) => f.write_fmt(format_args!(
                "{:width$}'{}' {} {}",
                "CharCaseInsensitive",
                c,
                okj.0,
                erj.0,
                width = WIDTH - 4
            )),
            &Op::String(okj, erj, len, bytes) => f.write_fmt(format_args!(
                "{:width$}'{}' {} {}",
                "String",
                std::str::from_utf8(&bytes[..len as usize]).unwrap(),
                okj.0,
                erj.0,
                width = WIDTH - 4
            )),
            &Op::StringCaseInsensitive(okj, erj, len, bytes) => f.write_fmt(format_args!(
                "{:width$}'{}' {} {}",
                "StringCaseInsensitive",
                std::str::from_utf8(&bytes[..len as usize]).unwrap(),
                okj.0,
                erj.0,
                width = WIDTH - 4
            )),
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

    pub fn compile(mut self) -> Result<Jump, PatternErrorKind> {
        self.ops.push(Op::Error);
        self.ops.push(Op::Ok);
        self.parse_subpatterns()?;
        self.optimize();
        Ok(self.start_jump)
    }

    fn assert_current(&self, c: char) -> Result<(), PatternErrorKind> {
        if self.current_char == c {
            Ok(())
        } else {
            Err(PatternErrorKind::Expected(c))
        }
    }

    fn next(&mut self) -> Result<char, PatternErrorKind> {
        match self.text.next() {
            Some(c) => {
                self.current_char = c;
                Ok(c)
            }
            None => Err(PatternErrorKind::UnexpectedEndOfPattern),
        }
    }

    fn next_is(&mut self, c: char) -> Result<bool, PatternErrorKind> {
        match self.next() {
            Ok(ch) => Ok(ch == c),
            Err(e) => Err(e),
        }
    }

    fn parse_subpatterns(&mut self) -> Result<(), PatternErrorKind> {
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
                if self.current_char == '|' {
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

    fn parse_stmt(&mut self, erj: JumpFrom) -> Result<(), PatternErrorKind> {
        match self.current_char {
            '{' => self.parse_repeat_stmt(erj),
            _ => match self.parse_expr(JumpFrom::End(Jump(0)), erj) {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
        }
    }

    fn parse_expr(&mut self, okj: JumpFrom, erj: JumpFrom) -> Result<Length, PatternErrorKind> {
        let len = match self.current_char {
            '(' => self.parse_sequence_expr(okj, erj)?,
            '[' => self.parse_group_expr(okj, erj)?,
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

    fn parse_repeat_stmt(&mut self, erj: JumpFrom) -> Result<(), PatternErrorKind> {
        let start_jump = self.ops.len().into();
        let end_jump = self.get_absolute_jump(JumpFrom::End(Jump(0)));

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
            self.jump_at_end(erj);
        }

        self.patch_unwind_jump(JumpFrom::End(Jump(0)), end_jump);

        self.assert_current('}')?;
        Ok(())
    }

    fn parse_sequence_expr(
        &mut self,
        okj: JumpFrom,
        erj: JumpFrom,
    ) -> Result<Length, PatternErrorKind> {
        let previous_state = self.text.clone();
        let mut len = Length(0);

        if self.next()? == '!' {
            let abs_erj = self.get_absolute_jump(erj);
            while !self.next_is(')')? {
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
            self.text = previous_state;
            let abs_erj = self.get_absolute_jump(erj);
            while !self.next_is(')')? {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(1)), JumpFrom::End(Jump(0)))?;
                self.ops.push(Op::Unwind(abs_erj, len));
                len += expr_len;
            }
            self.jump_at_end(okj);
            self.patch_unwind_jump(erj, abs_erj);
        }

        self.assert_current(')')?;
        Ok(len)
    }

    fn parse_group_expr(
        &mut self,
        okj: JumpFrom,
        erj: JumpFrom,
    ) -> Result<Length, PatternErrorKind> {
        let previous_state = self.text.clone();
        let mut len = None;

        if self.next()? == '!' {
            let abs_erj = self.get_absolute_jump(erj);
            while !self.next_is(']')? {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(0)), JumpFrom::End(Jump(1)))?;
                self.ops.push(Op::Unwind(abs_erj, expr_len));

                if len.unwrap_or(expr_len).0 != expr_len.0 {
                    return Err(PatternErrorKind::GroupWithElementsOfDifferentSize);
                }
                len = Some(expr_len);
            }

            let len = len.ok_or(PatternErrorKind::EmptyGroup)?;
            match okj {
                JumpFrom::Beginning(jump) => self.skip(jump, abs_erj, len),
                JumpFrom::End(mut jump) => {
                    jump += (self.ops.len() + 1).into();
                    self.skip(jump, abs_erj, len);
                }
            }
            self.patch_unwind_jump(erj, abs_erj);
        } else {
            self.text = previous_state;
            let abs_okj = self.get_absolute_jump(okj);
            while !self.next_is(']')? {
                let expr_len =
                    self.parse_expr(JumpFrom::Beginning(abs_okj), JumpFrom::End(Jump(0)))?;

                if len.unwrap_or(expr_len).0 != expr_len.0 {
                    return Err(PatternErrorKind::GroupWithElementsOfDifferentSize);
                }
                len = Some(expr_len);
            }
            self.jump_at_end(erj);
            self.patch_unwind_jump(okj, abs_okj);
        }

        self.assert_current(']')?;
        len.ok_or(PatternErrorKind::EmptyGroup)
    }

    fn parse_class_expr(
        &mut self,
        okj: JumpFrom,
        erj: JumpFrom,
    ) -> Result<Length, PatternErrorKind> {
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

        let op = match self.current_char {
            '%' => match self.next()? {
                'a' => Op::Alphabetic(okj, erj),
                'l' => Op::Lower(okj, erj),
                'u' => Op::Upper(okj, erj),
                'd' => Op::Digit(okj, erj),
                'w' => Op::Alphanumeric(okj, erj),
                '%' => Op::Char(okj, erj, '%'),
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
                c => return Err(PatternErrorKind::InvalidEscaping(c)),
            },
            '$' => {
                self.ops.push(Op::EndAnchor(okj, erj));
                return Ok(Length(0));
            }
            '.' => Op::SkipOne(okj, erj),
            '!' => return Err(PatternErrorKind::Unescaped('!')),
            '(' => return Err(PatternErrorKind::Unescaped('(')),
            ')' => return Err(PatternErrorKind::Unescaped(')')),
            '[' => return Err(PatternErrorKind::Unescaped('[')),
            ']' => return Err(PatternErrorKind::Unescaped(']')),
            '{' => return Err(PatternErrorKind::Unescaped('{')),
            '}' => return Err(PatternErrorKind::Unescaped('}')),
            '|' => return Err(PatternErrorKind::Unescaped('|')),
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

        #[inline]
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
                Op::EndAnchor(okj, erj)
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

        #[inline]
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
                Op::EndAnchor(okj, erj)
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

        #[inline]
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
                Op::EndAnchor(okj, erj)
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
    fn assert_size() {
        assert_eq!(16, std::mem::size_of::<Op>());
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

        let p = new_pattern("abcdefghij");
        assert_eq!(MatchResult::Ok(10), p.matches("abcdefghij"));

        let p = new_pattern("abcdefghijk");
        assert_eq!(MatchResult::Ok(11), p.matches("abcdefghijk"));

        let p = new_pattern("abcdefghijklmnopqrstuvwxyz");
        assert_eq!(MatchResult::Ok(26), p.matches("abcdefghijklmnopqrstuvwxyz"));
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

        let p = new_pattern("(abcdefghij)");
        assert_eq!(MatchResult::Ok(10), p.matches("abcdefghij"));

        let p = new_pattern("(abcdefghijk)");
        assert_eq!(MatchResult::Ok(11), p.matches("abcdefghijk"));

        let p = new_pattern("(abcdefghijklmnopqrstuvwxyz)");
        assert_eq!(MatchResult::Ok(26), p.matches("abcdefghijklmnopqrstuvwxyz"));
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
            p.matches_with_state("b", PatternState { op_index: 4 })
        );

        let p = new_pattern("a{.!$}b");
        match p.matches("axyz") {
            MatchResult::Pending(state) => {
                assert_eq!(MatchResult::Ok(1), p.matches_with_state("b", state))
            }
            _ => assert!(false),
        }

        let p = new_pattern("a{b$!c}{c!d}");
        match p.matches("abb") {
            MatchResult::Pending(state) => match p.matches_with_state("bb", state) {
                MatchResult::Pending(state) => {
                    assert_eq!(MatchResult::Ok(4), p.matches_with_state("bccd", state));
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
            Err(PatternError {
                kind: PatternErrorKind::GroupWithElementsOfDifferentSize,
                ..
            })
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
    fn utf8() {
        let p = new_pattern("[açé]");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok('ç'.len_utf8()), p.matches("ç"));
        assert_eq!(MatchResult::Ok('é'.len_utf8()), p.matches("é"));

        let p = new_pattern(".");
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok('ç'.len_utf8()), p.matches("ç"));
        assert_eq!(MatchResult::Ok('é'.len_utf8()), p.matches("é"));
    }

    #[test]
    fn bad_pattern() {
        assert!(matches!(
            try_new_pattern("("),
            Err(PatternError {
                kind: PatternErrorKind::UnexpectedEndOfPattern,
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern(")"),
            Err(PatternError {
                kind: PatternErrorKind::Unescaped(')'),
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("["),
            Err(PatternError {
                kind: PatternErrorKind::UnexpectedEndOfPattern,
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("]"),
            Err(PatternError {
                kind: PatternErrorKind::Unescaped(']'),
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("[]"),
            Err(PatternError {
                kind: PatternErrorKind::EmptyGroup,
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("{"),
            Err(PatternError {
                kind: PatternErrorKind::UnexpectedEndOfPattern,
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("}"),
            Err(PatternError {
                kind: PatternErrorKind::Unescaped('}'),
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("%"),
            Err(PatternError {
                kind: PatternErrorKind::UnexpectedEndOfPattern,
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("!"),
            Err(PatternError {
                kind: PatternErrorKind::Unescaped('!'),
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("%@"),
            Err(PatternError {
                kind: PatternErrorKind::InvalidEscaping('@'),
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("|"),
            Err(PatternError {
                kind: PatternErrorKind::Unescaped('|'),
                ..
            })
        ));
        assert!(matches!(
            try_new_pattern("a|"),
            Err(PatternError {
                kind: PatternErrorKind::UnexpectedEndOfPattern,
                ..
            })
        ));
    }
}

