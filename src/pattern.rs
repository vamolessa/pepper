#[derive(Clone, Copy)]
enum MatchResult {
    Pending(PatternState),
    Ok(usize),
    Err,
}

struct Flow {
    ok: u8,
    err: u8,
}

enum Op {
    Match,
    Error,
    Digit(Flow),
    Alphabetic(Flow),
    Byte(u8, Flow),
}

#[derive(Clone, Copy)]
pub struct PatternState {
    next_op: usize,
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

fn parse_ops(mut bytes: Bytes) -> Result<Vec<Op>, ()> {
    let mut ops = Vec::new();
    parse_expr(&mut bytes, &mut ops);
    Err(())
}

fn parse_expr(bytes: &mut Bytes, ops: &mut Vec<Op>) -> Result<(), ()> {
    while let Some(b) = bytes.next() {
        let flow = Flow {ok: 0, err: 0};
        match b {
            b'*' => (),
            b'\\' => match bytes.next() {
                Some(b'd') => ops.push(Op::Digit(flow)),
                Some(b'w') => ops.push(Op::Alphabetic(flow)),
                Some(b'[') => ops.push(Op::Byte(b'[', flow)),
                Some(b']') => ops.push(Op::Byte(b']', flow)),
                Some(b'{') => ops.push(Op::Byte(b'{', flow)),
                Some(b'}') => ops.push(Op::Byte(b'}', flow)),
                _ => return Err(()),
            },
            _ => ops.push(Op::Byte(b, flow)),
        }
    }

    Ok(())
}
