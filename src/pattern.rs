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
    pub fn build(pattern: &str) -> Result<Self, ()> {
        Ok(Self {
            ops: parse_ops(Bytes::from_slice(pattern.as_bytes()))?,
            state: PatternState { next_op: 0 },
        })
    }

    pub fn matches(&mut self, bytes: &[u8]) -> MatchResult {
        MatchResult::Err
    }
}

struct Bytes<'a> {
    slice: &'a [u8],
    index: usize,
}

impl<'a> Bytes<'a> {
    pub fn from_slice(slice: &'a [u8]) -> Self {
        Self { slice, index: 0 }
    }

    pub fn next(&mut self) -> Option<u8> {
        if self.index < self.slice.len() {
            let next = self.slice[self.index];
            self.index += 1;
            Some(next)
        } else {
            None
        }
    }

    pub fn put_back(&mut self) {
        self.index -= 1;
    }
}

enum Op {
    Match,
    Error,
    Jump(u8),
    Digit(u8, u8),
    Alphabetic(u8, u8),
    Byte(u8, u8, u8),
}

fn parse_ops(mut bytes: Bytes) -> Result<Vec<Op>, ()> {
    let mut ops = Vec::new();
    parse_expr(&mut bytes, &mut ops);
    Err(())
}

fn parse_expr(bytes: &mut Bytes, ops: &mut Vec<Op>) -> Result<(), ()> {
    while let Some(b) = bytes.next() {
        match b {
            b'*' => (),
            b'\\' => match bytes.next() {
                Some(b'd') => ops.push(Op::Digit(0, 0)),
                Some(b'w') => ops.push(Op::Alphabetic(0, 0)),
                Some(b'[') => ops.push(Op::Byte(b'[', 0, 0)),
                Some(b']') => ops.push(Op::Byte(b']', 0, 0)),
                Some(b'{') => ops.push(Op::Byte(b'{', 0, 0)),
                Some(b'}') => ops.push(Op::Byte(b'}', 0, 0)),
                _ => return Err(()),
            },
            _ => ops.push(Op::Byte(b, 0, 0)),
        }
    }

    Ok(())
}
