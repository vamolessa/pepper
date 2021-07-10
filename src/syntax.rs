use std::str::FromStr;

use crate::{
    buffer::BufferContent,
    buffer_position::{BufferPositionIndex, BufferRange},
    editor_utils::hash_bytes,
    glob::{Glob, InvalidGlobError},
    pattern::{MatchResult, Pattern, PatternError, PatternState},
};

const MAX_HIGHLIGHT_COUNT: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Keyword,
    Type,
    Symbol,
    Literal,
    String,
    Comment,
    Text,
    Whitespace,
}
impl FromStr for TokenKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "keywords" => Ok(Self::Keyword),
            "types" => Ok(Self::Type),
            "symbols" => Ok(Self::Symbol),
            "literals" => Ok(Self::Literal),
            "strings" => Ok(Self::String),
            "comments" => Ok(Self::Comment),
            "texts" => Ok(Self::Text),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub from: BufferPositionIndex,
    pub to: BufferPositionIndex,
}
impl Token {
    pub fn contains(&self, column_byte_index: BufferPositionIndex) -> bool {
        self.from <= column_byte_index && column_byte_index < self.to
    }
}
impl Default for Token {
    fn default() -> Self {
        Self {
            kind: TokenKind::Text,
            from: 0,
            to: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineParseState {
    Dirty,
    Finished,
    Unfinished(TokenKind, PatternState),
}

impl Default for LineParseState {
    fn default() -> Self {
        Self::Dirty
    }
}

pub struct Syntax {
    glob_hash: u64,
    glob: Glob,
    rules: [Pattern; 7],
}

impl Syntax {
    pub fn new() -> Self {
        let mut text_pattern = Pattern::new();
        let _ = text_pattern.compile("%a{%w_}|_{%w_}");
        Self {
            glob_hash: 0,
            glob: Glob::default(),
            rules: [
                Pattern::new(),
                Pattern::new(),
                Pattern::new(),
                Pattern::new(),
                Pattern::new(),
                Pattern::new(),
                text_pattern,
            ],
        }
    }

    pub fn set_glob(&mut self, pattern: &str) -> Result<(), InvalidGlobError> {
        self.glob_hash = hash_bytes(pattern.as_bytes());
        self.glob.compile(pattern)
    }

    pub fn set_rule<'a>(&mut self, kind: TokenKind, pattern: &'a str) -> Result<(), PatternError> {
        self.rules[kind as usize].compile(pattern)
    }

    fn parse_line(
        &self,
        line: &str,
        previous_parse_state: LineParseState,
        tokens: &mut Vec<Token>,
    ) -> LineParseState {
        tokens.clear();

        let line_len = line.len();
        let mut index = 0;

        match previous_parse_state {
            LineParseState::Dirty => unreachable!(),
            LineParseState::Finished => (),
            LineParseState::Unfinished(kind, state) => {
                match self.rules[kind as usize].matches_with_state(line, state) {
                    MatchResult::Ok(len) => {
                        tokens.push(Token {
                            kind,
                            from: 0,
                            to: len as _,
                        });
                        index += len;
                    }
                    MatchResult::Err => (),
                    MatchResult::Pending(state) => {
                        tokens.push(Token {
                            kind,
                            from: 0,
                            to: line_len as _,
                        });
                        return LineParseState::Unfinished(kind, state);
                    }
                }
            }
        }

        while index < line_len {
            let line_slice = &line[index..];
            let whitespace_len = line_slice
                .bytes()
                .take_while(u8::is_ascii_whitespace)
                .count();
            let line_slice = &line_slice[whitespace_len..];

            let mut best_pattern_kind = TokenKind::Text;
            let mut max_len = 0;

            const ALL_NON_WHITESPACE_TOKEN_KINDS: [TokenKind; 7] = [
                TokenKind::Keyword,
                TokenKind::Type,
                TokenKind::Symbol,
                TokenKind::Literal,
                TokenKind::String,
                TokenKind::Comment,
                TokenKind::Text,
            ];

            for &kind in ALL_NON_WHITESPACE_TOKEN_KINDS.iter() {
                let pattern = &self.rules[kind as usize];
                match pattern.matches(line_slice) {
                    MatchResult::Ok(len) => {
                        if len > max_len {
                            max_len = len;
                            best_pattern_kind = kind;
                        }
                    }
                    MatchResult::Err => (),
                    MatchResult::Pending(state) => {
                        tokens.push(Token {
                            kind,
                            from: index as _,
                            to: line_len as _,
                        });
                        return LineParseState::Unfinished(kind, state);
                    }
                }
            }

            let mut kind = best_pattern_kind;

            if max_len == 0 {
                kind = TokenKind::Text;
                max_len = line_slice
                    .bytes()
                    .take_while(u8::is_ascii_alphanumeric)
                    .count()
                    .max(1);
            }

            max_len += whitespace_len;

            let from = index;
            index = line_len.min(index + max_len);

            while !line.is_char_boundary(index) {
                index += 1;
            }

            tokens.push(Token {
                kind,
                from: from as _,
                to: index as _,
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
        syntaxes.push(Syntax::new());
        Self { syntaxes }
    }

    pub fn find_handle_by_path(&self, path: &str) -> Option<SyntaxHandle> {
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
        for s in &mut self.syntaxes {
            if s.glob_hash == syntax.glob_hash {
                *s = syntax;
                return;
            }
        }
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

pub enum HighlightResult {
    Complete,
    Pending,
}

pub struct HighlightedBuffer {
    highlighted_len: usize,
    lines: Vec<HighlightedLine>,
    dirty_line_indexes: Vec<BufferPositionIndex>,
}

impl HighlightedBuffer {
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
        if insert_line_count > 0 {
            let previous_highlighted_len = self.highlighted_len;
            self.highlighted_len += insert_line_count as usize;
            if self.highlighted_len > self.lines.len() {
                for line in &mut self.lines[previous_highlighted_len..] {
                    line.parse_state = LineParseState::Dirty;
                }
                self.lines
                    .resize_with(self.highlighted_len, HighlightedLine::default);
            } else {
                for line in &mut self.lines[previous_highlighted_len..self.highlighted_len] {
                    line.parse_state = LineParseState::Dirty;
                }
            }

            let insert_index = range.from.line_index + 1;
            self.lines[insert_index as usize..self.highlighted_len as usize]
                .rotate_right(insert_line_count as _);

            for index in &mut self.dirty_line_indexes {
                if insert_index <= *index {
                    *index += insert_line_count;
                }
            }
        }

        self.lines[range.from.line_index as usize].parse_state = LineParseState::Dirty;
        self.dirty_line_indexes.push(range.from.line_index);
    }

    pub fn on_delete(&mut self, range: BufferRange) {
        self.lines[range.from.line_index as usize].parse_state = LineParseState::Dirty;

        let delete_line_count = range.to.line_index - range.from.line_index;
        if delete_line_count > 0 {
            self.highlighted_len -= delete_line_count as usize;
            let delete_index = range.from.line_index + 1;
            self.lines[delete_index as usize..].rotate_left(delete_line_count as _);

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

    pub fn highlight_dirty_lines(
        &mut self,
        syntax: &Syntax,
        buffer: &BufferContent,
    ) -> HighlightResult {
        if self.dirty_line_indexes.is_empty() {
            return HighlightResult::Complete;
        }

        self.dirty_line_indexes.sort_unstable();

        let mut index = self.dirty_line_indexes[0];
        let mut last_dirty_index = BufferPositionIndex::MAX;

        let mut previous_parse_state = match index.checked_sub(1) {
            Some(i) => self.lines[i as usize].parse_state,
            None => LineParseState::Finished,
        };

        let mut highlight_count = 0;
        let mut i = 0;
        while i < self.dirty_line_indexes.len() {
            let dirty_index = self.dirty_line_indexes[i];
            i += 1;

            if dirty_index < index || dirty_index == last_dirty_index {
                continue;
            }

            index = dirty_index;
            last_dirty_index = dirty_index;

            while index < self.highlighted_len as _ {
                let bline = buffer.line_at(index as _).as_str();
                let hline = &mut self.lines[index as usize];

                let previous_state = hline.parse_state;
                previous_parse_state =
                    syntax.parse_line(bline, previous_parse_state, &mut hline.tokens);
                hline.parse_state = previous_parse_state;

                index += 1;
                highlight_count += 1;

                if highlight_count == MAX_HIGHLIGHT_COUNT {
                    i -= 1;
                    self.dirty_line_indexes[i] = index;
                    self.dirty_line_indexes.drain(..i);
                    return HighlightResult::Pending;
                }

                if previous_state == LineParseState::Finished
                    && previous_parse_state == LineParseState::Finished
                {
                    break;
                }
            }
        }

        self.dirty_line_indexes.clear();
        HighlightResult::Complete
    }

    pub fn line_tokens(&self, line_index: usize) -> &[Token] {
        if line_index < self.highlighted_len {
            &self.lines[line_index].tokens
        } else {
            &[]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ops::Range;

    use crate::buffer_position::BufferPosition;

    fn assert_next_token<'a, I>(iter: &mut I, kind: TokenKind, range: Range<usize>)
    where
        I: Iterator<Item = &'a Token>,
    {
        let expect = Some(Token {
            kind,
            from: range.start as _,
            to: range.end as _,
        });
        assert_eq!(expect, iter.next().cloned());
    }

    fn highlighted_tokens<'a>(
        highlighted: &'a HighlightedBuffer,
    ) -> impl 'a + Iterator<Item = &'a Token> {
        highlighted.lines[..highlighted.highlighted_len]
            .iter()
            .map(|l| l.tokens.iter())
            .flatten()
    }

    fn assert_token(slice: &str, kind: TokenKind, line: &str, token: &Token) {
        assert_eq!(kind, token.kind);
        assert_eq!(slice, &line[token.from as usize..token.to as usize]);
    }

    #[test]
    fn no_syntax() {
        let syntax = Syntax::new();
        let mut tokens = Vec::new();
        let line = " fn main() ;  ";
        let parse_state = syntax.parse_line(line, LineParseState::Finished, &mut tokens);

        assert_eq!(LineParseState::Finished, parse_state);
        assert_eq!(6, tokens.len());
        assert_token(" fn", TokenKind::Text, line, &tokens[0]);
        assert_token(" main", TokenKind::Text, line, &tokens[1]);
        assert_token("(", TokenKind::Text, line, &tokens[2]);
        assert_token(")", TokenKind::Text, line, &tokens[3]);
        assert_token(" ;", TokenKind::Text, line, &tokens[4]);
        assert_token("  ", TokenKind::Text, line, &tokens[5]);
    }

    #[test]
    fn one_rule_syntax() {
        let mut syntax = Syntax::new();
        syntax.set_rule(TokenKind::Symbol, ";").unwrap();

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
        let mut syntax = Syntax::new();
        syntax.set_rule(TokenKind::Keyword, "fn").unwrap();
        syntax.set_rule(TokenKind::Symbol, "%(|%)").unwrap();

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
        let mut syntax = Syntax::new();
        syntax.set_rule(TokenKind::Comment, "/*{!(*/).$}").unwrap();

        let mut tokens = Vec::new();
        let line0 = "before /* comment";
        let line1 = "only comment";
        let line2 = "still comment */ after";

        let line0_kind = syntax.parse_line(line0, LineParseState::Finished, &mut tokens);
        match line0_kind {
            LineParseState::Unfinished(i, _) => assert_eq!(TokenKind::Comment, i),
            _ => panic!("{:?}", line0_kind),
        }
        assert_eq!(2, tokens.len());
        assert_token("before", TokenKind::Text, line0, &tokens[0]);
        assert_token(" /* comment", TokenKind::Comment, line0, &tokens[1]);

        let line1_kind = syntax.parse_line(line1, line0_kind, &mut tokens);
        match line1_kind {
            LineParseState::Unfinished(i, _) => assert_eq!(TokenKind::Comment, i),
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
        let mut syntax = Syntax::new();
        syntax.set_rule(TokenKind::Comment, "/*{!(*/).$}").unwrap();
        syntax.set_rule(TokenKind::String, "'{!'.$}").unwrap();

        let mut buffer = BufferContent::new();
        let mut highlighted = HighlightedBuffer::new();

        let range = buffer.insert_text(BufferPosition::zero(), "/*\n*/");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);
        assert_eq!(buffer.line_count(), highlighted.lines.len());

        {
            let mut tokens = highlighted_tokens(&highlighted);
            assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
            assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
            assert_eq!(None, tokens.next());
        }

        let range = buffer.insert_text(BufferPosition::line_col(1, 0), "'");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        {
            let mut tokens = highlighted_tokens(&highlighted);
            assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
            assert_next_token(&mut tokens, TokenKind::Comment, 0..3);
            assert_eq!(None, tokens.next());
        }
    }

    #[test]
    fn highlight_range_after_unfinished_line() {
        let mut syntax = Syntax::new();
        syntax.set_rule(TokenKind::Comment, "/*{!(*/).$}").unwrap();

        let mut buffer = BufferContent::new();
        let mut highlighted = HighlightedBuffer::new();

        let range = buffer.insert_text(BufferPosition::zero(), "/*\n\n\n*/");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);
        assert_eq!(buffer.line_count(), highlighted.lines.len());

        let mut tokens = highlighted_tokens(&highlighted);
        assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
        assert_next_token(&mut tokens, TokenKind::Comment, 0..0);
        assert_next_token(&mut tokens, TokenKind::Comment, 0..0);
        assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
        assert_eq!(None, tokens.next());
    }

    #[test]
    fn highlight_lines_after_unfinished_to_finished() {
        let mut syntax = Syntax::new();
        syntax.set_rule(TokenKind::Comment, "/*{!(*/).$}").unwrap();

        let mut buffer = BufferContent::new();
        let mut highlighted = HighlightedBuffer::new();

        let range = buffer.insert_text(BufferPosition::zero(), "/*\n* /\n*/");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        let range = BufferRange::between(
            BufferPosition::line_col(1, 1),
            BufferPosition::line_col(1, 2),
        );
        buffer.delete_range(range);
        highlighted.on_delete(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        let mut parse_states = highlighted.lines[..highlighted.highlighted_len]
            .iter()
            .map(|l| l.parse_state);
        assert!(matches!(
            parse_states.next(),
            Some(LineParseState::Unfinished(_, _))
        ));
        assert_eq!(Some(LineParseState::Finished), parse_states.next());
        assert_eq!(Some(LineParseState::Finished), parse_states.next());
        assert_eq!(None, parse_states.next());

        {
            let mut tokens = highlighted_tokens(&highlighted);
            assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
            assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
            assert_next_token(&mut tokens, TokenKind::Text, 0..1);
            assert_next_token(&mut tokens, TokenKind::Text, 1..2);
            assert_eq!(None, tokens.next());
        }
    }

    #[test]
    fn highlight_lines_after_became_unfinished() {
        let mut syntax = Syntax::new();
        syntax.set_rule(TokenKind::Comment, "/*{!(*/).$}").unwrap();

        let mut buffer = BufferContent::new();
        let mut highlighted = HighlightedBuffer::new();

        let range = buffer.insert_text(BufferPosition::zero(), "/ *\na\n*/");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        let range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(0, 2),
        );
        buffer.delete_range(range);
        highlighted.on_delete(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);

        let mut tokens = highlighted_tokens(&highlighted);
        assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
        assert_next_token(&mut tokens, TokenKind::Comment, 0..1);
        assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
        assert_eq!(None, tokens.next());
    }

    #[test]
    fn highlight_unfinished_lines_on_multiline_delete() {
        let mut syntax = Syntax::new();
        syntax.set_rule(TokenKind::Comment, "/*{!(*/).$}").unwrap();

        let mut buffer = BufferContent::new();
        let mut highlighted = HighlightedBuffer::new();

        let range = buffer.insert_text(BufferPosition::zero(), "a\n/*\nb\nc*/");
        highlighted.on_insert(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);
        assert_eq!(buffer.line_count(), highlighted.highlighted_len);

        {
            let mut tokens = highlighted_tokens(&highlighted);
            assert_next_token(&mut tokens, TokenKind::Text, 0..1);
            assert_next_token(&mut tokens, TokenKind::Comment, 0..2);
            assert_next_token(&mut tokens, TokenKind::Comment, 0..1);
            assert_next_token(&mut tokens, TokenKind::Comment, 0..3);
            assert_eq!(None, tokens.next());
        }

        let range = BufferRange::between(BufferPosition::zero(), BufferPosition::line_col(1, 1));
        buffer.delete_range(range);
        highlighted.on_delete(range);
        highlighted.highlight_dirty_lines(&syntax, &buffer);
        assert_eq!(buffer.line_count(), highlighted.highlighted_len);

        {
            let mut tokens = highlighted_tokens(&highlighted);
            assert_next_token(&mut tokens, TokenKind::Text, 0..1);
            assert_next_token(&mut tokens, TokenKind::Text, 0..1);
            assert_next_token(&mut tokens, TokenKind::Text, 0..1);
            assert_next_token(&mut tokens, TokenKind::Text, 1..2);
            assert_next_token(&mut tokens, TokenKind::Text, 2..3);
            assert_eq!(None, tokens.next());
        }
    }
}
