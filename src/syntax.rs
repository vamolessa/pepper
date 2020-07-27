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

use std::ops::Range;

use crate::pattern::{Pattern, PatternState};

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
    Unfinished(usize, PatternState),
}

pub struct Token {
    pub kind: TokenKind,
    pub range: Range<usize>,
}

pub struct Syntax {
    patterns: Vec<Pattern>,
}

impl Syntax {
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
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
