use std::{cmp::Ordering, ops::Range};

use crate::{
    buffer::BufferContent,
    buffer_position::BufferRange,
    glob::Glob,
    pattern::{MatchResult, Pattern, PatternState},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    pub kind: TokenKind,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineParseState {
    Finished,
    Unfinished(usize, PatternState),
}

impl Default for LineParseState {
    fn default() -> Self {
        Self::Finished
    }
}

#[derive(Default)]
pub struct Syntax {
    glob: Glob,
    rules: Vec<(TokenKind, Pattern)>,
}

impl Syntax {
    pub fn set_glob(&mut self, pattern: &[u8]) {
        let _ = self.glob.compile(pattern);
    }

    pub fn add_rule(&mut self, kind: TokenKind, pattern: Pattern) {
        self.rules.push((kind, pattern));
    }

    fn parse_line(
        &self,
        line: &str,
        previous_parse_state: LineParseState,
        tokens: &mut Vec<Token>,
    ) -> LineParseState {
        tokens.clear();

        if self.rules.is_empty() {
            tokens.push(Token {
                kind: TokenKind::Text,
                range: 0..line.len(),
            });
            return LineParseState::Finished;
        }

        let line_len = line.len();
        let mut line_index = 0;

        match previous_parse_state {
            LineParseState::Finished => (),
            LineParseState::Unfinished(pattern_index, state) => {
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
                        return LineParseState::Unfinished(pattern_index, state);
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
                        return LineParseState::Unfinished(i, state);
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

        LineParseState::Finished
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct SyntaxHandle(usize);

pub struct SyntaxCollection {
    syntaxes: Vec<Syntax>,
}

impl SyntaxCollection {
    pub fn new() -> Self {
        let mut syntaxes = Vec::new();
        syntaxes.push(Syntax::default());
        Self { syntaxes }
    }

    pub fn find_handle_by_path(&self, path: &[u8]) -> Option<SyntaxHandle> {
        let mut iter = self.syntaxes.iter().enumerate();
        iter.next();
        for (i, syntax) in iter {
            if syntax.glob.matches(path) {
                return Some(SyntaxHandle(i));
            }
        }

        None
    }

    pub fn add(&mut self, syntax: Syntax) {
        self.syntaxes.push(syntax);
    }

    pub fn get(&self, handle: SyntaxHandle) -> &Syntax {
        &self.syntaxes[handle.0]
    }
}

#[derive(Default)]
struct HighlightedLine {
    parse_state: LineParseState,
    tokens: Vec<Token>,
}

pub struct HighlightedBuffer {
    highlighted_len: usize,
    lines: Vec<HighlightedLine>,
    dirty_line_indexes: Vec<usize>,
}

impl HighlightedBuffer {
    pub fn empty() -> &'static Self {
        static EMPTY: HighlightedBuffer = HighlightedBuffer {
            highlighted_len: 0,
            lines: Vec::new(),
            dirty_line_indexes: Vec::new(),
        };
        &EMPTY
    }

    pub fn new() -> Self {
        Self {
            highlighted_len: 1,
            lines: vec![HighlightedLine::default()],
            dirty_line_indexes: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.highlighted_len = 1;
        self.dirty_line_indexes.clear();
    }

    pub fn on_insert(&mut self, range: BufferRange) {
        let insert_line_count = range.to.line_index - range.from.line_index;
        self.highlighted_len += insert_line_count;

        if insert_line_count > 0 {
            if self.highlighted_len > self.lines.len() {
                self.lines
                    .resize_with(self.highlighted_len, HighlightedLine::default);
            }

            let insert_index = range.from.line_index + 1;
            self.lines[insert_index..].rotate_right(insert_line_count);

            for index in &mut self.dirty_line_indexes {
                if insert_index <= *index {
                    *index += insert_line_count;
                }
            }
        }

        for i in range.from.line_index..=range.to.line_index {
            self.dirty_line_indexes.push(i);
        }
    }

    pub fn on_delete(&mut self, range: BufferRange) {
        let delete_line_count = range.to.line_index - range.from.line_index;
        self.highlighted_len -= delete_line_count;

        if delete_line_count > 0 {
            let delete_index = range.from.line_index + 1;
            self.lines[delete_index..].rotate_left(delete_line_count);

            for index in &mut self.dirty_line_indexes {
                if range.to.line_index <= *index {
                    *index -= delete_line_count;
                } else if delete_index <= *index {
                    *index = range.from.line_index;
                }
            }
        }

        self.dirty_line_indexes.push(range.from.line_index);
    }

    pub fn highlight_dirty_lines(&mut self, syntax: &Syntax, buffer: &BufferContent) {
        if self.dirty_line_indexes.is_empty() {
            return;
        }

        self.dirty_line_indexes.sort();
        let mut index = self.dirty_line_indexes[0];
        let mut last_dirty_index = usize::MAX;

        let mut previous_parse_state = match index.checked_sub(1) {
            Some(i) => self.lines[i].parse_state,
            None => LineParseState::Finished,
        };

        for dirty_index in &self.dirty_line_indexes {
            let dirty_index = *dirty_index;
            if dirty_index < index || dirty_index == last_dirty_index {
                continue;
            }

            index = dirty_index;
            last_dirty_index = index;

            while index < self.lines.len() {
                let bline = buffer.line_at(index);
                let hline = &mut self.lines[index];

                let previous_state = hline.parse_state;
                previous_parse_state =
                    syntax.parse_line(bline.as_str(), previous_parse_state, &mut hline.tokens);
                hline.parse_state = previous_parse_state;

                index += 1;

                if previous_state == LineParseState::Finished
                    && previous_parse_state == LineParseState::Finished
                {
                    break;
                }
            }
        }

        self.dirty_line_indexes.clear();
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

    use crate::buffer_position::BufferPosition;

    macro_rules! assert_next_token {
        ($iter:expr, $kind:expr, $range:expr) => {
            assert_eq!(
                Some(Token {
                    kind: $kind,
                    range: $range,
                }),
                $iter.next().cloned(),
            );
        };
    }

    fn assert_token(slice: &str, kind: TokenKind, line: &str, token: &Token) {
        assert_eq!(kind, token.kind);
        assert_eq!(slice, &line[token.range.clone()]);
    }

    #[test]
    fn no_syntax() {
        let syntax = Syntax::default();
        let mut tokens = Vec::new();
        let line = " fn main() ;  ";
        let parse_state = syntax.parse_line(line, LineParseState::Finished, &mut tokens);

        assert_eq!(LineParseState::Finished, parse_state);
        assert_eq!(1, tokens.len());
        assert_token(line, TokenKind::Text, line, &tokens[0]);
    }

    #[test]
    fn one_rule_syntax() {
        let mut syntax = Syntax::default();
        syntax.add_rule(TokenKind::Symbol, Pattern::new(";").unwrap());

        let mut tokens = Vec::new();
        let line = " fn main() ;  ";
        let parse_state = syntax.parse_line(line, LineParseState::Finished, &mut tokens);

        assert_eq!(LineParseState::Finished, parse_state);
        assert_eq!(6, tokens.len());
        assert_token(" fn", TokenKind::Text, line, &tokens[0]);
        assert_token(" main", TokenKind::Text, line, &tokens[1]);
        assert_token("(", TokenKind::Text, line, &tokens[2]);
        assert_token(")", TokenKind::Text, line, &tokens[3]);
        assert_token(" ;", TokenKind::Symbol, line, &tokens[4]);
        assert_token("  ", TokenKind::Text, line, &tokens[5]);
    }

    #[test]
    fn simple_syntax() {
        let mut syntax = Syntax::default();
        syntax.add_rule(TokenKind::Keyword, Pattern::new("fn").unwrap());
        syntax.add_rule(TokenKind::Symbol, Pattern::new("%(").unwrap());
        syntax.add_rule(TokenKind::Symbol, Pattern::new("%)").unwrap());

        let mut tokens = Vec::new();
        let line = " fn main() ;  ";
        let parse_state = syntax.parse_line(line, LineParseState::Finished, &mut tokens);

        assert_eq!(LineParseState::Finished, parse_state);
        assert_eq!(6, tokens.len());
        assert_token(" fn", TokenKind::Keyword, line, &tokens[0]);
        assert_token(" main", TokenKind::Text, line, &tokens[1]);
        assert_token("(", TokenKind::Symbol, line, &tokens[2]);
        assert_token(")", TokenKind::Symbol, line, &tokens[3]);
        assert_token(" ;", TokenKind::Text, line, &tokens[4]);
        assert_token("  ", TokenKind::Text, line, &tokens[5]);
    }

    #[test]
    fn multiline_syntax() {
        let mut syntax = Syntax::default();
        syntax.add_rule(TokenKind::Comment, Pattern::new("/*{!(*/).$}").unwrap());

        let mut tokens = Vec::new();
        let line0 = "before /* comment";
        let line1 = "only comment";
        let line2 = "still comment */ after";

        let line0_kind = syntax.parse_line(line0, LineParseState::Finished, &mut tokens);
        match line0_kind {
            LineParseState::Unfinished(i, _) => assert_eq!(0, i),
            _ => panic!("{:?}", line0_kind),
        }
        assert_eq!(2, tokens.len());
        assert_token("before", TokenKind::Text, line0, &tokens[0]);
        assert_token(" /* comment", TokenKind::Comment, line0, &tokens[1]);

        let line1_kind = syntax.parse_line(line1, line0_kind, &mut tokens);
        match line1_kind {
            LineParseState::Unfinished(i, _) => assert_eq!(0, i),
            _ => panic!("{:?}", line1_kind),
        }
        assert_eq!(1, tokens.len());
        assert_token("only comment", TokenKind::Comment, line1, &tokens[0]);

        let line2_kind = syntax.parse_line(line2, line1_kind, &mut tokens);
        assert_eq!(LineParseState::Finished, line2_kind);
        assert_eq!(2, tokens.len());
        assert_token("still comment */", TokenKind::Comment, line2, &tokens[0]);
        assert_token(" after", TokenKind::Text, line2, &tokens[1]);
    }

    #[test]
    fn editing_highlighted_buffer() {
        let mut syntax = Syntax::default();
        syntax.add_rule(TokenKind::Comment, Pattern::new("/*{!(*/).$}").unwrap());
        syntax.add_rule(TokenKind::String, Pattern::new("'{!'.$}").unwrap());

        let mut buffer = BufferContent::new();
        let mut highlighted = HighlightedBuffer::new();

        let range = buffer.insert_text(BufferPosition::line_col(0, 0), "/*\n*/");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);
        assert_eq!(buffer.line_count(), highlighted.lines.len());

        let mut tokens = highlighted.lines.iter().map(|l| l.tokens.iter()).flatten();
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_eq!(None, tokens.next());

        let range = buffer.insert_text(BufferPosition::line_col(1, 0), "'");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        let mut tokens = highlighted.lines.iter().map(|l| l.tokens.iter()).flatten();
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_next_token!(tokens, TokenKind::Comment, 0..3);
        assert_eq!(None, tokens.next());
    }

    #[test]
    fn highlight_range_after_unfinished_line() {
        let mut syntax = Syntax::default();
        syntax.add_rule(TokenKind::Comment, Pattern::new("/*{!(*/).$}").unwrap());

        let mut buffer = BufferContent::new();
        let mut highlighted = HighlightedBuffer::new();

        let range = buffer.insert_text(BufferPosition::line_col(0, 0), "/*\n\n\n*/");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);
        assert_eq!(buffer.line_count(), highlighted.lines.len());

        let mut tokens = highlighted.lines.iter().map(|l| l.tokens.iter()).flatten();
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_next_token!(tokens, TokenKind::Comment, 0..0);
        assert_next_token!(tokens, TokenKind::Comment, 0..0);
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_eq!(None, tokens.next());
    }

    #[test]
    fn highlight_lines_after_unfinished_to_finished() {
        let mut syntax = Syntax::default();
        syntax.add_rule(TokenKind::Comment, Pattern::new("/*{!(*/).$}").unwrap());

        let mut buffer = BufferContent::new();
        let mut highlighted = HighlightedBuffer::new();

        let range = buffer.insert_text(BufferPosition::line_col(0, 0), "/*\n* /\n*/");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        let range = BufferRange::between(
            BufferPosition::line_col(1, 1),
            BufferPosition::line_col(1, 2),
        );
        buffer.delete_range(range);
        highlighted.on_delete(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        let mut parse_states = highlighted.lines.iter().map(|l| l.parse_state);
        assert!(matches!(
            parse_states.next(),
            Some(LineParseState::Unfinished(_, _))
        ));
        assert_eq!(Some(LineParseState::Finished), parse_states.next());
        assert_eq!(Some(LineParseState::Finished), parse_states.next());
        assert_eq!(None, parse_states.next());

        let mut tokens = highlighted.lines.iter().map(|l| l.tokens.iter()).flatten();
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_next_token!(tokens, TokenKind::Text, 0..1);
        assert_next_token!(tokens, TokenKind::Text, 1..2);
        assert_eq!(None, tokens.next());
    }

    #[test]
    fn highlight_lines_after_became_unfinished() {
        let mut syntax = Syntax::default();
        syntax.add_rule(TokenKind::Comment, Pattern::new("/*{!(*/).$}").unwrap());

        let mut buffer = BufferContent::new();
        let mut highlighted = HighlightedBuffer::new();

        let range = buffer.insert_text(BufferPosition::line_col(0, 0), "/ *\na\n*/");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        let range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(0, 2),
        );
        buffer.delete_range(range);
        highlighted.on_delete(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        let mut tokens = highlighted.lines.iter().map(|l| l.tokens.iter()).flatten();
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_next_token!(tokens, TokenKind::Comment, 0..1);
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_eq!(None, tokens.next());
    }
}
