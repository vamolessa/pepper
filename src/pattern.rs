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

    pub fn matches(&self, bytes: &[u8]) -> MatchResult {
        self.matches_with_state(bytes, &PatternState { op_index: 1 })
    }

    pub fn matches_with_state(&self, bytes: &[u8], state: &PatternState) -> MatchResult {
        if bytes.is_empty() {
            return MatchResult::Err;
        }

        let mut len = 0;
        let ops = &self.ops[..];
        let mut op_index = state.op_index;
        let mut bytes_index = 0;
        let mut byte = bytes[bytes_index];

        macro_rules! check {
            ($e:expr, $okj:expr, $erj:expr) => {
                if $e {
                    op_index = $okj.0 as _;
                } else {
                    op_index = $erj.0 as _;
                    continue;
                }
            };
        };

        loop {
            match ops[op_index] {
                Op::Ok => return MatchResult::Ok(len),
                Op::Error => return MatchResult::Err,
                Op::EndAnchor(okj, erj) => check!(false, okj, erj),
                Op::Any(okj, erj) => check!(true, okj, erj),
                Op::Alphabetic(okj, erj) => check!(byte.is_ascii_alphabetic(), okj, erj),
                Op::Lower(okj, erj) => check!(byte.is_ascii_lowercase(), okj, erj),
                Op::Upper(okj, erj) => check!(byte.is_ascii_uppercase(), okj, erj),
                Op::Digit(okj, erj) => check!(byte.is_ascii_digit(), okj, erj),
                Op::Alphanumeric(okj, erj) => check!(byte.is_ascii_alphanumeric(), okj, erj),
                Op::Byte(okj, erj, b) => check!(byte == b, okj, erj),
                Op::Unwind(jump, len) => {
                    bytes_index -= len.0 as usize;
                    op_index = jump.0 as _;
                    continue;
                }
            };

            len += 1;
            bytes_index += 1;
            if bytes_index == bytes.len() {
                break;
            }

            byte = bytes[bytes_index];
        }

        loop {
            match ops[op_index] {
                Op::Ok => return MatchResult::Ok(len),
                Op::Error => return MatchResult::Err,
                Op::EndAnchor(okj, _) => {
                    op_index = okj.0 as _;
                    match ops[op_index] {
                        Op::Ok => return MatchResult::Ok(len),
                        _ => return MatchResult::Pending(len, PatternState { op_index }),
                    }
                }
                Op::Any(_, erj)
                | Op::Alphabetic(_, erj)
                | Op::Lower(_, erj)
                | Op::Upper(_, erj)
                | Op::Digit(_, erj)
                | Op::Alphanumeric(_, erj)
                | Op::Byte(_, erj, _) => op_index = erj.0 as _,
                Op::Unwind(jump, _) => op_index = jump.0 as _,
            };
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

    fn peek(&self) -> u8 {
        self.bytes[self.index]
    }

    fn current(&self) -> u8 {
        self.bytes[self.index - 1]
    }

    fn next(&mut self) -> Option<u8> {
        if self.index < self.bytes.len() {
            let b = self.peek();
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
            b'*' => self.parse_repeat(okj, erj)?,
            b'(' => self.parse_sequence(okj, erj)?,
            b'[' => self.parse_group(okj, erj)?,
            _ => self.parse_class(okj, erj)?,
        };

        Some(len)
    }

    fn parse_repeat(&mut self, okj: JumpFrom, erj: JumpFrom) -> Option<Length> {
        let start_index = self.ops.len();
        self.parse_expr(okj, erj)?;
        self.ops.push(Op::Unwind(start_index.into(), Length(0)));
        Some(Length(0))
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

    fn patch_jump(&mut self, jump: JumpFrom, abs_jump: Jump) {
        if let JumpFrom::End(mut jump) = jump {
            let op_count = self.ops.len().into();
            if let Op::Unwind(j, _) = &mut self.ops[abs_jump.0 as usize] {
                jump += op_count;
                *j = jump;
            } else {
                unreachable!();
            }
        }
    }

    fn parse_sequence(&mut self, okj: JumpFrom, erj: JumpFrom) -> Option<Length> {
        let inverse = self.peek() == b'^';
        let mut len = Length(0);

        if inverse {
            self.next();

            let abs_okj = self.get_absolute_jump(okj);
            while self.next_is_not(b')') {
                len += self.parse_expr(JumpFrom::End(Jump(0)), JumpFrom::Beginning(abs_okj))?;
            }
            match erj {
                JumpFrom::Beginning(jump) => self.ops.push(Op::Unwind(jump, len)),
                JumpFrom::End(mut jump) => {
                    jump += (self.ops.len() + 1).into();
                    self.ops.push(Op::Unwind(jump, len));
                }
            }
            self.patch_jump(okj, abs_okj);
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
        let inverse = self.peek() == b'^';
        let mut len = Length(0);

        if inverse {
            self.next();

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
        let p = Pattern::new("a").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"a"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"aa"));
        assert_eq!(MatchResult::Err, p.matches(b"b"));
        assert_eq!(MatchResult::Err, p.matches(b""));

        let p = Pattern::new("aa").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches(b"aa"));
        assert_eq!(MatchResult::Ok(2), p.matches(b"aaa"));
        assert_eq!(MatchResult::Err, p.matches(b"baa"));

        let p = Pattern::new("%% %$ %. %^ %( %) %[ %] %*").unwrap();
        assert_eq!(MatchResult::Ok(17), p.matches(b"% $ . ^ ( ) [ ] *"));

        let p = Pattern::new(".").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"a"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"z"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"A"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"Z"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"0"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"9"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"!"));

        let p = Pattern::new("%a").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"a"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"z"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"A"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"Z"));
        assert_eq!(MatchResult::Err, p.matches(b"0"));
        assert_eq!(MatchResult::Err, p.matches(b"9"));
        assert_eq!(MatchResult::Err, p.matches(b"!"));

        let p = Pattern::new("%l").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"a"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"z"));
        assert_eq!(MatchResult::Err, p.matches(b"A"));
        assert_eq!(MatchResult::Err, p.matches(b"Z"));
        assert_eq!(MatchResult::Err, p.matches(b"0"));
        assert_eq!(MatchResult::Err, p.matches(b"9"));
        assert_eq!(MatchResult::Err, p.matches(b"!"));

        let p = Pattern::new("%u").unwrap();
        assert_eq!(MatchResult::Err, p.matches(b"a"));
        assert_eq!(MatchResult::Err, p.matches(b"z"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"A"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"Z"));
        assert_eq!(MatchResult::Err, p.matches(b"0"));
        assert_eq!(MatchResult::Err, p.matches(b"9"));
        assert_eq!(MatchResult::Err, p.matches(b"!"));

        let p = Pattern::new("%d").unwrap();
        assert_eq!(MatchResult::Err, p.matches(b"a"));
        assert_eq!(MatchResult::Err, p.matches(b"z"));
        assert_eq!(MatchResult::Err, p.matches(b"A"));
        assert_eq!(MatchResult::Err, p.matches(b"Z"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"0"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"9"));
        assert_eq!(MatchResult::Err, p.matches(b"!"));

        let p = Pattern::new("%w").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"a"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"z"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"A"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"Z"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"0"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"9"));
        assert_eq!(MatchResult::Err, p.matches(b"!"));
    }

    #[test]
    fn test_group() {
        let p = Pattern::new("[abc]").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"a"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"b"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"c"));
        assert_eq!(MatchResult::Err, p.matches(b"d"));

        let p = Pattern::new("z[abc]y").unwrap();
        assert_eq!(MatchResult::Ok(3), p.matches(b"zay"));
        assert_eq!(MatchResult::Ok(3), p.matches(b"zby"));
        assert_eq!(MatchResult::Ok(3), p.matches(b"zcy"));
        assert_eq!(MatchResult::Err, p.matches(b"z"));
        assert_eq!(MatchResult::Err, p.matches(b"zy"));
        assert_eq!(MatchResult::Err, p.matches(b"zdy"));

        let p = Pattern::new("z[a]").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches(b"za"));
        assert_eq!(MatchResult::Err, p.matches(b"z"));
        assert_eq!(MatchResult::Err, p.matches(b"zb"));

        let p = Pattern::new("z[%l%d]").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches(b"za"));
        assert_eq!(MatchResult::Ok(2), p.matches(b"zz"));
        assert_eq!(MatchResult::Ok(2), p.matches(b"z0"));
        assert_eq!(MatchResult::Ok(2), p.matches(b"z9"));
        assert_eq!(MatchResult::Err, p.matches(b"z"));
        assert_eq!(MatchResult::Err, p.matches(b"zA"));
        assert_eq!(MatchResult::Err, p.matches(b"zZ"));

        let p = Pattern::new("[^abc]").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"d"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"3"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"@"));
        assert_eq!(MatchResult::Err, p.matches(b"a"));
        assert_eq!(MatchResult::Err, p.matches(b"b"));
        assert_eq!(MatchResult::Err, p.matches(b"c"));
    }
    
    #[test]
    fn test_sequence() {
        let p = Pattern::new("(abc)").unwrap();
        assert_eq!(MatchResult::Ok(3), p.matches(b"abc"));
        assert_eq!(MatchResult::Ok(3), p.matches(b"abcd"));
        assert_eq!(MatchResult::Err, p.matches(b"a"));
        assert_eq!(MatchResult::Err, p.matches(b"ab"));

        let p = Pattern::new("z(abc)y").unwrap();
        assert_eq!(MatchResult::Ok(5), p.matches(b"zabcy"));
        assert_eq!(MatchResult::Ok(5), p.matches(b"zabcyd"));
        assert_eq!(MatchResult::Err, p.matches(b"zay"));
        assert_eq!(MatchResult::Err, p.matches(b"zaby"));

        let p = Pattern::new("z(%u%w)y").unwrap();
        assert_eq!(MatchResult::Ok(4), p.matches(b"zA0y"));
        assert_eq!(MatchResult::Ok(4), p.matches(b"zZay"));
        assert_eq!(MatchResult::Ok(4), p.matches(b"zA0yA"));
        assert_eq!(MatchResult::Err, p.matches(b"zaay"));
        assert_eq!(MatchResult::Err, p.matches(b"z8ay"));
    }

    //#[test]
    fn test_repeat() {
        let p = Pattern::new("a*").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"a"));
        assert_eq!(MatchResult::Ok(4), p.matches(b"aaaa"));

        let p = Pattern::new("a*b").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches(b"ab"));
        assert_eq!(MatchResult::Ok(3), p.matches(b"aab"));
        assert_eq!(MatchResult::Ok(5), p.matches(b"aaaab"));

        let p = Pattern::new("ab*c").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches(b"ac"));
        assert_eq!(MatchResult::Ok(3), p.matches(b"abc"));
        assert_eq!(MatchResult::Ok(5), p.matches(b"abbbc"));
    }

    //#[test]
    fn test_end_anchor() {
        let p = Pattern::new("a$").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"a"));
        assert_eq!(MatchResult::Err, p.matches(b"aa"));

        let p = Pattern::new("a$b").unwrap();
        assert_eq!(
            MatchResult::Pending(1, PatternState { op_index: 3 }),
            p.matches(b"a")
        );
        assert_eq!(
            MatchResult::Ok(1),
            p.matches_with_state(b"b", &PatternState { op_index: 3 })
        );

        let p = Pattern::new("a.*$b").unwrap();
        assert_eq!(
            MatchResult::Pending(4, PatternState { op_index: 4 }),
            p.matches(b"axyz")
        );
        assert_eq!(
            MatchResult::Ok(1),
            p.matches_with_state(b"b", &PatternState { op_index: 4 })
        );

        let p = Pattern::new("a[b$]*c*d").unwrap();
        assert_eq!(
            MatchResult::Pending(3, PatternState { op_index: 2 }),
            p.matches(b"abb")
        );
        assert_eq!(
            MatchResult::Pending(2, PatternState { op_index: 2 }),
            p.matches_with_state(b"bb", &PatternState { op_index: 2 })
        );
        assert_eq!(
            MatchResult::Ok(4),
            p.matches_with_state(b"bccd", &PatternState { op_index: 2 })
        );
    }

    //#[test]
    fn test_complex_pattern() {
        let p = Pattern::new(".*").unwrap();
        assert_eq!(MatchResult::Ok(10), p.matches(b"things 890"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"0"));
        assert_eq!(MatchResult::Ok(1), p.matches(b" "));

        let p = Pattern::new("[ab%d]*c").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"c"));
        assert_eq!(MatchResult::Ok(2), p.matches(b"ac"));
        assert_eq!(MatchResult::Ok(2), p.matches(b"bc"));
        assert_eq!(MatchResult::Ok(3), p.matches(b"bac"));
        assert_eq!(MatchResult::Ok(5), p.matches(b"0b4ac"));
        assert_eq!(MatchResult::Ok(14), p.matches(b"a1b234ba9bbbbc"));

        let p = Pattern::new("%d[%w_%.]*@").unwrap();
        assert_eq!(MatchResult::Ok(6), p.matches(b"1x4_5@"));
        assert_eq!(MatchResult::Ok(15), p.matches(b"9xxasd_234.45f@"));
    }

    //#[test]
    fn test_bad_pattern() {
        assert!(matches!(Pattern::new("("), None));
        assert!(matches!(Pattern::new(")"), None));
        assert!(matches!(Pattern::new("["), None));
        assert!(matches!(Pattern::new("]"), None));
        assert!(matches!(Pattern::new("*"), None));
        assert!(matches!(Pattern::new(""), None));
        assert!(matches!(Pattern::new("%"), None));
        assert!(matches!(Pattern::new("%h"), None));
    }
}
