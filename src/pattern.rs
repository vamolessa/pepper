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

        loop {
            match ops[op_index] {
                Op::Ok => return MatchResult::Ok(len),
                Op::Error => return MatchResult::Err,
                Op::Jump(jump) => op_index = jump as _,
                Op::Match(okj, erj, ref class) => {
                    let ok = match class {
                        CharClass::EndAnchor => false,
                        CharClass::Any => true,
                        CharClass::Alphabetic => byte.is_ascii_alphabetic(),
                        CharClass::Lower => byte.is_ascii_lowercase(),
                        CharClass::Upper => byte.is_ascii_uppercase(),
                        CharClass::Digit => byte.is_ascii_digit(),
                        CharClass::Alphanumeric => byte.is_ascii_alphanumeric(),
                        CharClass::Byte(b) => byte == *b,
                    };

                    if ok {
                        op_index = okj as _;
                    } else {
                        op_index = erj as _;
                        continue;
                    }
                }
                Op::Unwind(len, jump) => {
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
                Op::Jump(jump) => op_index = jump as _,
                Op::Match(okj, _, CharClass::EndAnchor) => {
                    op_index = okj as _;
                    match ops[op_index] {
                        Op::Ok => return MatchResult::Ok(len),
                        _ => return MatchResult::Pending(len, PatternState { op_index }),
                    }
                }
                Op::Match(_, erj, _) => op_index = erj as _,
                Op::Unwind(_, jump) => op_index = jump as _,
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
enum CharClass {
    EndAnchor,
    Any,
    Alphabetic,
    Lower,
    Upper,
    Digit,
    Alphanumeric,
    Byte(u8),
}

#[derive(Debug)]
enum Op {
    Ok,
    Error,
    Jump(u8),
    Match(u8, u8, CharClass),
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
        self.ops.push(Op::Error);
        let mut previous_len = 0;
        while let Some(_) = self.next() {
            previous_len = self.parse_expr(previous_len, 0, 0)?;
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

    fn parse_expr(&mut self, previous_len: usize, okj: u8, erj: u8) -> Option<usize> {
        let len = match self.current() {
            b'*' => self.parse_repeat(previous_len, okj, erj)?,
            b'(' => self.parse_sequence(okj, erj)?,
            b'[' => self.parse_group(okj, erj)?,
            _ => self.parse_class(okj, erj)?,
        };

        Some(len)
    }

    fn parse_class(&mut self, okj: u8, erj: u8) -> Option<usize> {
        let okj = self.ops.len() + 1;
        if okj > u8::max_value() as _ {
            return None;
        }
        let okj = okj as _;

        let char_class = match self.current() {
            b'%' => match self.next()? {
                b'a' => CharClass::Alphabetic,
                b'l' => CharClass::Lower,
                b'u' => CharClass::Upper,
                b'd' => CharClass::Digit,
                b'w' => CharClass::Alphanumeric,
                b'%' => CharClass::Byte(b'%'),
                b'$' => CharClass::Byte(b'$'),
                b'.' => CharClass::Byte(b'.'),
                b'^' => CharClass::Byte(b'^'),
                b'(' => CharClass::Byte(b'('),
                b')' => CharClass::Byte(b')'),
                b'[' => CharClass::Byte(b'['),
                b']' => CharClass::Byte(b']'),
                b'*' => CharClass::Byte(b'*'),
                _ => return None,
            },
            b'$' => CharClass::EndAnchor,
            b'.' => CharClass::Any,
            b'(' => return None,
            b')' => return None,
            b'[' => return None,
            b']' => return None,
            b'*' => return None,
            b => CharClass::Byte(b),
        };
        self.ops.push(Op::Match(okj, 0, char_class));

        Some(1)
    }

    fn parse_sequence(&mut self, okj: u8, erj: u8) -> Option<usize> {
        let inverse = self.current() == b'^';
        if inverse {
            self.next();
        }

        let mut op_indices = Vec::new();
        let mut previous_len = 0;
        while let Some(b) = self.next() {
            match b {
                b')' => break,
                _ => {
                    previous_len = self.parse_expr(previous_len, okj, erj)?;
                    op_indices.push(self.ops.len() - 1);
                }
            }
        }
        if self.current() != b')' {
            return None;
        }

        let op_count = self.ops.len() as _;
        let first_index = op_indices.first().cloned().unwrap_or(0);
        let last_index = op_indices.last().cloned().unwrap_or(0);

        for index in &op_indices {
            let index = *index;
            if let Op::Match(okj, erj, _) = &mut self.ops[index] {
                if inverse {
                    if index == last_index {
                        *okj = 0;
                    }

                    *erj = op_count;
                } else {
                }
            }
        }

        None
    }

    fn parse_group(&mut self, okj: u8, erj: u8) -> Option<usize> {
        let start_op_index = self.ops.len();
        let mut previous_len = 0;
        let mut len = 0;
        while let Some(b) = self.next() {
            match b {
                b'[' => return None,
                b']' => break,
                _ => {
                    previous_len = self.parse_expr(previous_len, okj, erj)?;
                    len += previous_len;
                }
            }
        }
        if self.current() != b']' {
            return None;
        }

        let end_op_index = self.ops.len();
        let okj = end_op_index as _;
        let mut erj = start_op_index as _;
        for op in &mut self.ops[start_op_index..(end_op_index - 1)] {
            erj += 1;
            if let Op::Match(ref mut o, ref mut e, _) = op {
                *o = okj;
                *e = erj;
            } else {
                unreachable!();
            }
        }

        Some(len)
    }

    fn parse_repeat(&mut self, previous_len: usize, okj: u8, erj: u8) -> Option<usize> {
        if previous_len == 0 {
            return None;
        }

        let len = self.ops.len();
        let previous_start_op_index = len - previous_len;
        let okj = previous_start_op_index as _;

        let mut i = previous_start_op_index;
        for op in &mut self.ops[previous_start_op_index..] {
            i += 1;
            if let Op::Match(ref mut o, ref mut e, _) = op {
                *o = okj;
                if i == len {
                    *e = len as _;
                }
            } else {
                unreachable!();
            }
        }

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
