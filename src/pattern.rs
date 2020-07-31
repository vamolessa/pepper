use std::{convert::From, fmt, ops::AddAssign};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchResult {
    Pending(usize, PatternState),
    Ok(usize),
    Err,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternState {
    op_index: usize,
}

pub struct Pattern {
    ops: Vec<Op>,
}

impl Pattern {
    pub fn new(pattern: &str) -> Option<Self> {
        Some(PatternParser::new(pattern.as_bytes()).parse()?)
    }

    pub fn matches(&self, text: &str) -> MatchResult {
        self.matches_with_state(text, &PatternState { op_index: 1 })
    }

    pub fn matches_with_state(&self, text: &str, state: &PatternState) -> MatchResult {
        let bytes = text.as_bytes();
        let ops = &self.ops[..];
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

        if bytes_index < bytes.len() {
            eprintln!("{:?}", std::str::from_utf8(&bytes[bytes_index..]).unwrap());
        } else {
            eprintln!("\"\"");
        }

        loop {
            eprintln!("[{}] {:?}", op_index, ops[op_index]);
            match ops[op_index] {
                Op::Ok => return MatchResult::Ok(bytes_index),
                Op::Error => return MatchResult::Err,
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
                Op::Any(okj, erj) => check!(true, okj, erj),
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
                Op::Jump(jump) => op_index = jump.0 as _,
                Op::Unwind(jump, len) => {
                    bytes_index -= len.0 as usize;
                    op_index = jump.0 as _;
                }
            };

            if bytes_index < bytes.len() {
                eprintln!("{:?}", std::str::from_utf8(&bytes[bytes_index..]).unwrap());
            } else {
                eprintln!("\"\"");
            }
        }
    }
}

impl fmt::Debug for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Pattern ")?;
        f.debug_map()
            .entries(self.ops.iter().enumerate().map(|(i, op)| (i, op)))
            .finish()
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

enum Op {
    Ok,
    Error,
    Jump(Jump),
    EndAnchor(Jump, Jump),
    Any(Jump, Jump),
    Alphabetic(Jump, Jump),
    Lower(Jump, Jump),
    Upper(Jump, Jump),
    Digit(Jump, Jump),
    Alphanumeric(Jump, Jump),
    Byte(Jump, Jump, u8),
    Unwind(Jump, Length),
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
            Op::Jump(jump) => f.write_fmt(format_args!(
                "{:width$}[{}]",
                "Jump",
                jump.0,
                width = WIDTH - 4
            )),
            Op::EndAnchor(okj, erj) => p!("EndAnchor", okj, erj),
            Op::Any(okj, erj) => p!("Any", okj, erj),
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
            Op::Unwind(jump, len) => f.write_fmt(format_args!(
                "{:width$}[{}] {}",
                "Unwind",
                len.0,
                jump.0,
                width = WIDTH - 4
            )),
        }
    }
}

struct PatternParser<'a> {
    pub bytes: &'a [u8],
    pub index: usize,
    pub ops: Vec<Op>,
}

impl<'a> PatternParser<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            index: 0,
            ops: Vec::new(),
        }
    }

    pub fn parse(mut self) -> Option<Pattern> {
        self.ops.push(Op::Error);
        while let Some(_) = self.next() {
            self.parse_expr(JumpFrom::End(Jump(0)), JumpFrom::Beginning(Jump(0)))?;
        }
        self.ops.push(Op::Ok);
        Some(Pattern { ops: self.ops })
    }

    fn peek(&self) -> Option<u8> {
        if self.index < self.bytes.len() {
            Some(self.bytes[self.index])
        } else {
            None
        }
    }

    fn current(&self) -> u8 {
        self.bytes[self.index - 1]
    }

    fn next(&mut self) -> Option<u8> {
        if let Some(b) = self.peek() {
            self.index += 1;
            Some(b)
        } else {
            None
        }
    }

    fn next_is_not(&mut self, byte: u8) -> bool {
        if let Some(b) = self.next() {
            b != byte
        } else {
            true
        }
    }

    fn parse_expr(&mut self, okj: JumpFrom, erj: JumpFrom) -> Option<Length> {
        let len = match self.current() {
            b'*' => self.parse_repeat(okj)?,
            b'(' => self.parse_sequence(okj, erj)?,
            b'[' => self.parse_group(okj, erj)?,
            _ => self.parse_class(okj, erj)?,
        };

        Some(len)
    }

    fn get_absolute_jump(&mut self, jump: JumpFrom) -> Jump {
        match jump {
            JumpFrom::Beginning(jump) => jump,
            JumpFrom::End(_) => {
                let jump = (self.ops.len() + 2).into();
                self.ops.push(Op::Jump(jump));
                let jump = self.ops.len().into();
                self.ops.push(Op::Jump(jump));
                jump
            }
        }
    }

    fn patch_jump(&mut self, jump: JumpFrom, abs_jump: Jump) {
        if let JumpFrom::End(mut jump) = jump {
            jump += self.ops.len().into();
            if let Op::Jump(j) = &mut self.ops[abs_jump.0 as usize] {
                *j = jump;
            } else {
                unreachable!();
            }
        }
    }

    fn jump_at_end(&mut self, jump: JumpFrom) {
        match jump {
            JumpFrom::Beginning(jump) => self.ops.push(Op::Jump(jump)),
            JumpFrom::End(Jump(0)) => (),
            JumpFrom::End(mut jump) => {
                jump += (self.ops.len() + 1).into();
                self.ops.push(Op::Jump(jump));
            }
        }
    }

    fn parse_repeat(&mut self, okj: JumpFrom) -> Option<Length> {
        self.next()?;
        let jump = self.ops.len().into();
        self.parse_expr(JumpFrom::Beginning(jump), okj)?;
        Some(Length(0))
    }

    fn parse_sequence(&mut self, okj: JumpFrom, erj: JumpFrom) -> Option<Length> {
        let inverse = self.peek()? == b'^';
        let mut len = Length(0);

        if inverse {
            self.next()?;

            let abs_erj = self.get_absolute_jump(erj);
            while self.next_is_not(b')') {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(2)), JumpFrom::End(Jump(0)))?;
                self.ops.push(Op::Any(
                    (self.ops.len() + 3).into(),
                    (self.ops.len() + 1).into(),
                ));
                self.ops.push(Op::Unwind(abs_erj, len));
                len += expr_len;
            }
            self.ops.push(Op::Unwind(abs_erj, len));
            self.jump_at_end(okj);
            self.patch_jump(erj, abs_erj);
        } else {
            let abs_erj = self.get_absolute_jump(erj);
            while self.next_is_not(b')') {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(1)), JumpFrom::End(Jump(0)))?;
                self.ops.push(Op::Unwind(abs_erj, len));
                len += expr_len;
            }
            self.jump_at_end(okj);
            self.patch_jump(erj, abs_erj);
        }

        if self.current() == b')' {
            Some(len)
        } else {
            None
        }
    }

    fn parse_group(&mut self, okj: JumpFrom, erj: JumpFrom) -> Option<Length> {
        let inverse = self.peek()? == b'^';
        let mut len = Length(0);

        if inverse {
            self.next()?;

            let abs_erj = self.get_absolute_jump(erj);
            while self.next_is_not(b']') {
                let expr_len = self.parse_expr(JumpFrom::End(Jump(0)), JumpFrom::End(Jump(1)))?;
                self.ops.push(Op::Unwind(abs_erj, expr_len));
                len += expr_len;
            }
            match okj {
                JumpFrom::Beginning(jump) => self.ops.push(Op::Any(jump, abs_erj)),
                JumpFrom::End(mut jump) => {
                    jump += (self.ops.len() + 1).into();
                    self.ops.push(Op::Any(jump, abs_erj));
                }
            }
            self.patch_jump(erj, abs_erj);
        } else {
            let abs_okj = self.get_absolute_jump(okj);
            while self.next_is_not(b']') {
                len += self.parse_expr(JumpFrom::Beginning(abs_okj), JumpFrom::End(Jump(0)))?;
            }
            self.jump_at_end(erj);
            self.patch_jump(okj, abs_okj);
        }

        if self.current() == b']' {
            Some(len)
        } else {
            None
        }
    }

    fn parse_class(&mut self, okj: JumpFrom, erj: JumpFrom) -> Option<Length> {
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
                b'^' => Op::Byte(okj, erj, b'^'),
                b'(' => Op::Byte(okj, erj, b'('),
                b')' => Op::Byte(okj, erj, b')'),
                b'[' => Op::Byte(okj, erj, b'['),
                b']' => Op::Byte(okj, erj, b']'),
                b'*' => Op::Byte(okj, erj, b'*'),
                _ => return None,
            },
            b'$' => Op::EndAnchor(okj, erj),
            b'.' => Op::Any(okj, erj),
            b'^' => return None,
            b'(' => return None,
            b')' => return None,
            b'[' => return None,
            b']' => return None,
            b'*' => return None,
            b => Op::Byte(okj, erj, b),
        };

        self.ops.push(op);
        Some(Length(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_pattern() {
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

        let p = Pattern::new("%% %$ %. %^ %( %) %[ %] %*").unwrap();
        assert_eq!(MatchResult::Ok(17), p.matches("% $ . ^ ( ) [ ] *"));

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
    fn test_group() {
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

        let p = Pattern::new("[^abc]").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("d"));
        assert_eq!(MatchResult::Ok(1), p.matches("3"));
        assert_eq!(MatchResult::Ok(1), p.matches("@"));
        assert_eq!(MatchResult::Ok(1), p.matches("@a"));
        assert_eq!(MatchResult::Ok(1), p.matches("@b"));
        assert_eq!(MatchResult::Ok(1), p.matches("@c"));
        assert_eq!(MatchResult::Err, p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("b"));
        assert_eq!(MatchResult::Err, p.matches("c"));

        let p = Pattern::new("x[^%w]y").unwrap();
        assert_eq!(MatchResult::Err, p.matches("xay"));
        assert_eq!(MatchResult::Err, p.matches("xzy"));
        assert_eq!(MatchResult::Err, p.matches("xAy"));
        assert_eq!(MatchResult::Err, p.matches("xZy"));
        assert_eq!(MatchResult::Err, p.matches("x0y"));
        assert_eq!(MatchResult::Err, p.matches("x9y"));
        assert_eq!(MatchResult::Ok(3), p.matches("x#y"));
    }

    #[test]
    fn test_sequence() {
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

        let p = Pattern::new("(^abc)").unwrap();
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
    fn test_repeat() {
        let p = Pattern::new("*a").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Ok(4), p.matches("aaaa"));

        let p = Pattern::new("*ab").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches("ab"));
        assert_eq!(MatchResult::Ok(3), p.matches("aab"));
        assert_eq!(MatchResult::Ok(5), p.matches("aaaab"));

        let p = Pattern::new("a*bc").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(3), p.matches("abc"));
        assert_eq!(MatchResult::Ok(5), p.matches("abbbc"));
    }

    #[test]
    fn test_end_anchor() {
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

        let p = Pattern::new("a*.$b").unwrap();
        assert_eq!(
            MatchResult::Pending(4, PatternState { op_index: 4 }),
            p.matches("axyz")
        );
        assert_eq!(
            MatchResult::Ok(1),
            p.matches_with_state("b", &PatternState { op_index: 4 })
        );

        let p = Pattern::new("a*[b$]*cd").unwrap();
        assert_eq!(
            MatchResult::Pending(3, PatternState { op_index: 2 }),
            p.matches("abb")
        );
        assert_eq!(
            MatchResult::Pending(2, PatternState { op_index: 2 }),
            p.matches_with_state("bb", &PatternState { op_index: 2 })
        );
        assert_eq!(
            MatchResult::Ok(4),
            p.matches_with_state("bccd", &PatternState { op_index: 2 })
        );
    }

    #[test]
    fn test_complex_pattern() {
        let p = Pattern::new("*.").unwrap();
        assert_eq!(MatchResult::Ok(10), p.matches("things 890"));
        assert_eq!(MatchResult::Ok(1), p.matches("0"));
        assert_eq!(MatchResult::Ok(1), p.matches(" "));

        let p = Pattern::new("*[ab%d]c").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("c"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(2), p.matches("bc"));
        assert_eq!(MatchResult::Ok(3), p.matches("bac"));
        assert_eq!(MatchResult::Ok(5), p.matches("0b4ac"));
        assert_eq!(MatchResult::Ok(14), p.matches("a1b234ba9bbbbc"));

        let p = Pattern::new("%d*[%w_%.]@").unwrap();
        assert_eq!(MatchResult::Ok(6), p.matches("1x4_5@"));
        assert_eq!(MatchResult::Ok(15), p.matches("9xxasd_234.45f@"));

        let p = Pattern::new("ab*(^ba)ba").unwrap();
        assert_eq!(MatchResult::Ok(4), p.matches("abba"));
    }

    #[test]
    fn test_pattern_composition() {
        let p = Pattern::new("[a(^bc)d]").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches("a"));
        assert_eq!(MatchResult::Err, p.matches("b"));
        assert_eq!(MatchResult::Ok(2), p.matches("bx"));
        assert_eq!(MatchResult::Ok(2), p.matches("bxa"));
        assert_eq!(MatchResult::Ok(2), p.matches("bxd"));
        assert_eq!(MatchResult::Ok(1), p.matches("d"));

        let p = Pattern::new("*(a[^ab])").unwrap();
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(0), p.matches("a"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
        assert_eq!(MatchResult::Ok(2), p.matches("aca"));
        assert_eq!(MatchResult::Ok(2), p.matches("acab"));

        let p = Pattern::new("*[(^ab)c]").unwrap();
        assert_eq!(MatchResult::Ok(0), p.matches(""));
        assert_eq!(MatchResult::Ok(1), p.matches("c"));
        assert_eq!(MatchResult::Ok(0), p.matches("ab"));
        assert_eq!(MatchResult::Ok(2), p.matches("ac"));
    }

    #[test]
    fn test_bad_pattern() {
        assert!(matches!(Pattern::new("("), None));
        assert!(matches!(Pattern::new(")"), None));
        assert!(matches!(Pattern::new("["), None));
        assert!(matches!(Pattern::new("]"), None));
        assert!(matches!(Pattern::new("*"), None));
        assert!(matches!(Pattern::new("%"), None));
        assert!(matches!(Pattern::new("%!"), None));
    }
}
