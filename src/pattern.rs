use std::fmt;

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
        self.matches_with_state(bytes, &PatternState { op_index: 2 })
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
                    op_index = $okj as _;
                } else {
                    op_index = $erj as _;
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
                    bytes_index -= len as usize;
                    op_index = jump as _;
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
                    op_index = okj as _;
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
                | Op::Byte(_, erj, _) => op_index = erj as _,
                Op::Unwind(jump, _) => op_index = jump as _,
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

#[derive(Debug)]
enum Op {
    Ok,
    Error,
    EndAnchor(u8, u8),
    Any(u8, u8),
    Alphabetic(u8, u8),
    Lower(u8, u8),
    Upper(u8, u8),
    Digit(u8, u8),
    Alphanumeric(u8, u8),
    Byte(u8, u8, u8),
    Unwind(u8, u8),
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
        self.ops.push(Op::Ok);
        self.ops.push(Op::Error);
        while let Some(_) = self.next() {
            self.parse_expr(0, 1)?;
        }
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
            b == byte
        } else {
            true
        }
    }

    fn parse_expr(&mut self, okj: u8, erj: u8) -> Option<u8> {
        let len = match self.current() {
            b'*' => self.parse_repeat(okj, erj)?,
            b'(' => self.parse_sequence(okj, erj)?,
            b'[' => self.parse_group(okj, erj)?,
            _ => self.parse_class(okj, erj)?,
        };

        Some(len)
    }

    fn parse_class(&mut self, okj: u8, erj: u8) -> Option<u8> {
        let okj = self.ops.len() + 1;
        if okj > u8::max_value() as _ {
            return None;
        }
        let okj = okj as _;

        let op = match self.current() {
            b'%' => match self.next()? {
                b'a' => Op::Alphabetic(okj, erj),
                b'l' => Op::Lower(okj, erj),
                b'u' => Op::Upper(okj, erj),
                b'd' => Op::Digit(okj, erj),
                b'w' => Op::Alphanumeric(okj, erj),
                b'%' => Op::Byte(b'%', okj, erj),
                b'$' => Op::Byte(b'$', okj, erj),
                b'.' => Op::Byte(b'.', okj, erj),
                b'^' => Op::Byte(b'^', okj, erj),
                b'(' => Op::Byte(b'(', okj, erj),
                b')' => Op::Byte(b')', okj, erj),
                b'[' => Op::Byte(b'[', okj, erj),
                b']' => Op::Byte(b']', okj, erj),
                b'*' => Op::Byte(b'*', okj, erj),
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
        Some(1)
    }

    fn parse_sequence(&mut self, okj: u8, erj: u8) -> Option<u8> {
        let inverse = self.current() == b'^';
        let mut len = 0;

        if inverse {
            self.next();

            while self.next_is_not(b')') {
                let inner_okj = self.ops.len() as _;
                len += self.parse_expr(inner_okj, okj)?;
            }
            self.ops.push(Op::Unwind(erj, len));
        } else {
            while self.next_is_not(b')') {
                let inner_erj = self.ops.len() as _;
                let inner_okj = inner_erj + 1;
                len += self.parse_expr(inner_okj, inner_erj)?;
                self.ops.push(Op::Unwind(erj, len));
            }
            self.ops.push(Op::Unwind(okj, 0));
        }

        if self.current() == b')' {
            Some(len)
        } else {
            None
        }
    }

    fn parse_group(&mut self, okj: u8, erj: u8) -> Option<u8> {
        let inverse = self.current() == b'^';
        let mut len = 0;

        if inverse {
            self.next();

            while self.next_is_not(b']') {
                let inner_okj = self.ops.len() as _;
                let inner_erj = inner_okj + 1;
                let previous_len = self.parse_expr(inner_okj, inner_erj)?;
                self.ops.push(Op::Unwind(erj, previous_len));
                len += previous_len;
            }
            self.ops.push(Op::Any(self.ops.len() as _, erj));
        } else {
            while self.next_is_not(b']') {
                len += self.parse_expr(okj, erj)?;
            }
        }

        if self.current() == b']' {
            Some(len)
        } else {
            None
        }
    }

    fn parse_repeat(&mut self, okj: u8, erj: u8) -> Option<u8> {
        let start_index = self.ops.len();
        self.parse_expr(okj, erj)?;
        self.ops.push(Op::Unwind(start_index as _, 0));
        Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_simple_classes() {
        let p = Pattern::new("a").unwrap();
        assert_eq!(MatchResult::Ok(1), p.matches(b"a"));
        assert_eq!(MatchResult::Ok(1), p.matches(b"aa"));
        assert_eq!(MatchResult::Err, p.matches(b"b"));
        assert_eq!(MatchResult::Err, p.matches(b""));

        let p = Pattern::new("aa").unwrap();
        assert_eq!(MatchResult::Ok(2), p.matches(b"aa"));
        assert_eq!(MatchResult::Ok(2), p.matches(b"aaa"));
        assert_eq!(MatchResult::Err, p.matches(b"baa"));

        let p = Pattern::new("%% %[ %] %* %. %$").unwrap();
        assert_eq!(MatchResult::Ok(11), p.matches(b"% [ ] * . $"));

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
    fn test_match_custom_classes() {
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
    }

    #[test]
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

    #[test]
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

    #[test]
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

    #[test]
    fn test_bad_pattern() {
        assert!(matches!(Pattern::new("["), None));
        assert!(matches!(Pattern::new("]"), None));
        assert!(matches!(Pattern::new("*"), None));
        assert!(matches!(Pattern::new("%"), None));
        assert!(matches!(Pattern::new("%h"), None));
    }
}
