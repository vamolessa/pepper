use std::{cmp::Ordering, iter, ops::Range};

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

#[derive(Default, Clone)]
struct HighlightedLine {
    parse_state: LineParseState,
    tokens: Vec<Token>,
    dirty: bool,
}

struct HighlightedLinePool {
    pool: Vec<HighlightedLine>,
}
impl HighlightedLinePool {
    pub const fn new() -> Self {
        Self { pool: Vec::new() }
    }

    pub fn rent(&mut self) -> HighlightedLine {
        match self.pool.pop() {
            Some(mut line) => {
                line.tokens.clear();
                line.parse_state = LineParseState::Finished;
                line
            }
            None => HighlightedLine::default(),
        }
    }

    pub fn dispose(&mut self, line: HighlightedLine) {
        self.pool.push(line);
    }
}

pub struct HighlightedBuffer {
    lines: Vec<HighlightedLine>,
    line_pool: HighlightedLinePool,
}

impl HighlightedBuffer {
    pub fn empty() -> &'static Self {
        static EMPTY: HighlightedBuffer = HighlightedBuffer {
            lines: Vec::new(),
            line_pool: HighlightedLinePool::new(),
        };
        &EMPTY
    }

    pub fn new() -> Self {
        Self {
            lines: vec![HighlightedLine::default()],
            line_pool: HighlightedLinePool::new(),
        }
    }

    pub fn clear(&mut self) {
        for line in self.lines.drain(..) {
            self.line_pool.dispose(line);
        }
        self.lines.push(self.line_pool.rent());
    }

    pub fn on_insert(&mut self, syntax: &Syntax, buffer: &BufferContent, range: BufferRange) {
        self.require_size(range.from.line_index + 1);
        let pool = &mut self.line_pool;
        let insert_index = range.from.line_index + 1;
        let insert_count = range.to.line_index - range.from.line_index;
        self.lines.splice(
            insert_index..insert_index,
            iter::repeat_with(|| pool.rent()).take(insert_count),
        );
        self.highlight_line_range(syntax, buffer, range.from.line_index, insert_count);
    }

    pub fn on_delete(&mut self, syntax: &Syntax, buffer: &BufferContent, range: BufferRange) {
        self.require_size(range.to.line_index + 1);
        for line in self.lines.drain(range.from.line_index..range.to.line_index) {
            self.line_pool.dispose(line);
        }
        self.highlight_line_range(syntax, buffer, range.from.line_index, 1);
    }

    pub fn refresh(&mut self, syntax: &Syntax, buffer: &BufferContent) {
        self.require_size(buffer.line_count());
        self.highlight_line_range(syntax, buffer, 0, buffer.line_count());
    }

    fn require_size(&mut self, min_len: usize) {
        if self.lines.len() < min_len {
            let pool = &mut self.line_pool;
            self.lines.resize_with(min_len, || pool.rent());
        }
    }

    fn highlight_line_range(
        &mut self,
        syntax: &Syntax,
        buffer: &BufferContent,
        index: usize,
        len: usize,
    ) {
        let mut previous_parse_state = match index.checked_sub(1) {
            Some(i) => self.lines[i].parse_state,
            None => LineParseState::Finished,
        };

        for (bline, hline) in buffer
            .lines()
            .zip(self.lines.iter_mut())
            .skip(index)
            .take(len)
        {
            previous_parse_state =
                syntax.parse_line(bline.as_str(), previous_parse_state, &mut hline.tokens);
            hline.parse_state = previous_parse_state;
        }

        for (bline, hline) in buffer.lines().zip(self.lines.iter_mut()).skip(index + len) {
            let previous_state = hline.parse_state;
            previous_parse_state =
                syntax.parse_line(bline.as_str(), previous_parse_state, &mut hline.tokens);
            hline.parse_state = previous_parse_state;

            if previous_state == LineParseState::Finished
                && !matches!(previous_parse_state, LineParseState::Unfinished(_, _))
            {
                break;
            }
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
        let range = buffer.insert_text(BufferPosition::line_col(0, 0), "/*\n*/");

        let mut highlighted = HighlightedBuffer::new();
        highlighted.on_insert(&syntax, &buffer, range);
        assert_eq!(buffer.line_count(), highlighted.lines.len());

        let mut tokens = highlighted.lines.iter().map(|l| l.tokens.iter()).flatten();
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_eq!(None, tokens.next());

        let range = buffer.insert_text(BufferPosition::line_col(1, 0), "'");
        highlighted.on_insert(&syntax, &buffer, range);

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
        let range = buffer.insert_text(BufferPosition::line_col(0, 0), "/*\n\n\n*/");

        let mut highlighted = HighlightedBuffer::new();
        highlighted.on_insert(&syntax, &buffer, range);
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
        let range = buffer.insert_text(BufferPosition::line_col(0, 0), "/*\n* /\n*/");

        let mut highlighted = HighlightedBuffer::new();
        highlighted.on_insert(&syntax, &buffer, range);

        let range = BufferRange::between(
            BufferPosition::line_col(1, 1),
            BufferPosition::line_col(1, 2),
        );
        buffer.delete_range(range);
        highlighted.on_delete(&syntax, &buffer, range);

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
        let range = buffer.insert_text(BufferPosition::line_col(0, 0), "/ *\na\n*/");

        let mut highlighted = HighlightedBuffer::new();
        highlighted.on_insert(&syntax, &buffer, range);

        let range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(0, 2),
        );
        buffer.delete_range(range);
        highlighted.on_delete(&syntax, &buffer, range);

        let mut tokens = highlighted.lines.iter().map(|l| l.tokens.iter()).flatten();
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_next_token!(tokens, TokenKind::Comment, 0..1);
        assert_next_token!(tokens, TokenKind::Comment, 0..2);
        assert_eq!(None, tokens.next());
    }
}
