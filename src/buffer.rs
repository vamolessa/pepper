use std::{
    fmt,
    fs::File,
    io,
    ops::RangeBounds,
    path::{Path, PathBuf},
};

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    client::ClientManager,
    editor_event::{EditorEvent, EditorEventQueue},
    history::{Edit, EditKind, History},
    script::ScriptValue,
    syntax::{HighlightResult, HighlightedBuffer, SyntaxCollection, SyntaxHandle},
    word_database::{WordDatabase, WordIter, WordKind},
};

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

struct BufferLinePool {
    pool: Vec<BufferLine>,
}

impl BufferLinePool {
    pub const fn new() -> Self {
        Self { pool: Vec::new() }
    }

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

    pub fn split_off(&mut self, other: &mut BufferLine, index: usize) {
        other.text.clear();
        other.char_count = 0;
        other.push_text(&self.text[index..]);

        self.text.truncate(index);
        self.char_count -= other.char_count();
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
    line_pool: BufferLinePool,
}

impl BufferContent {
    pub fn empty() -> &'static Self {
        static EMPTY: BufferContent = BufferContent {
            lines: Vec::new(),
            line_pool: BufferLinePool::new(),
        };
        &EMPTY
    }

    pub fn new() -> Self {
        Self {
            lines: vec![BufferLine::new()],
            line_pool: BufferLinePool::new(),
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

    pub fn read<R>(&mut self, read: &mut R) -> io::Result<()>
    where
        R: io::BufRead,
    {
        for line in self.lines.drain(..) {
            self.line_pool.dispose(line);
        }

        loop {
            let mut line = self.line_pool.rent();
            match read.read_line(&mut line.text) {
                Ok(0) => {
                    self.line_pool.dispose(line);
                    break;
                }
                Ok(_) => {
                    if line.text.ends_with('\n') {
                        line.text.truncate(line.text.len() - 1);
                    }
                    if line.text.ends_with('\r') {
                        line.text.truncate(line.text.len() - 1);
                    }

                    line.char_count = line.text.chars().count();
                    self.lines.push(line);
                }
                Err(e) => return Err(e),
            }
        }

        if self.lines.is_empty() {
            self.lines.push(self.line_pool.rent());
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

    pub fn insert_text(&mut self, position: BufferPosition, text: &str) -> BufferRange {
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
            let mut split_line = self.line_pool.rent();
            self.lines[position.line_index].split_off(&mut split_line, position.column_byte_index);

            let mut line_count = 0;
            let mut lines = text.lines();
            if let Some(line) = lines.next() {
                self.lines[position.line_index].push_text(&line);
            }
            for line_text in lines {
                line_count += 1;

                let mut line = self.line_pool.rent();
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

    pub fn delete_range(&mut self, range: BufferRange) {
        let from = range.from;
        let to = range.to;

        if from.line_index == to.line_index {
            let line = &mut self.lines[from.line_index];
            line.delete_range(from.column_byte_index..to.column_byte_index);
        } else {
            self.lines[from.line_index].delete_range(from.column_byte_index..);
            let lines_range = (from.line_index + 1)..to.line_index;
            if lines_range.start < lines_range.end {
                for line in self.lines.drain(lines_range) {
                    self.line_pool.dispose(line);
                }
            }
            let to_line_index = from.line_index + 1;
            if to_line_index < self.lines.len() {
                let to_line = self.lines.remove(to_line_index);
                self.lines[from.line_index].push_text(&to_line.as_str()[to.column_byte_index..]);
            }
        }
    }

    pub fn clear(&mut self) {
        for line in self.lines.drain(..) {
            self.line_pool.dispose(line);
        }
        self.lines.push(self.line_pool.rent());
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
    pub has_history: bool,
    pub can_save: bool,
    pub uses_word_database: bool,
}
impl BufferCapabilities {
    pub fn text() -> Self {
        Self {
            has_history: true,
            can_save: true,
            uses_word_database: true,
        }
    }

    pub fn log() -> Self {
        Self {
            has_history: false,
            can_save: false,
            uses_word_database: false,
        }
    }
}

pub enum BufferError {
    BufferDoesNotHavePath,
    CouldNotOpenFile,
    CouldNotReadFile,
    CouldNotCreateFile,
    CouldNotWriteFile,
}
impl BufferError {
    pub fn display(self, buffer: &Buffer) -> BufferErrorDisplay {
        BufferErrorDisplay {
            error: self,
            buffer,
        }
    }
}
pub struct BufferErrorDisplay<'a> {
    error: BufferError,
    buffer: &'a Buffer,
}
impl<'a> fmt::Display for BufferErrorDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let path = self.buffer.path.as_path();
        match self.error {
            BufferError::BufferDoesNotHavePath => f.write_str("buffer does not have a path"),
            BufferError::CouldNotOpenFile => {
                f.write_fmt(format_args!("could not open file '{:?}'", path))
            }
            BufferError::CouldNotReadFile => {
                f.write_fmt(format_args!("could not read from file '{:?}'", path))
            }
            BufferError::CouldNotCreateFile => {
                f.write_fmt(format_args!("could not create file '{:?}'", path))
            }
            BufferError::CouldNotWriteFile => {
                f.write_fmt(format_args!("could not write to file '{:?}'", path))
            }
        }
    }
}

pub struct Buffer {
    alive: bool,
    handle: BufferHandle,
    path: PathBuf,
    content: BufferContent,
    syntax_handle: SyntaxHandle,
    highlighted: HighlightedBuffer,
    history: History,
    search_ranges: Vec<BufferRange>,
    needs_save: bool,
    capabilities: BufferCapabilities,
}

impl Buffer {
    fn new(handle: BufferHandle) -> Self {
        Self {
            alive: true,
            handle,
            path: PathBuf::new(),
            content: BufferContent::new(),
            syntax_handle: SyntaxHandle::default(),
            highlighted: HighlightedBuffer::new(),
            history: History::new(),
            search_ranges: Vec::new(),
            needs_save: false,
            capabilities: BufferCapabilities::default(),
        }
    }

    fn dispose(&mut self, word_database: &mut WordDatabase) {
        self.remove_all_words_from_database(word_database);
        self.content.clear();

        self.alive = false;
        self.path.clear();
        self.syntax_handle = SyntaxHandle::default();
        self.highlighted.clear();
        self.history.clear();
        self.search_ranges.clear();
        self.needs_save = false;
        self.capabilities = BufferCapabilities::default();
    }

    fn remove_all_words_from_database(&mut self, word_database: &mut WordDatabase) {
        if self.capabilities.uses_word_database {
            for line in &self.content.lines {
                for word in WordIter::new(line.as_str()).of_kind(WordKind::Identifier) {
                    word_database.remove(word);
                }
            }
        }
    }

    pub fn handle(&self) -> BufferHandle {
        self.handle
    }

    pub fn path(&self) -> Option<&Path> {
        if self.path.as_os_str().is_empty() {
            None
        } else {
            Some(&self.path)
        }
    }

    pub fn set_path(&mut self, path: Option<&Path>) {
        self.path.clear();
        if let Some(path) = path {
            self.path.push(path);
        }
    }

    pub fn highlighted(&self) -> &HighlightedBuffer {
        &self.highlighted
    }

    pub fn update_highlighting(&mut self, syntaxes: &SyntaxCollection) -> HighlightResult {
        self.highlighted
            .highlight_dirty_lines(syntaxes.get(self.syntax_handle), &self.content)
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
            self.highlighted.clear();
            self.highlighted.on_insert(BufferRange::between(
                BufferPosition::line_col(0, 0),
                BufferPosition::line_col(self.content.line_count() - 1, 0),
            ));
        }
    }

    pub fn content(&self) -> &BufferContent {
        &self.content
    }

    pub fn needs_save(&self) -> bool {
        self.capabilities.can_save && self.needs_save
    }

    pub fn capabilities(&self) -> &BufferCapabilities {
        &self.capabilities
    }

    pub fn insert_text(
        &mut self,
        word_database: &mut WordDatabase,
        position: BufferPosition,
        text: &str,
        events: &mut EditorEventQueue,
    ) -> BufferRange {
        self.search_ranges.clear();
        let position = self.content.clamp_position(position);

        if text.is_empty() {
            return BufferRange::between(position, position);
        }
        self.needs_save = true;

        let range = Self::insert_text_no_history(
            &mut self.content,
            &mut self.highlighted,
            self.capabilities.uses_word_database,
            word_database,
            position,
            text,
        );

        events.enqueue_buffer_insert(self.handle, range, text);

        if self.capabilities.has_history {
            self.history.add_edit(Edit {
                kind: EditKind::Insert,
                range,
                text,
            });
        }

        range
    }

    pub fn insert_text_no_history(
        content: &mut BufferContent,
        highlighted: &mut HighlightedBuffer,
        uses_word_database: bool,
        word_database: &mut WordDatabase,
        position: BufferPosition,
        text: &str,
    ) -> BufferRange {
        if uses_word_database {
            for word in WordIter::new(content.line_at(position.line_index).as_str())
                .of_kind(WordKind::Identifier)
            {
                word_database.remove(word);
            }
        }

        let range = content.insert_text(position, text);
        highlighted.on_insert(range);

        if uses_word_database {
            let line_count = range.to.line_index - range.from.line_index + 1;
            for line in content.lines().skip(range.from.line_index).take(line_count) {
                for word in WordIter::new(line.as_str()).of_kind(WordKind::Identifier) {
                    word_database.add(word);
                }
            }
        }

        range
    }

    pub fn delete_range(
        &mut self,
        word_database: &mut WordDatabase,
        mut range: BufferRange,
        events: &mut EditorEventQueue,
    ) {
        self.search_ranges.clear();
        range.from = self.content.clamp_position(range.from);
        range.to = self.content.clamp_position(range.to);

        if range.from == range.to {
            return;
        }
        self.needs_save = true;

        events.enqueue(EditorEvent::BufferDeleteText {
            handle: self.handle,
            range,
        });

        let from = range.from;
        let to = range.to;

        if self.capabilities.has_history {
            fn add_history_delete_line(buffer: &mut Buffer, from: BufferPosition) {
                let line = buffer.content.line_at(from.line_index).as_str();
                let range = BufferRange::between(
                    BufferPosition::line_col(from.line_index, line.len()),
                    BufferPosition::line_col(from.line_index + 1, 0),
                );
                buffer.history.add_edit(Edit {
                    kind: EditKind::Delete,
                    range,
                    text: "\n",
                });
                let range = BufferRange::between(from, range.from);
                buffer.history.add_edit(Edit {
                    kind: EditKind::Delete,
                    range,
                    text: &line[from.column_byte_index..],
                });
            }

            if from.line_index == to.line_index {
                let text = &self.content.line_at(from.line_index).as_str()
                    [from.column_byte_index..to.column_byte_index];
                self.history.add_edit(Edit {
                    kind: EditKind::Delete,
                    range,
                    text,
                });
            } else {
                let text = &self.content.line_at(to.line_index).as_str()[..to.column_byte_index];
                self.history.add_edit(Edit {
                    kind: EditKind::Delete,
                    range: BufferRange::between(BufferPosition::line_col(to.line_index, 0), to),
                    text,
                });
                for line_index in ((from.line_index + 1)..to.line_index).rev() {
                    add_history_delete_line(self, BufferPosition::line_col(line_index, 0));
                }
                add_history_delete_line(self, from);
            }
        }

        Self::delete_range_no_history(
            &mut self.content,
            &mut self.highlighted,
            self.capabilities.uses_word_database,
            word_database,
            range,
        );
    }

    fn delete_range_no_history(
        content: &mut BufferContent,
        highlighted: &mut HighlightedBuffer,
        uses_word_database: bool,
        word_database: &mut WordDatabase,
        range: BufferRange,
    ) {
        if uses_word_database {
            let line_count = range.to.line_index - range.from.line_index + 1;
            for line in content.lines().skip(range.from.line_index).take(line_count) {
                for word in WordIter::new(line.as_str()).of_kind(WordKind::Identifier) {
                    word_database.remove(word);
                }
            }

            content.delete_range(range);

            for word in WordIter::new(content.line_at(range.from.line_index).as_str())
                .of_kind(WordKind::Identifier)
            {
                word_database.add(word);
            }
        } else {
            content.delete_range(range);
        }

        highlighted.on_delete(range);
    }

    pub fn commit_edits(&mut self) {
        self.history.commit_edits();
    }

    pub fn undo<'a>(
        &'a mut self,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
    ) -> impl 'a + Iterator<Item = Edit<'a>> {
        self.history_edits(word_database, events, |h| h.undo_edits())
    }

    pub fn redo<'a>(
        &'a mut self,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
    ) -> impl 'a + Iterator<Item = Edit<'a>> {
        self.history_edits(word_database, events, |h| h.redo_edits())
    }

    fn history_edits<'a, F, I>(
        &'a mut self,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
        selector: F,
    ) -> I
    where
        F: FnOnce(&'a mut History) -> I,
        I: 'a + Clone + Iterator<Item = Edit<'a>>,
    {
        self.search_ranges.clear();
        self.needs_save = true;

        let content = &mut self.content;
        let highlighted = &mut self.highlighted;
        let uses_word_database = self.capabilities.uses_word_database;

        let edits = selector(&mut self.history);
        for edit in edits.clone() {
            match edit.kind {
                EditKind::Insert => {
                    Self::insert_text_no_history(
                        content,
                        highlighted,
                        uses_word_database,
                        word_database,
                        edit.range.from,
                        edit.text,
                    );
                    events.enqueue_buffer_insert(self.handle, edit.range, edit.text);
                }
                EditKind::Delete => {
                    Self::delete_range_no_history(
                        content,
                        highlighted,
                        uses_word_database,
                        word_database,
                        edit.range,
                    );
                    events.enqueue(EditorEvent::BufferDeleteText {
                        handle: self.handle,
                        range: edit.range,
                    });
                }
            }
        }

        edits
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

    pub fn save_to_file(
        &mut self,
        path: Option<&Path>,
        events: &mut EditorEventQueue,
    ) -> Result<(), BufferError> {
        let new_path = match path {
            Some(path) => {
                self.set_path(Some(path));
                true
            }
            None => false,
        };

        if !self.capabilities.can_save {
            return Ok(());
        }

        match self.path() {
            Some(path) => {
                let file = File::create(path).map_err(|_| BufferError::CouldNotCreateFile)?;
                let mut writer = io::BufWriter::new(file);

                self.content
                    .write(&mut writer)
                    .map_err(|_| BufferError::CouldNotWriteFile)?;
                self.needs_save = false;

                events.enqueue(EditorEvent::BufferSave {
                    handle: self.handle,
                    new_path,
                });
                Ok(())
            }
            None => Err(BufferError::BufferDoesNotHavePath),
        }
    }

    pub fn discard_and_reload_from_file(
        &mut self,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
    ) -> Result<(), BufferError> {
        if !self.capabilities.can_save {
            return Ok(());
        }

        let path = self.path.as_path();
        if path.as_os_str().is_empty() {
            return Err(BufferError::BufferDoesNotHavePath);
        }

        let file = File::open(path).map_err(|_| BufferError::CouldNotOpenFile)?;
        let mut reader = io::BufReader::new(file);

        self.remove_all_words_from_database(word_database);

        self.content
            .read(&mut reader)
            .map_err(|_| BufferError::CouldNotReadFile)?;
        self.highlighted.clear();
        self.highlighted.on_insert(BufferRange::between(
            BufferPosition::line_col(0, 0),
            BufferPosition::line_col(self.content.line_count() - 1, 0),
        ));

        events.enqueue(EditorEvent::BufferLoad {
            handle: self.handle,
        });

        if self.capabilities.uses_word_database {
            for line in &self.content.lines {
                for word in WordIter::new(line.as_str()).of_kind(WordKind::Identifier) {
                    word_database.add(word);
                }
            }
        }

        self.history.clear();
        self.search_ranges.clear();

        self.needs_save = false;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BufferHandle(pub usize);
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
    pub fn new(&mut self, capabilities: BufferCapabilities) -> &mut Buffer {
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
                self.buffers.push(Buffer::new(handle));
                handle
            }
        };

        let buffer = &mut self.buffers[handle.0];
        buffer.alive = true;
        buffer.capabilities = capabilities;
        buffer
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

    pub fn find_with_path(&self, root: &Path, path: &Path) -> Option<&Buffer> {
        if path.as_os_str().len() == 0 {
            return None;
        }

        let path = path.strip_prefix(root).unwrap_or(path);

        for buffer in self.iter() {
            let buffer_path = buffer.path.as_path();
            let buffer_path = buffer_path.strip_prefix(root).unwrap_or(buffer_path);

            if buffer_path == path {
                return Some(buffer);
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

    pub fn defer_remove(&mut self, handle: BufferHandle, events: &mut EditorEventQueue) {
        let buffer = &mut self.buffers[handle.0];
        if buffer.alive {
            buffer.alive = false;
            events.enqueue(EditorEvent::BufferClose { handle });
        }
    }

    pub fn remove(
        &mut self,
        handle: BufferHandle,
        clients: &mut ClientManager,
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

    fn buffer_from_str(text: &str) -> BufferContent {
        let mut buffer = BufferContent::new();
        buffer.insert_text(BufferPosition::line_col(0, 0), text);
        buffer
    }

    #[test]
    fn buffer_line_char_count() {
        let mut line_pool = BufferLinePool::new();
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
        let mut buffer = buffer_from_str("abd");
        let range = buffer.insert_text(BufferPosition::line_col(0, 2), "ç");
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
        let mut buffer = BufferContent::new();

        assert_eq!(1, buffer.line_count());
        assert_eq!("", buffer.to_string());

        buffer.insert_text(BufferPosition::line_col(0, 0), "hold");
        buffer.insert_text(BufferPosition::line_col(0, 2), "r");
        buffer.insert_text(BufferPosition::line_col(0, 1), "ello w");
        assert_eq!(1, buffer.line_count());
        assert_eq!("hello world", buffer.to_string());

        buffer.insert_text(BufferPosition::line_col(0, 5), "\n");
        buffer.insert_text(
            BufferPosition::line_col(1, 6),
            " appending more\nand more\nand even more\nlines",
        );
        assert_eq!(5, buffer.line_count());
        assert_eq!(
            "hello\n world appending more\nand more\nand even more\nlines",
            buffer.to_string()
        );

        let mut buffer = buffer_from_str("this is content");
        buffer.insert_text(BufferPosition::line_col(0, 8), "some\nmultiline ");
        assert_eq!(2, buffer.line_count());
        assert_eq!("this is some\nmultiline content", buffer.to_string());

        let mut buffer = buffer_from_str("this is content");
        buffer.insert_text(
            BufferPosition::line_col(0, 8),
            "some\nmore\nextensive\nmultiline ",
        );
        assert_eq!(4, buffer.line_count());
        assert_eq!(
            "this is some\nmore\nextensive\nmultiline content",
            buffer.to_string()
        );

        let mut buffer = buffer_from_str("abc");
        let range = buffer.insert_text(BufferPosition::line_col(0, 3), "\n");
        assert_eq!(
            BufferRange::between(
                BufferPosition::line_col(0, 3),
                BufferPosition::line_col(1, 0)
            ),
            range
        );
    }

    #[test]
    fn buffer_content_delete_range() {
        let mut buffer = buffer_from_str("abc");
        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(0, 1),
        ));
        assert_eq!("abc", buffer.to_string());
        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(0, 2),
        ));
        assert_eq!("ac", buffer.to_string());

        let mut buffer = buffer_from_str("this is the initial\ncontent of the buffer");

        assert_eq!(2, buffer.line_count());
        assert_eq!(
            "this is the initial\ncontent of the buffer",
            buffer.to_string()
        );

        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 0),
            BufferPosition::line_col(0, 0),
        ));
        assert_eq!(2, buffer.line_count());
        assert_eq!(
            "this is the initial\ncontent of the buffer",
            buffer.to_string()
        );

        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 11),
            BufferPosition::line_col(0, 19),
        ));
        assert_eq!(2, buffer.line_count());
        assert_eq!("this is the\ncontent of the buffer", buffer.to_string());

        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 8),
            BufferPosition::line_col(1, 15),
        ));
        assert_eq!(1, buffer.line_count());
        assert_eq!("this is buffer", buffer.to_string());

        let mut buffer = buffer_from_str("this\nbuffer\ncontains\nmultiple\nlines\nyes");
        assert_eq!(6, buffer.line_count());
        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 4),
            BufferPosition::line_col(4, 1),
        ));
        assert_eq!("this\nbuffines\nyes", buffer.to_string());
    }

    #[test]
    fn buffer_content_delete_lines() {
        let mut buffer = buffer_from_str("first line\nsecond line\nthird line");
        assert_eq!(3, buffer.line_count());
        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 0),
            BufferPosition::line_col(2, 0),
        ));
        assert_eq!("first line\nthird line", buffer.to_string());

        let mut buffer = buffer_from_str("first line\nsecond line\nthird line");
        assert_eq!(3, buffer.line_count());
        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 0),
            BufferPosition::line_col(1, 11),
        ));
        assert_eq!("first line\n\nthird line", buffer.to_string());
    }

    #[test]
    fn buffer_delete_undo_redo_single_line() {
        let mut word_database = WordDatabase::new();
        let mut events = EditorEventQueue::default();

        let mut buffer = Buffer::new(BufferHandle(0));
        buffer.capabilities = BufferCapabilities::text();
        buffer.insert_text(
            &mut word_database,
            BufferPosition::line_col(0, 0),
            "single line content",
            &mut events,
        );
        let range = BufferRange::between(
            BufferPosition::line_col(0, 7),
            BufferPosition::line_col(0, 12),
        );
        buffer.delete_range(&mut word_database, range, &mut events);

        assert_eq!("single content", buffer.content.to_string());
        {
            let mut ranges = buffer.undo(&mut word_database, &mut events);
            assert_eq!(range, ranges.next().unwrap().range);
            ranges.next().unwrap();
            assert!(ranges.next().is_none());
        }
        assert!(buffer.content.to_string().is_empty());
        let mut redo_iter = buffer.redo(&mut word_database, &mut events);
        redo_iter.next().unwrap();
        redo_iter.next().unwrap();
        assert!(redo_iter.next().is_none());
        drop(redo_iter);
        assert_eq!("single content", buffer.content.to_string());
    }

    #[test]
    fn buffer_delete_undo_redo_multi_line() {
        let mut word_database = WordDatabase::new();
        let mut events = EditorEventQueue::default();

        let mut buffer = Buffer::new(BufferHandle(0));
        buffer.capabilities = BufferCapabilities::text();
        let insert_range = buffer.insert_text(
            &mut word_database,
            BufferPosition::line_col(0, 0),
            "multi\nline\ncontent",
            &mut events,
        );
        assert_eq!("multi\nline\ncontent", buffer.content.to_string());

        let delete_range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(1, 3),
        );
        buffer.delete_range(&mut word_database, delete_range, &mut events);
        assert_eq!("me\ncontent", buffer.content.to_string());

        {
            let mut undo_edits = buffer.undo(&mut word_database, &mut events);
            assert_eq!(delete_range, undo_edits.next().unwrap().range);
            assert_eq!(insert_range, undo_edits.next().unwrap().range);
            assert!(undo_edits.next().is_none());
        }
        assert_eq!("", buffer.content.to_string());

        {
            let mut redo_edits = buffer.redo(&mut word_database, &mut events);
            redo_edits.next().unwrap();
            redo_edits.next().unwrap();
            assert!(redo_edits.next().is_none());
        }
        assert_eq!("me\ncontent", buffer.content.to_string());
    }

    #[test]
    fn buffer_content_range_text() {
        let buffer = buffer_from_str("abc\ndef\nghi");
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

        let buffer = buffer_from_str("word");
        assert_word!(buffer.word_at(col(0)), col(0), WordKind::Identifier, "word");
        assert_word!(buffer.word_at(col(2)), col(0), WordKind::Identifier, "word");
        assert_word!(buffer.word_at(col(4)), col(4), WordKind::Whitespace, "");

        let buffer = buffer_from_str("asd word+? asd");
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

        let buffer = buffer_from_str("word");
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

        let buffer = buffer_from_str("first second third");
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
        let buffer = buffer_from_str("(\n(\na\n)\nbc)");

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
        let buffer = buffer_from_str("|a|bcd|efg|");

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
