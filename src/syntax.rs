use std::{cmp::Ordering, iter, ops::Range};

use serde_derive::{Deserialize, Serialize};

use crate::{
    buffer::BufferContent,
    buffer_position::BufferRange,
    pattern::{MatchResult, Pattern, PatternState},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum TokenKind {
    Whitespace,
    Text,
    Comment,
    Keyword,
    Type,
    Symbol,
    String,
    Literal,
}

impl TokenKind {
    pub fn from_str(text: &str) -> Option<Self> {
        match text {
            "text" => Some(Self::Text),
            "comment" => Some(Self::Comment),
            "keyword" => Some(Self::Keyword),
            "type" => Some(Self::Type),
            "symbol" => Some(Self::Symbol),
            "string" => Some(Self::String),
            "literal" => Some(Self::Literal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    pub kind: TokenKind,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Finished,
    Unfinished(usize, PatternState),
}

impl Default for LineKind {
    fn default() -> Self {
        Self::Finished
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Syntax {
    extensions: Vec<String>,
    rules: Vec<(TokenKind, Pattern)>,
}

impl Syntax {
    pub fn new() -> Self {
        Self {
            extensions: Vec::new(),
            rules: Vec::new(),
        }
    }

    pub fn add_extension(&mut self, extension: String) {
        self.extensions.push(extension);
    }

    pub fn add_rule(&mut self, kind: TokenKind, pattern: Pattern) {
        self.rules.push((kind, pattern));
    }

    pub fn extensions(&self) -> impl Iterator<Item = &str> {
        self.extensions.iter().map(|e| &e[..])
    }

    pub fn rules(&self) -> impl Iterator<Item = (TokenKind, &Pattern)> {
        self.rules.iter().map(|(t, p)| (*t, p))
    }

    fn parse_line(
        &self,
        line: &str,
        previous_line_kind: LineKind,
        tokens: &mut Vec<Token>,
    ) -> LineKind {
        tokens.clear();

        if self.rules.len() == 0 {
            tokens.push(Token {
                kind: TokenKind::Text,
                range: 0..line.len(),
            });
            return LineKind::Finished;
        }

        let line_len = line.len();
        let mut line_index = 0;

        match previous_line_kind {
            LineKind::Finished => (),
            LineKind::Unfinished(pattern_index, state) => {
                match self.rules[pattern_index].1.matches_with_state(line, &state) {
                    MatchResult::Ok(len) => {
                        tokens.push(Token {
                            kind: self.rules[pattern_index].0,
                            range: 0..len,
                        });
                        line_index += len;
                    }
                    MatchResult::Err => (),
                    MatchResult::Pending(_, state) => {
                        tokens.push(Token {
                            kind: self.rules[pattern_index].0,
                            range: 0..line_len,
                        });
                        return LineKind::Unfinished(pattern_index, state);
                    }
                }
            }
        }

        while line_index < line_len {
            let line_slice = &line[line_index..];
            let whitespace_len = line_slice
                .bytes()
                .take_while(|b| b.is_ascii_whitespace())
                .count();
            let line_slice = &line_slice[whitespace_len..];

            let mut best_pattern_index = 0;
            let mut max_len = 0;
            for (i, (kind, pattern)) in self.rules.iter().enumerate() {
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
                            range: line_index..line_len,
                        });
                        return LineKind::Unfinished(i, state);
                    }
                }
            }

            let mut kind = self.rules[best_pattern_index].0;

            if max_len == 0 {
                kind = TokenKind::Text;
                max_len = line_slice
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric())
                    .count()
                    .max(1);
            }

            max_len += whitespace_len;

            let from = line_index;
            line_index = line_len.min(line_index + max_len);

            while !line.is_char_boundary(line_index) {
                line_index += 1;
            }

            tokens.push(Token {
                kind,
                range: from..line_index,
            });
        }

        LineKind::Finished
    }
}

#[derive(Clone, Copy)]
pub struct SyntaxHandle(usize);

#[derive(Debug, Default)]
pub struct SyntaxCollection {
    syntaxes: Vec<Syntax>,
}

impl SyntaxCollection {
    pub fn find_by_extension(&self, extension: &str) -> Option<SyntaxHandle> {
        for (i, syntax) in self.syntaxes.iter().enumerate() {
            for ext in &syntax.extensions {
                if extension == ext {
                    return Some(SyntaxHandle(i));
                }
            }
        }

        None
    }

    pub fn get_by_extension(&mut self, extension: &str) -> &mut Syntax {
        match self.find_by_extension(extension) {
            Some(handle) => &mut self.syntaxes[handle.0],
            None => {
                let mut syntax = Syntax::new();
                syntax.add_extension(extension.into());
                self.syntaxes.push(syntax);
                let last_index = self.syntaxes.len() - 1;
                &mut self.syntaxes[last_index]
            }
        }
    }

    pub fn get(&self, handle: SyntaxHandle) -> &Syntax {
        &self.syntaxes[handle.0]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Syntax> {
        self.syntaxes.iter()
    }
}

#[derive(Default, Clone)]
struct HighlightedLine {
    kind: LineKind,
    tokens: Vec<Token>,
}

#[derive(Default)]
pub struct HighlightedBuffer {
    lines: Vec<HighlightedLine>,
}

impl HighlightedBuffer {
    pub fn highligh_all(&mut self, syntax: &Syntax, buffer: &BufferContent) {
        self.lines
            .resize(buffer.line_count(), HighlightedLine::default());

        let mut previous_line_kind = LineKind::Finished;
        for (bline, hline) in buffer.lines_from(0).zip(self.lines.iter_mut()) {
            hline.kind = syntax.parse_line(bline.text(..), previous_line_kind, &mut hline.tokens);
            previous_line_kind = hline.kind;
        }
    }

    pub fn on_insert(&mut self, syntax: &Syntax, buffer: &BufferContent, range: BufferRange) {
        let mut previous_line_kind = self.previous_line_kind_at(range.from.line_index);

        let insert_index = range.from.line_index + 1;
        let insert_count = range.to.line_index - range.from.line_index;
        self.lines.splice(
            insert_index..insert_index,
            iter::repeat(HighlightedLine::default()).take(insert_count),
        );

        for (bline, hline) in buffer
            .lines_from(range.from.line_index)
            .zip(self.lines[range.from.line_index..].iter_mut())
            .take(insert_count + 1)
        {
            hline.kind = syntax.parse_line(bline.text(..), previous_line_kind, &mut hline.tokens);
            previous_line_kind = hline.kind;
        }

        self.fix_highlight_from(syntax, buffer, previous_line_kind, range.to.line_index + 1);
    }

    pub fn on_delete(&mut self, syntax: &Syntax, buffer: &BufferContent, range: BufferRange) {
        let previous_line_kind = self.previous_line_kind_at(range.from.line_index);
        self.lines.drain(range.from.line_index..range.to.line_index);

        let bline = buffer.line(range.from.line_index);
        let hline = &mut self.lines[range.from.line_index];
        hline.kind = syntax.parse_line(bline.text(..), previous_line_kind, &mut hline.tokens);
        let previous_line_kind = hline.kind;

        self.fix_highlight_from(syntax, buffer, previous_line_kind, range.to.line_index + 1);
    }

    fn previous_line_kind_at(&self, index: usize) -> LineKind {
        if index > 0 {
            self.lines[index].kind
        } else {
            LineKind::Finished
        }
    }

    fn fix_highlight_from(
        &mut self,
        syntax: &Syntax,
        buffer: &BufferContent,
        mut previous_line_kind: LineKind,
        fix_from_index: usize,
    ) {
        if fix_from_index > self.lines.len() {
            return;
        }

        for (bline, hline) in buffer
            .lines_from(fix_from_index)
            .zip(self.lines[fix_from_index..].iter_mut())
        {
            if previous_line_kind == LineKind::Finished && hline.kind == LineKind::Finished {
                break;
            }

            hline.kind = syntax.parse_line(bline.text(..), previous_line_kind, &mut hline.tokens);
            previous_line_kind = hline.kind;
        }
    }

    pub fn find_token_kind_at(&self, line_index: usize, char_index: usize) -> TokenKind {
        if line_index >= self.lines.len() {
            return TokenKind::Text;
        }

        let tokens = &self.lines[line_index].tokens;
        match tokens.binary_search_by(|t| {
            if char_index < t.range.start {
                Ordering::Greater
            } else if char_index >= t.range.end {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }) {
            Ok(index) => tokens[index].kind,
            Err(_) => TokenKind::Text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_token(slice: &str, kind: TokenKind, line: &str, token: &Token) {
        assert_eq!(kind, token.kind);
        assert_eq!(slice, &line[token.range.clone()]);
    }

    #[test]
    fn test_no_syntax() {
        let syntax = Syntax::new();
        let mut tokens = Vec::new();
        let line = " fn main() ;  ";
        let line_kind = syntax.parse_line(line, LineKind::Finished, &mut tokens);

        assert_eq!(LineKind::Finished, line_kind);
        assert_eq!(1, tokens.len());
        assert_token(line, TokenKind::Text, line, &tokens[0]);
    }

    #[test]
    fn test_one_rule_syntax() {
        let mut syntax = Syntax::new();
        syntax.add_rule(TokenKind::Symbol, Pattern::new(";").unwrap());

        let mut tokens = Vec::new();
        let line = " fn main() ;  ";
        let line_kind = syntax.parse_line(line, LineKind::Finished, &mut tokens);

        assert_eq!(LineKind::Finished, line_kind);
        assert_eq!(6, tokens.len());
        assert_token(" fn", TokenKind::Text, line, &tokens[0]);
        assert_token(" main", TokenKind::Text, line, &tokens[1]);
        assert_token("(", TokenKind::Text, line, &tokens[2]);
        assert_token(")", TokenKind::Text, line, &tokens[3]);
        assert_token(" ;", TokenKind::Symbol, line, &tokens[4]);
        assert_token("  ", TokenKind::Text, line, &tokens[5]);
    }

    #[test]
    fn test_simple_syntax() {
        let mut syntax = Syntax::new();
        syntax.add_rule(TokenKind::Keyword, Pattern::new("fn").unwrap());
        syntax.add_rule(TokenKind::Symbol, Pattern::new("%(").unwrap());
        syntax.add_rule(TokenKind::Symbol, Pattern::new("%)").unwrap());

        let mut tokens = Vec::new();
        let line = " fn main() ;  ";
        let line_kind = syntax.parse_line(line, LineKind::Finished, &mut tokens);

        assert_eq!(LineKind::Finished, line_kind);
        assert_eq!(6, tokens.len());
        assert_token(" fn", TokenKind::Keyword, line, &tokens[0]);
        assert_token(" main", TokenKind::Text, line, &tokens[1]);
        assert_token("(", TokenKind::Symbol, line, &tokens[2]);
        assert_token(")", TokenKind::Symbol, line, &tokens[3]);
        assert_token(" ;", TokenKind::Text, line, &tokens[4]);
        assert_token("  ", TokenKind::Text, line, &tokens[5]);
    }

    #[test]
    fn test_multiline_syntax() {
        let mut syntax = Syntax::new();
        syntax.add_rule(TokenKind::Comment, Pattern::new("/*{!(*/).$}").unwrap());

        let mut tokens = Vec::new();
        let line0 = "before /* comment";
        let line1 = "only comment";
        let line2 = "still comment */ after";

        let line0_kind = syntax.parse_line(line0, LineKind::Finished, &mut tokens);
        match line0_kind {
            LineKind::Unfinished(i, _) => assert_eq!(0, i),
            _ => panic!("{:?}", line0_kind),
        }
        assert_eq!(2, tokens.len());
        assert_token("before", TokenKind::Text, line0, &tokens[0]);
        assert_token(" /* comment", TokenKind::Comment, line0, &tokens[1]);

        let line1_kind = syntax.parse_line(line1, line0_kind, &mut tokens);
        match line1_kind {
            LineKind::Unfinished(i, _) => assert_eq!(0, i),
            _ => panic!("{:?}", line1_kind),
        }
        assert_eq!(1, tokens.len());
        assert_token("only comment", TokenKind::Comment, line1, &tokens[0]);

        let line2_kind = syntax.parse_line(line2, line1_kind, &mut tokens);
        assert_eq!(LineKind::Finished, line2_kind);
        assert_eq!(2, tokens.len());
        assert_token("still comment */", TokenKind::Comment, line2, &tokens[0]);
        assert_token(" after", TokenKind::Text, line2, &tokens[1]);
    }
}
