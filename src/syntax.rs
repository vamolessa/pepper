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

use crate::pattern::{MatchResult, Pattern, PatternState};

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
    patterns: Vec<(TokenKind, Pattern)>,
}

impl Syntax {
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
        }
    }

    pub fn add_pattern(&mut self, kind: TokenKind, pattern: Pattern) {
        self.patterns.push((kind, pattern));
    }

    pub fn parse_line(
        &self,
        line: &str,
        previous_line_kind: LineKind,
        tokens: &mut Vec<Token>,
    ) -> LineKind {
        let mut line_index = 0;

        match previous_line_kind {
            LineKind::AllFinished => (),
            LineKind::Unfinished(pattern_index, state) => match self.patterns[pattern_index]
                .1
                .matches_from_state(line.as_bytes(), &state)
            {
                MatchResult::Ok(len) => {
                    tokens.push(Token {
                        kind: self.patterns[pattern_index].0,
                        range: 0..len,
                    });
                    line_index += len;
                }
                MatchResult::Err => (),
                MatchResult::Pending(_, state) => {
                    tokens.push(Token {
                        kind: self.patterns[pattern_index].0,
                        range: 0..line.len(),
                    });
                    return LineKind::Unfinished(pattern_index, state);
                }
            },
        }

        while line_index < line.len() {
            let line_slice = &line[line_index..].as_bytes();
            let mut best_pattern_index = 0;
            let mut max_len = 0;
            for (i, (kind, pattern)) in self.patterns.iter().enumerate() {
                match pattern.matches(line_slice) {
                    MatchResult::Ok(len) => {
                        if len > max_len {
                            max_len = len;
                            best_pattern_index = i;
                        }
                    }
                    MatchResult::Err => (),
                    MatchResult::Pending(_, state) => {
                        tokens.push(Token {
                            kind: *kind,
                            range: line_index..line.len(),
                        });
                        return LineKind::Unfinished(i, state);
                    }
                }
            }
            let from = line_index;
            line_index += line_index + max_len;
            tokens.push(Token {
                kind: self.patterns[best_pattern_index].0,
                range: from..line_index,
            });
        }

        LineKind::AllFinished
    }
}
