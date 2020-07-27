#[derive(Clone, Copy)]
enum MatchResult {
    Pending(PatternState),
    Ok(usize),
    Err,
}

#[derive(Clone, Copy)]
pub struct PatternState {
    next_op: usize,
}

pub struct Pattern {
    ops: Vec<Op>,
    state: PatternState,
}

impl Pattern {
    pub fn build(pattern: &str) -> Option<Self> {
        Some(Self {
            ops: parse_ops(pattern.as_bytes())?,
            state: PatternState { next_op: 0 },
        })
    }

    pub fn matches(&mut self, bytes: &[u8]) -> MatchResult {
        MatchResult::Err
    }
}

enum Op {
    Match,
    Error,
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
        while let Some(b) = self.next() {
            self.parse_expr()?;
        }
        self.ops.push(Op::Match);

        Some(self.ops)
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

    fn parse_expr(&mut self) -> Option<()> {
        match self.bytes[self.index] {
            b'*' => None,
            b'[' => self.parse_custom_class(),
            _ => self.parse_class(),
        }
    }

    fn parse_class(&mut self) -> Option<()> {
        let okj = self.ops.len() + 1;
        if okj > u8::max_value() as _ {
            return None;
        }
        let okj = okj as _;

        match self.bytes[self.index] {
            b'%' => match self.next()? {
                b'a' => self.ops.push(Op::Alphabetic(okj, 0)),
                b'l' => self.ops.push(Op::Lower(okj, 0)),
                b'u' => self.ops.push(Op::Upper(okj, 0)),
                b'd' => self.ops.push(Op::Digit(okj, 0)),
                b'w' => self.ops.push(Op::Alphanumeric(okj, 0)),
                b'%' => self.ops.push(Op::Byte(b'%', okj, 0)),
                b'[' => self.ops.push(Op::Byte(b'[', okj, 0)),
                b']' => self.ops.push(Op::Byte(b']', okj, 0)),
                b'*' => self.ops.push(Op::Byte(b'*', okj, 0)),
                _ => return None,
            },
            b'[' => return None,
            b']' => return None,
            b'*' => return None,
            b => self.ops.push(Op::Byte(b, okj, 0)),
        }

        Some(())
    }

    fn parse_custom_class(&mut self) -> Option<()> {
        let start_op_index = self.ops.len();
        while let Some(b) = self.next() {
            match b {
                b'[' => return None,
                b']' => break,
                _ => self.parse_expr()?,
            }
        }
        if self.bytes[self.index] != b']' {
            return None;
        }

        let end_op_index = self.ops.len();
        let okj = end_op_index as _;
        let mut erj = start_op_index as _;
        for op in &mut self.ops[start_op_index..(end_op_index - 1)] {
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
            erj += 1;
        }

        Some(())
    }
}
