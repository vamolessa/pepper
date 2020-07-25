/*
   line_comments //
   block_comments /* */

   keywords if else while loop fn match let use mod
   modifiers pub

   symbols ( ) { } [ ] < > = ! + - * / | : ;

   strings " "
   chars ' '
   literals true false
*/

use std::{convert::TryFrom, ops::Range};

#[derive(Default)]
pub struct SmallString {
    len: u8,
    bytes: [u8; Self::capacity()],
}

impl SmallString {
    pub const fn capacity() -> usize {
        15
    }

    pub fn from_str(s: &str) -> Result<Self, ()> {
        let bytes = s.as_bytes();
        if bytes.len() <= Self::capacity() {
            Ok(Self {
                len: bytes.len() as _,
                bytes: <[u8; Self::capacity()]>::try_from(bytes).unwrap(),
            })
        } else {
            Err(())
        }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as _]).unwrap()
    }
}

pub enum TokenKind {
    LineComment,
    BlockComment,
    Keyword,
    Modifier,
    Symbol,
    String,
    Char,
    Literal,
    Number,
}

pub enum LineKind {
    Normal,
    UnfinishedString(usize),
    UnfinishedBlockComment(usize),
}

pub struct Token {
    pub kind: TokenKind,
    pub range: Range<usize>,
}

#[derive(Default)]
pub struct Syntax {
    pub line_comments: Vec<SmallString>,
    pub block_comments: Vec<(SmallString, SmallString)>,
    pub keywords: Vec<SmallString>,
    pub modifiers: Vec<SmallString>,
    pub symbols: Vec<SmallString>,
    pub strings: Vec<(SmallString, SmallString)>,
    pub chars: Vec<(SmallString, SmallString)>,
    pub literals: Vec<SmallString>,
}

impl Syntax {
    pub fn parse_line(
        line: &str,
        previous_line_kind: LineKind,
        tokens: &mut Vec<TokenKind>,
    ) -> LineKind {
        LineKind::Normal
    }
}
