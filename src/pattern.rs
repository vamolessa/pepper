#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchResult {
    Pending(PatternState),
    Ok(usize),
    Err,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternState {
    op_index: usize,
}

#[derive(Debug)]
pub struct Pattern {
    ops: Vec<Op>,
}

impl Pattern {
    pub fn new(pattern: &str) -> Option<Self> {
        Some(Self {
            ops: parse_ops(pattern.as_bytes())?,
        })
    }

    pub fn matches(&self, bytes: &[u8]) -> MatchResult {
        if bytes.is_empty() {
            return MatchResult::Err;
        }

        let mut len = 0;
        let ops = &self.ops[..];
        let mut op_index = 1;
        let mut bytes_index = 0;
        let mut byte = bytes[bytes_index];

        macro_rules! jump {
            ($e:expr, $ok_jump:expr, $err_jump:expr) => {
                if $e {
                    op_index = $ok_jump as _;
                } else {
                    op_index = $err_jump as _;
                    continue;
                }
            };
        }

        loop {
            match ops[op_index] {
                Op::Match => return MatchResult::Ok(len),
                Op::Error => return MatchResult::Err,
                Op::Any(okj, erj) => jump!(true, okj, erj),
                Op::Alphabetic(okj, erj) => jump!(byte.is_ascii_alphabetic(), okj, erj),
                Op::Lower(okj, erj) => jump!(byte.is_ascii_lowercase(), okj, erj),
                Op::Upper(okj, erj) => jump!(byte.is_ascii_uppercase(), okj, erj),
                Op::Digit(okj, erj) => jump!(byte.is_ascii_digit(), okj, erj),
                Op::Alphanumeric(okj, erj) => jump!(byte.is_ascii_alphanumeric(), okj, erj),
                Op::Byte(b, okj, erj) => jump!(byte == b, okj, erj),
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
                Op::Match => return MatchResult::Ok(len),
                Op::Error => return MatchResult::Err,
                Op::Any(_, erj)
                | Op::Alphabetic(_, erj)
                | Op::Lower(_, erj)
                | Op::Upper(_, erj)
                | Op::Digit(_, erj)
                | Op::Alphanumeric(_, erj) 
                | Op::Byte(_, _, erj) => {
                    op_index = erj as _;
                }
            };
        }
    }
}

#[derive(Debug)]
enum Op {
    Match,
    Error,
    Any(u8, u8),
    Alphabetic(u8, u8),
    Lower(u8, u8),
    Upper(u8, u8),
    Digit(u8, u8),
    Alphanumeric(u8, u8),
    Byte(u8, u8, u8),
}

fn parse_ops(bytes: &[u8]) -> Option<Vec<Op>> {
    let mut parser = OpParser::new(bytes);
    parser.parse()
}

struct OpParser<'a> {
    pub bytes: &'a [u8],
    pub index: usize,
    pub ops: Vec<Op>,
}

impl<'a> OpParser<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            index: 0,
            ops: Vec::new(),
        }
    }

    pub fn parse(mut self) -> Option<Vec<Op>> {
        self.ops.push(Op::Error);
        let mut previous_len = 0;
        while let Some(b) = self.next() {
            previous_len = self.parse_expr(previous_len)?;
        }
        self.ops.push(Op::Match);

        Some(self.ops)
    }

    fn current(&self) -> u8 {
        self.bytes[self.index - 1]
    }

    fn next(&mut self) -> Option<u8> {
        if self.index < self.bytes.len() {
            let b = self.bytes[self.index];
            self.index += 1;
            Some(b)
        } else {
            None
        }
    }

    fn parse_expr(&mut self, previous_len: usize) -> Option<usize> {
        let start_len = self.ops.len();
        match self.current() {
            b'*' => self.parse_repeat(previous_len)?,
            b'[' => self.parse_custom_class()?,
            _ => self.parse_class()?,
        }

        Some(self.ops.len() - start_len)
    }

    fn parse_class(&mut self) -> Option<()> {
        let okj = self.ops.len() + 1;
        if okj > u8::max_value() as _ {
            return None;
        }
        let okj = okj as _;

        match self.current() {
            b'%' => match self.next()? {
                b'a' => self.ops.push(Op::Alphabetic(okj, 0)),
                b'l' => self.ops.push(Op::Lower(okj, 0)),
                b'u' => self.ops.push(Op::Upper(okj, 0)),
                b'd' => self.ops.push(Op::Digit(okj, 0)),
                b'w' => self.ops.push(Op::Alphanumeric(okj, 0)),
                b'%' => self.ops.push(Op::Byte(b'%', okj, 0)),
                b'.' => self.ops.push(Op::Byte(b'.', okj, 0)),
                b'[' => self.ops.push(Op::Byte(b'[', okj, 0)),
                b']' => self.ops.push(Op::Byte(b']', okj, 0)),
                b'*' => self.ops.push(Op::Byte(b'*', okj, 0)),
                _ => return None,
            },
            b'.' => self.ops.push(Op::Any(okj, 0)),
            b'[' => return None,
            b']' => return None,
            b'*' => return None,
            b => self.ops.push(Op::Byte(b, okj, 0)),
        }

        Some(())
    }

    fn parse_custom_class(&mut self) -> Option<()> {
        let start_op_index = self.ops.len();
        let mut previous_len = 0;
        while let Some(b) = self.next() {
            match b {
                b'[' => return None,
                b']' => break,
                _ => previous_len = self.parse_expr(previous_len)?,
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
            match op {
                Op::Alphabetic(ref mut o, ref mut e)
                | Op::Lower(ref mut o, ref mut e)
                | Op::Upper(ref mut o, ref mut e)
                | Op::Digit(ref mut o, ref mut e)
                | Op::Alphanumeric(ref mut o, ref mut e)
                | Op::Byte(_, ref mut o, ref mut e) => {
                    *o = okj;
                    *e = erj;
                }
                _ => unreachable!(),
            }
        }

        Some(())
    }

    fn parse_repeat(&mut self, previous_len: usize) -> Option<()> {
        if previous_len == 0 {
            return None;
        }

        let len = self.ops.len();
        let previous_start_op_index = len - previous_len;
        let okj = previous_start_op_index as _;

        let mut i = previous_start_op_index;
        for op in &mut self.ops[previous_start_op_index..] {
            i += 1;
            match op {
                Op::Any(ref mut o, ref mut e)
                | Op::Alphabetic(ref mut o, ref mut e)
                | Op::Lower(ref mut o, ref mut e)
                | Op::Upper(ref mut o, ref mut e)
                | Op::Digit(ref mut o, ref mut e)
                | Op::Alphanumeric(ref mut o, ref mut e)
                | Op::Byte(_, ref mut o, ref mut e) => {
                    *o = okj;
                    if i == len {
                        *e = len as _;
                    }
                }
                _ => unreachable!(),
            }
        }

        Some(())
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

        let p = Pattern::new("%% %[ %] %* %.").unwrap();
        assert_eq!(MatchResult::Ok(9), p.matches(b"% [ ] * ."));

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
