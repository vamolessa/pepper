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
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..self.len as _]) }
    }
}

#[derive(Clone, Copy)]
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
    AllFinished,
    Unfinished(usize, usize),
}

pub struct Token {
    pub kind: TokenKind,
    pub range: Range<usize>,
}

pub struct Syntax {
    scanners: Vec<Scanner>,
}

impl Syntax {
    pub fn new() -> Self {
        Self {
            scanners: Vec::new(),
        }
    }

    pub fn parse_line(
        line: &str,
        previous_line_kind: LineKind,
        tokens: &mut Vec<TokenKind>,
    ) -> LineKind {
        LineKind::AllFinished
    }
}

#[derive(Clone, Copy)]
enum ScannerResult {
    Pending,
    Ok(TokenKind),
    Err,
}

struct Scanner {
    state: usize,
    arg0: SmallString,
    arg1: SmallString,
    result: ScannerResult,
    body: fn(&mut usize, &str, &str, char) -> ScannerResult,
}

impl Scanner {
    pub fn new(
        arg0: SmallString,
        arg1: SmallString,
        result: ScannerResult,
        body: fn(&mut usize, &str, &str, char) -> ScannerResult,
    ) -> Self {
        Self {
            state: 0,
            arg0,
            arg1,
            result,
            body,
        }
    }

    pub fn state(&mut self) -> &mut usize {
        &mut self.state
    }
    
    pub fn result(&self) -> ScannerResult {
        self.result
    }

    pub fn scan(&mut self, ch: char) {
        self.result = (self.body)(&mut self.state, self.arg0.as_str(), self.arg1.as_str(), ch);
    }
}

fn scan_line_comment(state: &mut usize, arg0: &str, arg1: &str, ch: char) -> ScannerResult {
    ScannerResult::Err
}
