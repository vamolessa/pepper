use std::{
    convert::From,
    fmt,
    fs::File,
    io,
    ops::RangeBounds,
    path::{Path, PathBuf},
};

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    client::ClientCollection,
    editor_event::{EditorEvent, EditorEventQueue},
    history::{Edit, EditHandle, EditKind, History},
    script::ScriptValue,
    syntax::{HighlightedBuffer, SyntaxCollection, SyntaxHandle},
    word_database::{WordDatabase, WordIter, WordKind},
};

#[derive(Debug)]
enum TextImpl {
    Inline(u8, [u8; Text::inline_string_max_len()]),
    String(String),
}

#[derive(Debug)]
pub struct Text(TextImpl);

impl Text {
    pub const fn inline_string_max_len() -> usize {
        30
    }

    pub fn new() -> Self {
        Self(TextImpl::Inline(0, [0; Self::inline_string_max_len()]))
    }

    pub fn as_str(&self) -> &str {
        match &self.0 {
            TextImpl::Inline(len, buf) => unsafe {
                let len = *len as usize;
                std::str::from_utf8_unchecked(&buf[..len])
            },
            TextImpl::String(s) => s,
        }
    }

    pub fn clear(&mut self) {
        match &mut self.0 {
            TextImpl::Inline(len, _) => *len = 0,
            TextImpl::String(s) => s.clear(),
        }
    }

    pub fn push_str(&mut self, text: &str) {
        match &mut self.0 {
            TextImpl::Inline(len, buf) => {
                let previous_len = *len as usize;
                *len += text.len() as u8;
                if *len as usize <= Self::inline_string_max_len() {
                    buf[previous_len..*len as usize].copy_from_slice(text.as_bytes());
                } else {
                    let mut s = String::with_capacity(*len as _);
                    s.push_str(unsafe { std::str::from_utf8_unchecked(&buf[..previous_len]) });
                    s.push_str(text);
                    *self = Self(TextImpl::String(s));
                }
            }
            TextImpl::String(s) => s.push_str(text),
        }
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Self {
        if s.len() <= Self::inline_string_max_len() {
            let mut buf = [0; Self::inline_string_max_len()];
            buf[..s.len()].copy_from_slice(s.as_bytes());
            Self(TextImpl::Inline(s.len() as _, buf))
        } else {
            Self(TextImpl::String(String::from(s)))
        }
    }
}

impl From<String> for Text {
    fn from(s: String) -> Self {
        if s.len() <= Self::inline_string_max_len() {
            let mut buf = [0; Self::inline_string_max_len()];
            buf[..s.len()].copy_from_slice(s.as_bytes());
            Self(TextImpl::Inline(s.len() as _, buf))
        } else {
            Self(TextImpl::String(s))
        }
    }
}

pub struct WordRefWithIndex<'a> {
    pub kind: WordKind,
    pub text: &'a str,
    pub index: usize,
}
impl<'a> WordRefWithIndex<'a> {
    pub fn to_word_ref_with_position(self, line_index: usize) -> WordRefWithPosition<'a> {
        WordRefWithPosition {
            kind: self.kind,
            text: self.text,
            position: BufferPosition::line_col(line_index, self.index),
        }
    }
}

pub struct WordRefWithPosition<'a> {
    pub kind: WordKind,
    pub text: &'a str,
    pub position: BufferPosition,
}
impl<'a> WordRefWithPosition<'a> {
    pub fn end_position(&self) -> BufferPosition {
        BufferPosition::line_col(
            self.position.line_index,
            self.position.column_byte_index + self.text.len(),
        )
    }
}

#[derive(Default)]
pub struct BufferLinePool {
    pool: Vec<BufferLine>,
}

impl BufferLinePool {
    pub fn rent(&mut self) -> BufferLine {
        match self.pool.pop() {
            Some(mut line) => {
                line.text.clear();
                line.char_count = 0;
                line
            }
            None => BufferLine::new(),
        }
    }

    pub fn read<R>(&mut self, read: &mut R) -> io::Result<Option<BufferLine>>
    where
        R: io::BufRead,
    {
        let mut line = self.rent();
        match read.read_line(&mut line.text) {
            Ok(0) => {
                self.dispose(line);
                Ok(None)
            }
            Ok(_) => {
                if line.text.ends_with('\n') {
                    line.text.truncate(line.text.len() - 1);
                }
                if line.text.ends_with('\r') {
                    line.text.truncate(line.text.len() - 1);
                }

                line.char_count = line.text.chars().count();
                Ok(Some(line))
            }
            Err(e) => Err(e),
        }
    }

    pub fn dispose(&mut self, line: BufferLine) {
        self.pool.push(line);
    }
}

pub struct BufferLine {
    text: String,
    char_count: usize,
}

impl BufferLine {
    fn new() -> Self {
        Self {
            text: String::new(),
            char_count: 0,
        }
    }

    pub fn char_count(&self) -> usize {
        self.char_count
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn chars_from<'a>(
        &'a self,
        index: usize,
    ) -> (
        impl 'a + Iterator<Item = (usize, char)>,
        impl 'a + Iterator<Item = (usize, char)>,
    ) {
        let (left, right) = self.text.split_at(index);
        let left_chars = left.char_indices().rev();
        let right_chars = right.char_indices().map(move |(i, c)| (index + i, c));
        (left_chars, right_chars)
    }

    pub fn words_from<'a>(
        &'a self,
        index: usize,
    ) -> (
        WordRefWithIndex<'a>,
        impl Iterator<Item = WordRefWithIndex<'a>>,
        impl Iterator<Item = WordRefWithIndex<'a>>,
    ) {
        let mid_word = self.word_at(index);
        let mid_start_index = mid_word.index;
        let mid_end_index = mid_start_index + mid_word.text.len();

        let left = &self.text[..mid_start_index];
        let right = &self.text[mid_end_index..];

        let mut left_column_index = mid_start_index;
        let left_words = WordIter::new(left).rev().map(move |w| {
            left_column_index -= w.text.len();
            WordRefWithIndex {
                kind: w.kind,
                text: w.text,
                index: left_column_index,
            }
        });

        let mut right_column_index = mid_end_index;
        let right_words = WordIter::new(right).map(move |w| {
            let index = right_column_index;
            right_column_index += w.text.len();
            WordRefWithIndex {
                kind: w.kind,
                text: w.text,
                index,
            }
        });

        (mid_word, left_words, right_words)
    }

    pub fn word_at(&self, index: usize) -> WordRefWithIndex {
        let (before, after) = self.text.split_at(index);
        match WordIter::new(after).next() {
            Some(right) => match WordIter::new(before).next_back() {
                Some(left) => {
                    if left.kind == right.kind {
                        let end_index = index + right.text.len();
                        let index = index - left.text.len();
                        WordRefWithIndex {
                            kind: left.kind,
                            text: &self.text[index..end_index],
                            index,
                        }
                    } else {
                        WordRefWithIndex {
                            kind: right.kind,
                            text: right.text,
                            index,
                        }
                    }
                }
                None => WordRefWithIndex {
                    kind: right.kind,
                    text: right.text,
                    index,
                },
            },
            None => WordRefWithIndex {
                kind: WordKind::Whitespace,
                text: "",
                index,
            },
        }
    }

    pub fn split_off(&mut self, pool: &mut BufferLinePool, index: usize) -> BufferLine {
        let mut new_line = pool.rent();
        new_line.push_text(&self.text[index..]);

        self.text.truncate(index);
        self.char_count -= new_line.char_count();

        new_line
    }

    pub fn insert_text(&mut self, index: usize, text: &str) {
        self.text.insert_str(index, text);
        self.char_count += text.chars().count();
    }

    pub fn push_text(&mut self, text: &str) {
        self.text.push_str(text);
        self.char_count += text.chars().count();
    }

    pub fn delete_range<R>(&mut self, range: R)
    where
        R: RangeBounds<usize>,
    {
        self.char_count -= self.text.drain(range).count();
    }
}

pub struct BufferContent {
    lines: Vec<BufferLine>,
}

impl BufferContent {
    pub fn empty() -> &'static Self {
        static EMPTY: BufferContent = BufferContent { lines: Vec::new() };
        &EMPTY
    }

    pub fn new() -> Self {
        Self {
            lines: vec![BufferLine::new()],
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn lines(&self) -> impl Iterator<Item = &BufferLine> {
        self.lines.iter()
    }

    pub fn line_at(&self, index: usize) -> &BufferLine {
        &self.lines[index]
    }

    pub fn read<R>(&mut self, pool: &mut BufferLinePool, read: &mut R) -> io::Result<()>
    where
        R: io::BufRead,
    {
        for line in self.lines.drain(..) {
            pool.dispose(line);
        }
        while let Some(line) = pool.read(read)? {
            self.lines.push(line);
        }
        if self.lines.is_empty() {
            self.lines.push(pool.rent());
        }
        Ok(())
    }

    pub fn write<W>(&self, write: &mut W) -> io::Result<()>
    where
        W: io::Write,
    {
        let end_index = self.lines.len() - 1;
        for line in &self.lines[..end_index] {
            writeln!(write, "{}", line.as_str())?;
        }
        write!(write, "{}", self.lines[end_index].as_str())?;
        Ok(())
    }

    pub fn saturate_position(&self, mut position: BufferPosition) -> BufferPosition {
        position.line_index = position.line_index.min(self.line_count() - 1);
        let line = self.line_at(position.line_index).as_str();
        position.column_byte_index = line.len().min(position.column_byte_index);
        while !line.is_char_boundary(position.column_byte_index) {
            position.column_byte_index += 1;
        }
        position
    }

    pub fn append_range_text_to_string(&self, range: BufferRange, text: &mut String) {
        let from = self.clamp_position(range.from);
        let to = self.clamp_position(range.to);

        let first_line = self.lines[from.line_index].as_str();
        if from.line_index == to.line_index {
            let range_text = &first_line[from.column_byte_index..to.column_byte_index];
            text.push_str(range_text);
        } else {
            text.push_str(&first_line[from.column_byte_index..]);
            let lines_range = (from.line_index + 1)..to.line_index;
            if lines_range.start < lines_range.end {
                for line in &self.lines[lines_range] {
                    text.push('\n');
                    text.push_str(line.as_str());
                }
            }

            let to_line = &self.lines[to.line_index];
            text.push('\n');
            text.push_str(&to_line.as_str()[..to.column_byte_index]);
        }
    }

    pub fn find_search_ranges(&self, text: &str, ranges: &mut Vec<BufferRange>) {
        if text.is_empty() {
            return;
        }

        if text.as_bytes().iter().any(|c| c.is_ascii_uppercase()) {
            for (i, line) in self.lines.iter().enumerate() {
                for (j, _) in line.as_str().match_indices(text) {
                    ranges.push(BufferRange::between(
                        BufferPosition::line_col(i, j),
                        BufferPosition::line_col(i, j + text.len()),
                    ));
                }
            }
        } else {
            let bytes = text.as_bytes();
            let bytes_len = bytes.len();

            for (i, line) in self.lines.iter().enumerate() {
                let mut column_index = 0;
                let mut line = line.as_str().as_bytes();
                while line.len() >= bytes_len {
                    if line
                        .iter()
                        .zip(bytes.iter())
                        .all(|(a, b)| a.eq_ignore_ascii_case(b))
                    {
                        let from = BufferPosition::line_col(i, column_index);
                        column_index += bytes_len;
                        let to = BufferPosition::line_col(i, column_index);
                        ranges.push(BufferRange::between(from, to));
                        line = &line[bytes_len..];
                    } else {
                        column_index += 1;
                        line = &line[1..];
                    }
                }
            }
        }
    }

    fn clamp_position(&self, mut position: BufferPosition) -> BufferPosition {
        position.line_index = position.line_index.min(self.line_count() - 1);
        position.column_byte_index = position
            .column_byte_index
            .min(self.lines[position.line_index].as_str().len());

        position
    }

    pub fn insert_text(
        &mut self,
        pool: &mut BufferLinePool,
        position: BufferPosition,
        text: &str,
    ) -> BufferRange {
        let position = self.clamp_position(position);

        if let None = text.find('\n') {
            let line = &mut self.lines[position.line_index];
            let previous_len = line.as_str().len();
            line.insert_text(position.column_byte_index, text);
            let len_diff = line.as_str().len() - previous_len;

            let end_position = BufferPosition::line_col(
                position.line_index,
                position.column_byte_index + len_diff,
            );
            BufferRange::between(position, end_position)
        } else {
            let split_line =
                self.lines[position.line_index].split_off(pool, position.column_byte_index);

            let mut line_count = 0;
            let mut lines = text.lines();
            if let Some(line) = lines.next() {
                self.lines[position.line_index].push_text(&line);
            }
            for line_text in lines {
                line_count += 1;

                let mut line = pool.rent();
                line.push_text(line_text);
                self.lines.insert(position.line_index + line_count, line);
            }

            let end_position = if text.ends_with('\n') {
                line_count += 1;
                self.lines
                    .insert(position.line_index + line_count, split_line);

                BufferPosition::line_col(position.line_index + line_count, 0)
            } else {
                let line = &mut self.lines[position.line_index + line_count];
                let column_byte_index = line.as_str().len();
                line.push_text(split_line.as_str());

                BufferPosition::line_col(position.line_index + line_count, column_byte_index)
            };

            BufferRange::between(position, end_position)
        }
    }

    pub fn delete_range(&mut self, pool: &mut BufferLinePool, range: BufferRange) -> Text {
        let from = self.clamp_position(range.from);
        let to = self.clamp_position(range.to);

        if from.line_index == to.line_index {
            let line = &mut self.lines[from.line_index];
            let range = from.column_byte_index..to.column_byte_index;
            let deleted_text = &line.as_str()[range.clone()];
            let text = Text::from(deleted_text);
            line.delete_range(range);

            text
        } else {
            let mut deleted_text = Text::new();

            let line = &mut self.lines[from.line_index];
            let delete_range = from.column_byte_index..;
            deleted_text.push_str(&line.as_str()[delete_range.clone()]);
            line.delete_range(delete_range);
            drop(line);

            let lines_range = (from.line_index + 1)..to.line_index;
            if lines_range.start < lines_range.end {
                for line in self.lines.drain(lines_range) {
                    deleted_text.push_str("\n");
                    deleted_text.push_str(line.as_str());
                    pool.dispose(line);
                }
            }
            let to_line_index = from.line_index + 1;
            if to_line_index < self.lines.len() {
                let to_line = self.lines.remove(to_line_index);
                self.lines[from.line_index].push_text(&to_line.as_str()[to.column_byte_index..]);
                deleted_text.push_str("\n");
                deleted_text.push_str(&to_line.as_str()[..to.column_byte_index]);
            }

            deleted_text
        }
    }

    pub fn words_from<'a>(
        &'a self,
        position: BufferPosition,
    ) -> (
        WordRefWithPosition<'a>,
        impl Iterator<Item = WordRefWithPosition<'a>>,
        impl Iterator<Item = WordRefWithPosition<'a>>,
    ) {
        let BufferPosition {
            line_index,
            column_byte_index,
        } = self.clamp_position(position);

        let (mid_word, left_words, right_words) =
            self.line_at(line_index).words_from(column_byte_index);

        (
            mid_word.to_word_ref_with_position(line_index),
            left_words.map(move |w| w.to_word_ref_with_position(line_index)),
            right_words.map(move |w| w.to_word_ref_with_position(line_index)),
        )
    }

    pub fn word_at(&self, position: BufferPosition) -> WordRefWithPosition {
        let position = self.clamp_position(position);
        self.line_at(position.line_index)
            .word_at(position.column_byte_index)
            .to_word_ref_with_position(position.line_index)
    }

    pub fn find_delimiter_pair_at(
        &self,
        position: BufferPosition,
        delimiter: char,
    ) -> Option<BufferRange> {
        let position = self.clamp_position(position);
        let line = self.line_at(position.line_index).as_str();

        let mut is_right_delim = false;
        let mut last_i = 0;
        for (i, c) in line.char_indices() {
            if c != delimiter {
                continue;
            }

            if i >= position.column_byte_index {
                if is_right_delim {
                    return Some(BufferRange::between(
                        BufferPosition::line_col(
                            position.line_index,
                            last_i + delimiter.len_utf8(),
                        ),
                        BufferPosition::line_col(position.line_index, i),
                    ));
                }

                if i != position.column_byte_index {
                    break;
                }
            }

            is_right_delim = !is_right_delim;
            last_i = i;
        }

        None
    }

    pub fn find_balanced_chars_at(
        &self,
        position: BufferPosition,
        left: char,
        right: char,
    ) -> Option<BufferRange> {
        fn find<I>(iter: I, target: char, other: char, balance: &mut usize) -> Option<usize>
        where
            I: Iterator<Item = (usize, char)>,
        {
            let mut b = *balance;
            for (i, c) in iter {
                if c == target {
                    if b == 0 {
                        *balance = 0;
                        return Some(i);
                    } else {
                        b -= 1;
                    }
                } else if c == other {
                    b += 1;
                }
            }
            *balance = b;
            None
        }

        let position = self.clamp_position(position);
        let line = self.line_at(position.line_index).as_str();
        let (before, after) = line.split_at(position.column_byte_index);

        let mut balance = 0;

        let mut left_position = None;
        let mut right_position = None;

        let mut after_chars = after.char_indices();
        if let Some((i, c)) = after_chars.next() {
            if c == left {
                left_position = Some(position.column_byte_index + i + c.len_utf8());
            } else if c == right {
                right_position = Some(position.column_byte_index + i);
            }
        }

        let right_position = match right_position {
            Some(column_index) => BufferPosition::line_col(position.line_index, column_index),
            None => match find(after_chars, right, left, &mut balance) {
                Some(column_byte_index) => {
                    let column_byte_index = position.column_byte_index + column_byte_index;
                    BufferPosition::line_col(position.line_index, column_byte_index)
                }
                None => {
                    let mut pos = None;
                    for line_index in (position.line_index + 1)..self.line_count() {
                        let line = self.line_at(line_index).as_str();
                        if let Some(column_byte_index) =
                            find(line.char_indices(), right, left, &mut balance)
                        {
                            pos = Some(BufferPosition::line_col(line_index, column_byte_index));
                            break;
                        }
                    }
                    pos?
                }
            },
        };

        balance = 0;

        let left_position = match left_position {
            Some(column_index) => BufferPosition::line_col(position.line_index, column_index),
            None => match find(before.char_indices().rev(), left, right, &mut balance) {
                Some(column_byte_index) => {
                    let column_byte_index = column_byte_index + left.len_utf8();
                    BufferPosition::line_col(position.line_index, column_byte_index)
                }
                None => {
                    let mut pos = None;
                    for line_index in (0..position.line_index).rev() {
                        let line = self.line_at(line_index).as_str();
                        if let Some(column_byte_index) =
                            find(line.char_indices().rev(), left, right, &mut balance)
                        {
                            let column_byte_index = column_byte_index + left.len_utf8();
                            pos = Some(BufferPosition::line_col(line_index, column_byte_index));
                            break;
                        }
                    }
                    pos?
                }
            },
        };

        Some(BufferRange::between(left_position, right_position))
    }
}

impl fmt::Display for BufferContent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let end_index = self.lines.len() - 1;
        for line in &self.lines[..end_index] {
            f.write_str(line.as_str())?;
            f.write_str("\n")?;
        }
        f.write_str(self.lines[end_index].as_str())
    }
}

#[derive(Default)]
pub struct BufferCapabilities {
    has_history: bool,
    can_save: bool,
}
impl BufferCapabilities {
    pub fn text(&mut self) {
        self.has_history = true;
        self.can_save = true;
    }

    pub fn log(&mut self) {
        self.has_history = false;
        self.can_save = false;
    }
}

pub struct Buffer {
    alive: bool,
    path: PathBuf,
    content: BufferContent,
    line_pool: BufferLinePool,
    syntax_handle: SyntaxHandle,
    highlighted: HighlightedBuffer,
    history: History,
    search_ranges: Vec<BufferRange>,
    needs_save: bool,
    capabilities: BufferCapabilities,
}

impl Buffer {
    fn new() -> Self {
        Self {
            alive: true,
            path: PathBuf::new(),
            content: BufferContent::new(),
            line_pool: BufferLinePool::default(),
            syntax_handle: SyntaxHandle::default(),
            highlighted: HighlightedBuffer::new(),
            history: History::new(),
            search_ranges: Vec::new(),
            needs_save: false,
            capabilities: BufferCapabilities::default(),
        }
    }

    fn dispose(&mut self, word_database: &mut WordDatabase) {
        for line in self.content.lines.drain(..) {
            for word in WordIter::new(line.as_str()).of_kind(WordKind::Identifier) {
                word_database.remove_word(word);
            }

            self.line_pool.dispose(line);
        }
        self.content.lines.push(self.line_pool.rent());

        self.alive = false;
        self.path.clear();
        self.syntax_handle = SyntaxHandle::default();
        self.highlighted.clear();
        self.history.clear();
        self.search_ranges.clear();
        self.needs_save = false;
        self.capabilities = BufferCapabilities::default();
    }

    pub fn path(&self) -> Option<&Path> {
        if self.path.as_os_str().is_empty() {
            None
        } else {
            Some(&self.path)
        }
    }

    pub fn set_path(&mut self, syntaxes: &SyntaxCollection, path: Option<&Path>) {
        self.path.clear();
        if let Some(path) = path {
            self.path.push(path);
        }
        self.refresh_syntax(syntaxes);
    }

    pub fn capabilities(&mut self) -> &mut BufferCapabilities {
        &mut self.capabilities
    }

    pub fn refresh_syntax(&mut self, syntaxes: &SyntaxCollection) {
        let path = self.path.to_str().unwrap_or("").as_bytes();
        if path.is_empty() {
            return;
        }

        let syntax_handle = syntaxes
            .find_handle_by_path(path)
            .unwrap_or(SyntaxHandle::default());

        if self.syntax_handle != syntax_handle {
            self.syntax_handle = syntax_handle;
            self.highlighted
                .highligh_all(syntaxes.get(self.syntax_handle), &self.content);
        }
    }

    pub fn content(&self) -> &BufferContent {
        &self.content
    }

    pub fn highlighted(&self) -> &HighlightedBuffer {
        &self.highlighted
    }

    pub fn needs_save(&self) -> bool {
        self.capabilities.can_save && self.needs_save
    }

    pub fn insert_text(
        &mut self,
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        position: BufferPosition,
        text: &str,
    ) -> BufferRange {
        self.search_ranges.clear();
        if text.is_empty() {
            return BufferRange::between(position, position);
        }
        self.needs_save = true;

        for word in WordIter::new(self.content.line_at(position.line_index).as_str())
            .of_kind(WordKind::Identifier)
        {
            word_database.remove_word(word);
        }

        let range = self
            .content
            .insert_text(&mut self.line_pool, position, text);

        let line_count = range.to.line_index - range.from.line_index + 1;
        for line in self
            .content
            .lines()
            .skip(range.from.line_index)
            .take(line_count)
        {
            for word in WordIter::new(line.as_str()).of_kind(WordKind::Identifier) {
                word_database.add_word(word);
            }
        }

        self.highlighted
            .on_insert(syntaxes.get(self.syntax_handle), &self.content, range);

        if self.capabilities.has_history {
            self.history.add_edit(Edit {
                kind: EditKind::Insert,
                range,
                text,
            });
        }

        range
    }

    pub fn delete_range(
        &mut self,
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        range: BufferRange,
    ) {
        self.search_ranges.clear();
        if range.from == range.to {
            return;
        }
        self.needs_save = true;

        let line_count = range.to.line_index - range.from.line_index + 1;
        for line in self
            .content
            .lines()
            .skip(range.from.line_index)
            .take(line_count)
        {
            for word in WordIter::new(line.as_str()).of_kind(WordKind::Identifier) {
                word_database.remove_word(word);
            }
        }

        let deleted_text = self.content.delete_range(&mut self.line_pool, range);

        for word in WordIter::new(self.content.line_at(range.from.line_index).as_str())
            .of_kind(WordKind::Identifier)
        {
            word_database.add_word(word);
        }

        self.highlighted
            .on_delete(syntaxes.get(self.syntax_handle), &self.content, range);

        if self.capabilities.has_history {
            self.history.add_edit(Edit {
                kind: EditKind::Delete,
                range,
                text: deleted_text.as_str(),
            });
        }
    }

    pub fn commit_edits(&mut self) {
        self.history.commit_edits();
    }

    pub fn undo<'a>(
        &'a mut self,
        syntaxes: &'a SyntaxCollection,
    ) -> impl 'a + Iterator<Item = Edit<'a>> {
        self.history_edits(syntaxes, |h| h.undo_edits())
    }

    pub fn redo<'a>(
        &'a mut self,
        syntaxes: &SyntaxCollection,
    ) -> impl 'a + Iterator<Item = Edit<'a>> {
        self.history_edits(syntaxes, |h| h.redo_edits())
    }

    fn history_edits<'a, F, I>(&'a mut self, syntaxes: &SyntaxCollection, selector: F) -> I
    where
        F: FnOnce(&'a mut History) -> I,
        I: 'a + Clone + Iterator<Item = Edit<'a>>,
    {
        self.search_ranges.clear();
        self.needs_save = true;

        let syntax = syntaxes.get(self.syntax_handle);
        let edits = selector(&mut self.history);

        for edit in edits.clone() {
            match edit.kind {
                EditKind::Insert => {
                    let range =
                        self.content
                            .insert_text(&mut self.line_pool, edit.range.from, edit.text);
                    self.highlighted.on_insert(syntax, &self.content, range);
                }
                EditKind::Delete => {
                    self.content.delete_range(&mut self.line_pool, edit.range);
                    self.highlighted
                        .on_delete(syntax, &self.content, edit.range);
                }
            }
        }

        edits
    }

    pub fn current_edit_handle(&self) -> EditHandle {
        self.history.current_edit_handle()
    }

    pub fn edits_since<'a>(&'a self, handle: EditHandle) -> impl 'a + Iterator<Item = Edit<'a>> {
        self.history.edits_since(handle)
    }

    pub fn set_search(&mut self, text: &str) {
        self.search_ranges.clear();
        self.content
            .find_search_ranges(text, &mut self.search_ranges);
    }

    pub fn set_search_with<F>(&mut self, selector: F) -> &str
    where
        F: FnOnce(&BufferContent) -> &str,
    {
        self.search_ranges.clear();
        let text = selector(&self.content);
        self.content
            .find_search_ranges(text, &mut self.search_ranges);
        text
    }

    pub fn search_ranges(&self) -> &[BufferRange] {
        &self.search_ranges
    }

    pub fn discard_and_reload_from_file(
        &mut self,
        syntaxes: &SyntaxCollection,
    ) -> Result<(), String> {
        if !self.capabilities.can_save {
            return Ok(());
        }

        let path = self.path.as_path();
        if path.as_os_str().is_empty() {
            return Err("buffer has no path".into());
        }

        let file =
            File::open(path).map_err(|e| format!("could not open file {:?}: {:?}", path, e))?;
        let mut reader = io::BufReader::new(file);

        self.content
            .read(&mut self.line_pool, &mut reader)
            .map_err(|e| format!("could not read file {:?}: {:?}", path, e))?;

        self.highlighted
            .highligh_all(syntaxes.get(self.syntax_handle), &self.content);

        self.history.clear();
        self.search_ranges.clear();

        self.needs_save = false;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BufferHandle(usize);
impl_from_script!(BufferHandle, value => match value {
    ScriptValue::Integer(n) if n >= 0 => Some(Self(n as _)),
    _ => None,
});
impl_to_script!(BufferHandle, (self, _engine) => ScriptValue::Integer(self.0 as _));

#[derive(Default)]
pub struct BufferCollection {
    buffers: Vec<Buffer>,
}

impl BufferCollection {
    pub fn new(&mut self, events: &mut EditorEventQueue) -> (BufferHandle, &mut Buffer) {
        let mut handle = None;
        for (i, buffer) in self.buffers.iter_mut().enumerate() {
            if !buffer.alive {
                handle = Some(BufferHandle(i));
                break;
            }
        }
        let handle = match handle {
            Some(handle) => handle,
            None => {
                let handle = BufferHandle(self.buffers.len());
                self.buffers.push(Buffer::new());
                handle
            }
        };

        events.enqueue(EditorEvent::BufferLoad { handle });
        let buffer = &mut self.buffers[handle.0];
        (handle, buffer)
    }

    pub fn get(&self, handle: BufferHandle) -> Option<&Buffer> {
        let buffer = &self.buffers[handle.0];
        if buffer.alive {
            Some(buffer)
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, handle: BufferHandle) -> Option<&mut Buffer> {
        let buffer = &mut self.buffers[handle.0];
        if buffer.alive {
            Some(buffer)
        } else {
            None
        }
    }

    pub fn find_with_path(&self, root: &Path, path: &Path) -> Option<BufferHandle> {
        if path.as_os_str().len() == 0 {
            return None;
        }

        let path = path.strip_prefix(root).unwrap_or(path);

        for (handle, buffer) in self.iter_with_handles() {
            let buffer_path = buffer.path.as_path();
            let buffer_path = buffer_path.strip_prefix(root).unwrap_or(buffer_path);

            if buffer_path == path {
                return Some(handle);
            }
        }

        None
    }

    pub fn iter(&self) -> impl Iterator<Item = &Buffer> {
        self.buffers
            .iter()
            .filter_map(|b| if b.alive { Some(b) } else { None })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Buffer> {
        self.buffers
            .iter_mut()
            .filter_map(|b| if b.alive { Some(b) } else { None })
    }

    pub fn iter_with_handles(&self) -> impl Iterator<Item = (BufferHandle, &Buffer)> {
        self.buffers.iter().enumerate().filter_map(|(i, b)| {
            if b.alive {
                Some((BufferHandle(i), b))
            } else {
                None
            }
        })
    }

    pub fn save_to_file(
        &mut self,
        handle: BufferHandle,
        path: Option<&Path>,
        events: &mut EditorEventQueue,
    ) -> Result<(), String> {
        match self.get_mut(handle) {
            Some(buffer) => {
                let new_path = match path {
                    Some(path) => {
                        buffer.path.clear();
                        buffer.path.push(path);
                        true
                    }
                    None => false,
                };

                if !buffer.capabilities.can_save {
                    return Ok(());
                }

                match buffer.path() {
                    Some(path) => {
                        let file = File::create(path)
                            .map_err(|e| format!("could not create file {:?}: {:?}", path, e))?;
                        let mut writer = io::BufWriter::new(file);

                        buffer
                            .content
                            .write(&mut writer)
                            .map_err(|e| format!("could not write to file {:?}: {:?}", path, e))?;
                        buffer.needs_save = false;

                        events.enqueue(EditorEvent::BufferSave { handle, new_path });
                        Ok(())
                    }
                    None => Err("buffer has no path".into()),
                }
            }
            None => Ok(()),
        }
    }

    pub fn save_all_to_file(&mut self, events: &mut EditorEventQueue) -> Result<(), String> {
        let buffer_count = self.buffers.len();
        for i in 0..buffer_count {
            self.save_to_file(BufferHandle(i), None, events)?;
        }
        Ok(())
    }

    pub fn defer_remove_where<F>(&mut self, events: &mut EditorEventQueue, predicate: F)
    where
        F: Fn(BufferHandle, &Buffer) -> bool,
    {
        for i in 0..self.buffers.len() {
            let buffer = &mut self.buffers[i];
            if !buffer.alive {
                continue;
            }

            let handle = BufferHandle(i);
            if !predicate(handle, buffer) {
                continue;
            }

            events.enqueue(EditorEvent::BufferClose { handle });
        }
    }

    pub fn remove(
        &mut self,
        handle: BufferHandle,
        clients: &mut ClientCollection,
        word_database: &mut WordDatabase,
    ) {
        let buffer = &mut self.buffers[handle.0];
        if !buffer.alive {
            return;
        }

        for client in clients.iter_mut() {
            client
                .navigation_history
                .remove_snapshots_with_buffer_handle(handle);
        }

        buffer.dispose(word_database);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer_position::BufferPosition;

    fn buffer_from_str(line_pool: &mut BufferLinePool, text: &str) -> BufferContent {
        let mut buffer = BufferContent::new();
        buffer.insert_text(line_pool, BufferPosition::line_col(0, 0), text);
        buffer
    }

    #[test]
    fn text_size() {
        assert_eq!(32, std::mem::size_of::<Text>());
    }

    #[test]
    fn text_grow() {
        const S1: &str = "123456789012345678901234567890";
        const S2: &str = "abc";

        let mut text = Text::new();
        text.push_str(S1);
        assert_eq!(S1, text.as_str());
        text.push_str(S2);

        let mut s = String::new();
        s.push_str(S1);
        s.push_str(S2);
        assert_eq!(s, text.as_str());
    }

    #[test]
    fn buffer_line_char_count() {
        let mut line_pool = BufferLinePool::default();
        let mut line = line_pool.rent();
        line.push_text("abc");
        assert_eq!(3, line.char_count());
        line.insert_text(1, "def");
        assert_eq!(6, line.char_count());
        line.delete_range(1..3);
        assert_eq!(4, line.char_count());
        line.push_text("ghi");
        assert_eq!(7, line.char_count());
    }

    #[test]
    fn buffer_utf8_support() {
        let mut line_pool = BufferLinePool::default();
        let mut buffer = buffer_from_str(&mut line_pool, "abd");
        let range = buffer.insert_text(&mut line_pool, BufferPosition::line_col(0, 2), "ç");
        assert_eq!(
            BufferRange::between(
                BufferPosition::line_col(0, 2),
                BufferPosition::line_col(0, 2 + 'ç'.len_utf8())
            ),
            range
        );
    }

    #[test]
    fn buffer_content_insert_text() {
        let mut line_pool = BufferLinePool::default();
        let mut buffer = BufferContent::new();

        assert_eq!(1, buffer.line_count());
        assert_eq!("", buffer.to_string());

        buffer.insert_text(&mut line_pool, BufferPosition::line_col(0, 0), "hold");
        buffer.insert_text(&mut line_pool, BufferPosition::line_col(0, 2), "r");
        buffer.insert_text(&mut line_pool, BufferPosition::line_col(0, 1), "ello w");
        assert_eq!(1, buffer.line_count());
        assert_eq!("hello world", buffer.to_string());

        buffer.insert_text(&mut line_pool, BufferPosition::line_col(0, 5), "\n");
        buffer.insert_text(
            &mut line_pool,
            BufferPosition::line_col(1, 6),
            " appending more\nand more\nand even more\nlines",
        );
        assert_eq!(5, buffer.line_count());
        assert_eq!(
            "hello\n world appending more\nand more\nand even more\nlines",
            buffer.to_string()
        );

        let mut buffer = buffer_from_str(&mut line_pool, "this is content");
        buffer.insert_text(
            &mut line_pool,
            BufferPosition::line_col(0, 8),
            "some\nmultiline ",
        );
        assert_eq!(2, buffer.line_count());
        assert_eq!("this is some\nmultiline content", buffer.to_string());

        let mut buffer = buffer_from_str(&mut line_pool, "this is content");
        buffer.insert_text(
            &mut line_pool,
            BufferPosition::line_col(0, 8),
            "some\nmore\nextensive\nmultiline ",
        );
        assert_eq!(4, buffer.line_count());
        assert_eq!(
            "this is some\nmore\nextensive\nmultiline content",
            buffer.to_string()
        );
    }

    #[test]
    fn buffer_content_delete_range() {
        let mut line_pool = BufferLinePool::default();
        let mut buffer = buffer_from_str(&mut line_pool, "abc");
        buffer.delete_range(
            &mut line_pool,
            BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(0, 1),
            ),
        );
        assert_eq!("abc", buffer.to_string());
        buffer.delete_range(
            &mut line_pool,
            BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(0, 2),
            ),
        );
        assert_eq!("ac", buffer.to_string());

        let mut buffer =
            buffer_from_str(&mut line_pool, "this is the initial\ncontent of the buffer");

        assert_eq!(2, buffer.line_count());
        assert_eq!(
            "this is the initial\ncontent of the buffer",
            buffer.to_string()
        );

        let deleted_text = buffer.delete_range(
            &mut line_pool,
            BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(0, 0),
            ),
        );
        assert_eq!(2, buffer.line_count());
        assert_eq!(
            "this is the initial\ncontent of the buffer",
            buffer.to_string()
        );
        assert_eq!("", deleted_text.as_str());

        let deleted_text = buffer.delete_range(
            &mut line_pool,
            BufferRange::between(
                BufferPosition::line_col(0, 11),
                BufferPosition::line_col(0, 19),
            ),
        );
        assert_eq!(2, buffer.line_count());
        assert_eq!("this is the\ncontent of the buffer", buffer.to_string());
        assert_eq!(" initial", deleted_text.as_str());

        let deleted_text = buffer.delete_range(
            &mut line_pool,
            BufferRange::between(
                BufferPosition::line_col(0, 8),
                BufferPosition::line_col(1, 15),
            ),
        );
        assert_eq!(1, buffer.line_count());
        assert_eq!("this is buffer", buffer.to_string());
        assert_eq!("the\ncontent of the ", deleted_text.as_str());

        let mut buffer = buffer_from_str(
            &mut line_pool,
            "this\nbuffer\ncontains\nmultiple\nlines\nyes",
        );
        assert_eq!(6, buffer.line_count());
        let deleted_text = buffer.delete_range(
            &mut line_pool,
            BufferRange::between(
                BufferPosition::line_col(1, 4),
                BufferPosition::line_col(4, 1),
            ),
        );
        assert_eq!("this\nbuffines\nyes", buffer.to_string());
        assert_eq!("er\ncontains\nmultiple\nl", deleted_text.as_str());
    }

    #[test]
    fn buffer_content_delete_lines() {
        let mut pool = BufferLinePool::default();
        let mut buffer = buffer_from_str(&mut pool, "first line\nsecond line\nthird line");
        assert_eq!(3, buffer.line_count());
        let deleted_text = buffer.delete_range(
            &mut pool,
            BufferRange::between(
                BufferPosition::line_col(1, 0),
                BufferPosition::line_col(2, 0),
            ),
        );
        assert_eq!("first line\nthird line", buffer.to_string());
        assert_eq!("second line\n", deleted_text.as_str());

        let mut buffer = buffer_from_str(&mut pool, "first line\nsecond line\nthird line");
        assert_eq!(3, buffer.line_count());
        let deleted_text = buffer.delete_range(
            &mut pool,
            BufferRange::between(
                BufferPosition::line_col(1, 0),
                BufferPosition::line_col(1, 11),
            ),
        );
        assert_eq!("first line\n\nthird line", buffer.to_string());
        assert_eq!("second line", deleted_text.as_str());
    }

    #[test]
    fn buffer_delete_undo_redo_single_line() {
        let mut word_database = WordDatabase::new();
        let syntaxes = SyntaxCollection::new();

        let mut buffer = Buffer::new();
        buffer.capabilities.text();
        buffer.insert_text(
            &mut word_database,
            &syntaxes,
            BufferPosition::line_col(0, 0),
            "single line content",
        );
        let range = BufferRange::between(
            BufferPosition::line_col(0, 7),
            BufferPosition::line_col(0, 12),
        );
        buffer.delete_range(&mut word_database, &syntaxes, range);

        assert_eq!("single content", buffer.content.to_string());
        {
            let mut ranges = buffer.undo(&syntaxes);
            assert_eq!(range, ranges.next().unwrap().range);
            ranges.next().unwrap();
            assert!(ranges.next().is_none());
        }
        assert!(buffer.content.to_string().is_empty());
        let mut redo_iter = buffer.redo(&syntaxes);
        redo_iter.next().unwrap();
        redo_iter.next().unwrap();
        assert!(redo_iter.next().is_none());
        drop(redo_iter);
        assert_eq!("single content", buffer.content.to_string());
    }

    #[test]
    fn buffer_delete_undo_redo_multi_line() {
        let mut word_database = WordDatabase::new();
        let syntaxes = SyntaxCollection::new();

        let mut buffer = Buffer::new();
        buffer.capabilities.text();
        buffer.insert_text(
            &mut word_database,
            &syntaxes,
            BufferPosition::line_col(0, 0),
            "multi\nline\ncontent",
        );
        let range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(1, 3),
        );
        buffer.delete_range(&mut word_database, &syntaxes, range);

        assert_eq!("me\ncontent", buffer.content.to_string());
        {
            let mut ranges = buffer.undo(&syntaxes);
            assert_eq!(range, ranges.next().unwrap().range);
            ranges.next().unwrap();
            assert!(ranges.next().is_none());
        }
        assert!(buffer.content.to_string().is_empty());
        let mut redo_iter = buffer.redo(&syntaxes);
        redo_iter.next().unwrap();
        redo_iter.next().unwrap();
        assert!(redo_iter.next().is_none());
        drop(redo_iter);
        assert_eq!("me\ncontent", buffer.content.to_string());
    }

    #[test]
    fn buffer_content_range_text() {
        let mut pool = BufferLinePool::default();
        let buffer = buffer_from_str(&mut pool, "abc\ndef\nghi");
        let mut text = String::new();
        buffer.append_range_text_to_string(
            BufferRange::between(
                BufferPosition::line_col(0, 2),
                BufferPosition::line_col(2, 1),
            ),
            &mut text,
        );
        assert_eq!("c\ndef\ng", &text);
    }

    #[test]
    fn buffer_content_word_at() {
        macro_rules! assert_word {
            ($word:expr, $pos:expr, $kind:expr, $text:expr) => {
                assert_eq!($pos, $word.position);
                assert_eq!($kind, $word.kind);
                assert_eq!($text, $word.text);
            };
        };
        fn col(column: usize) -> BufferPosition {
            BufferPosition::line_col(0, column)
        }

        let mut pool = BufferLinePool::default();
        let buffer = buffer_from_str(&mut pool, "word");
        assert_word!(buffer.word_at(col(0)), col(0), WordKind::Identifier, "word");
        assert_word!(buffer.word_at(col(2)), col(0), WordKind::Identifier, "word");
        assert_word!(buffer.word_at(col(4)), col(4), WordKind::Whitespace, "");

        let buffer = buffer_from_str(&mut pool, "asd word+? asd");
        assert_word!(buffer.word_at(col(3)), col(3), WordKind::Whitespace, " ");
        assert_word!(buffer.word_at(col(4)), col(4), WordKind::Identifier, "word");
        assert_word!(buffer.word_at(col(6)), col(4), WordKind::Identifier, "word");
        assert_word!(buffer.word_at(col(8)), col(8), WordKind::Symbol, "+?");
        assert_word!(buffer.word_at(col(9)), col(8), WordKind::Symbol, "+?");
        assert_word!(buffer.word_at(col(10)), col(10), WordKind::Whitespace, " ");
    }

    #[test]
    fn buffer_content_words_from() {
        macro_rules! assert_word {
            ($word:expr, $pos:expr, $kind:expr, $text:expr) => {
                let word = $word;
                assert_eq!($pos, word.position);
                assert_eq!($kind, word.kind);
                assert_eq!($text, word.text);
            };
        };
        fn col(column: usize) -> BufferPosition {
            BufferPosition::line_col(0, column)
        }

        let mut pool = BufferLinePool::default();
        let buffer = buffer_from_str(&mut pool, "word");
        let (w, mut lw, mut rw) = buffer.words_from(col(0));
        assert_word!(w, col(0), WordKind::Identifier, "word");
        assert!(lw.next().is_none());
        assert!(rw.next().is_none());
        let (w, mut lw, mut rw) = buffer.words_from(col(2));
        assert_word!(w, col(0), WordKind::Identifier, "word");
        assert!(lw.next().is_none());
        assert!(rw.next().is_none());
        let (w, mut lw, mut rw) = buffer.words_from(col(4));
        assert_word!(w, col(4), WordKind::Whitespace, "");
        assert_word!(lw.next().unwrap(), col(0), WordKind::Identifier, "word");
        assert!(lw.next().is_none());
        assert!(rw.next().is_none());

        let buffer = buffer_from_str(&mut pool, "first second third");
        let (w, mut lw, mut rw) = buffer.words_from(col(8));
        assert_word!(w, col(6), WordKind::Identifier, "second");
        assert_word!(lw.next().unwrap(), col(5), WordKind::Whitespace, " ");
        assert_word!(lw.next().unwrap(), col(0), WordKind::Identifier, "first");
        assert!(lw.next().is_none());
        assert_word!(rw.next().unwrap(), col(12), WordKind::Whitespace, " ");
        assert_word!(rw.next().unwrap(), col(13), WordKind::Identifier, "third");
        assert!(rw.next().is_none());
    }

    #[test]
    fn buffer_find_balanced_chars() {
        let mut pool = BufferLinePool::default();
        let buffer = buffer_from_str(&mut pool, "(\n(\na\n)\nbc)");

        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(4, 2)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(0, 0), '(', ')')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(1, 1),
                BufferPosition::line_col(3, 0)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(2, 0), '(', ')')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(4, 2)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(0, 1), '(', ')')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(4, 2)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(4, 0), '(', ')')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(4, 2)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(0, 0), '(', ')')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(4, 2)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(4, 2), '(', ')')
        );
    }

    #[test]
    fn buffer_find_delimiter_pairs() {
        let mut pool = BufferLinePool::default();
        let buffer = buffer_from_str(&mut pool, "|a|bcd|efg|");

        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(0, 2)
            )),
            buffer.find_delimiter_pair_at(BufferPosition::line_col(0, 0), '|')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(0, 2)
            )),
            buffer.find_delimiter_pair_at(BufferPosition::line_col(0, 2), '|')
        );
        assert_eq!(
            None,
            buffer.find_delimiter_pair_at(BufferPosition::line_col(0, 4), '|')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 7),
                BufferPosition::line_col(0, 10)
            )),
            buffer.find_delimiter_pair_at(BufferPosition::line_col(0, 6), '|')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 7),
                BufferPosition::line_col(0, 10)
            )),
            buffer.find_delimiter_pair_at(BufferPosition::line_col(0, 10), '|')
        );
        assert_eq!(
            None,
            buffer.find_delimiter_pair_at(BufferPosition::line_col(0, 11), '|')
        );
    }
}
