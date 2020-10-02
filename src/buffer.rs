use std::{
    convert::From,
    fs::File,
    io,
    ops::RangeBounds,
    path::{Path, PathBuf},
};

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    client::ClientCollection,
    history::{Edit, EditKind, History},
    script::ScriptValue,
    syntax::{self, HighlightedBuffer, SyntaxCollection, SyntaxHandle},
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

pub struct BufferLine {
    text: String,
    char_count: usize,
}

impl BufferLine {
    pub fn new(text: String) -> Self {
        let char_count = text.chars().count();
        Self { text, char_count }
    }

    pub fn char_count(&self) -> usize {
        self.char_count
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn split_off(&mut self, index: usize) -> BufferLine {
        let splitted = self.text.split_off(index);
        let splitted_char_count = splitted.chars().count();
        self.char_count -= splitted_char_count;

        BufferLine {
            text: splitted,
            char_count: splitted_char_count,
        }
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
    pub const fn empty() -> Self {
        Self { lines: Vec::new() }
    }

    pub fn from_str(text: &str) -> Self {
        let mut this = Self { lines: Vec::new() };
        this.lines.push(BufferLine::new(String::new()));
        this.insert_text(BufferPosition::line_col(0, 0), text);
        this
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

    pub fn write<W>(&self, write: &mut W) -> io::Result<()>
    where
        W: io::Write,
    {
        let last_index = self.lines.len() - 1;
        for line in &self.lines[..last_index] {
            writeln!(write, "{}", line.as_str())?;
        }
        write!(write, "{}", self.lines[last_index].as_str())?;
        Ok(())
    }

    pub fn saturate_position(&self, mut position: BufferPosition) -> BufferPosition {
        position.line_index = position.line_index.min(self.line_count() - 1);
        position.column_byte_index = self
            .line_at(position.line_index)
            .as_str()
            .len()
            .min(position.column_byte_index);
        position
    }

    pub fn append_range_text_to_string(&self, range: BufferRange, text: &mut String) {
        let from = self.clamp_position(range.from);
        let to = self.clamp_position(range.to);

        if from.line_index == to.line_index {
            let range_text =
                &self.lines[from.line_index].as_str()[from.column_byte_index..to.column_byte_index];
            text.push_str(range_text);
        } else {
            text.push_str(&self.lines[from.line_index].as_str()[from.column_byte_index..]);
            let lines_range = (from.line_index + 1)..to.line_index;
            if lines_range.start < lines_range.end {
                for line in &self.lines[lines_range] {
                    text.push('\n');
                    text.push_str(line.as_str());
                }
            }
            let to_line_index = from.line_index + 1;
            if to_line_index < self.lines.len() {
                let to_line = &self.lines[to_line_index];
                text.push('\n');
                text.push_str(&to_line.as_str()[..to.column_byte_index]);
            }
        }
    }

    pub fn find_search_ranges(&self, text: &str, ranges: &mut Vec<BufferRange>) {
        if text.is_empty() {
            return;
        }

        for (i, line) in self.lines.iter().enumerate() {
            for (j, _) in line.as_str().match_indices(text) {
                ranges.push(BufferRange::between(
                    BufferPosition::line_col(i, j),
                    BufferPosition::line_col(i, j + text.len()),
                ));
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

    fn insert_text(&mut self, position: BufferPosition, text: &str) -> BufferRange {
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
            let split_line = self.lines[position.line_index].split_off(position.column_byte_index);

            let mut line_count = 0;
            let mut lines = text.lines();
            if let Some(line) = lines.next() {
                self.lines[position.line_index].push_text(&line);
            }
            for line in lines {
                line_count += 1;
                self.lines.insert(
                    position.line_index + line_count,
                    BufferLine::new(line.into()),
                );
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

    fn delete_range(&mut self, range: BufferRange) -> Text {
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

    pub fn chars_from<'a>(
        &'a self,
        position: BufferPosition,
    ) -> (
        impl 'a + Iterator<Item = (usize, char)>,
        impl 'a + Iterator<Item = (usize, char)>,
    ) {
        let BufferPosition {
            line_index,
            column_byte_index,
        } = position;
        let (left, right) = self
            .line_at(line_index)
            .as_str()
            .split_at(column_byte_index);
        let left_chars = left.char_indices().rev();
        let right_chars = right
            .char_indices()
            .map(move |(i, c)| (column_byte_index + i, c));
        (left_chars, right_chars)
    }

    pub fn words_from<'a>(
        &'a self,
        position: BufferPosition,
    ) -> (
        WordRefWithPosition<'a>,
        impl Iterator<Item = WordRefWithPosition<'a>>,
        impl Iterator<Item = WordRefWithPosition<'a>>,
    ) {
        let position = self.clamp_position(position);
        let mid_word = self.word_at(position);
        let mid_start_index = mid_word.position.column_byte_index;
        let mid_end_index = mid_start_index + mid_word.text.len();

        let line = self.line_at(position.line_index).as_str();
        let left = &line[..mid_start_index];
        let right = &line[mid_end_index..];

        let mut left_column_index = mid_start_index;
        let left_words = WordIter::new(left).rev().map(move |w| {
            left_column_index -= w.text.len();
            let position = BufferPosition::line_col(position.line_index, left_column_index);
            WordRefWithPosition {
                kind: w.kind,
                text: w.text,
                position,
            }
        });

        let mut right_column_index = mid_end_index;
        let right_words = WordIter::new(right).map(move |w| {
            let position = BufferPosition::line_col(position.line_index, right_column_index);
            right_column_index += w.text.len();
            WordRefWithPosition {
                kind: w.kind,
                text: w.text,
                position,
            }
        });

        (mid_word, left_words, right_words)
    }

    pub fn word_at(&self, position: BufferPosition) -> WordRefWithPosition {
        let position = self.clamp_position(position);
        let line = self.line_at(position.line_index).as_str();
        let (before, after) = line.split_at(position.column_byte_index);

        match WordIter::new(after).next() {
            Some(right) => match WordIter::new(before).next_back() {
                Some(left) => {
                    if left.kind == right.kind {
                        let end_index = position.column_byte_index + right.text.len();
                        let position = BufferPosition::line_col(
                            position.line_index,
                            position.column_byte_index - left.text.len(),
                        );
                        WordRefWithPosition {
                            kind: left.kind,
                            text: &line[position.column_byte_index..end_index],
                            position,
                        }
                    } else {
                        WordRefWithPosition {
                            kind: right.kind,
                            text: right.text,
                            position,
                        }
                    }
                }
                None => WordRefWithPosition {
                    kind: right.kind,
                    text: right.text,
                    position,
                },
            },
            None => WordRefWithPosition {
                kind: WordKind::Whitespace,
                text: "",
                position,
            },
        }
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

pub struct Buffer {
    path: PathBuf,
    pub content: BufferContent,
    syntax_handle: SyntaxHandle,
    pub highlighted: HighlightedBuffer,
    history: History,
    search_ranges: Vec<BufferRange>,
    needs_save: bool,
}

impl Buffer {
    pub fn new(
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        path: Option<PathBuf>,
        content: BufferContent,
    ) -> Self {
        for line in content.lines() {
            for word in WordIter::new(line.as_str()).of_kind(WordKind::Identifier) {
                word_database.add_word(word);
            }
        }

        let syntax_handle = path
            .as_ref()
            .and_then(|p| syntaxes.find_handle_by_extension(syntax::get_path_extension(p)))
            .unwrap_or(SyntaxHandle::default());

        let mut highlighted = HighlightedBuffer::new();
        highlighted.highligh_all(syntaxes.get(syntax_handle), &content);

        let path = path.unwrap_or(PathBuf::new());

        Self {
            path,
            content,
            syntax_handle,
            highlighted,
            history: History::new(),
            search_ranges: Vec::new(),
            needs_save: false,
        }
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

        let syntax_handle = path
            .and_then(|p| syntaxes.find_handle_by_extension(syntax::get_path_extension(p)))
            .unwrap_or(SyntaxHandle::default());

        if self.syntax_handle != syntax_handle {
            self.syntax_handle = syntax_handle;
            self.highlighted
                .highligh_all(syntaxes.get(self.syntax_handle), &self.content);
        }
    }

    pub fn needs_save(&self) -> bool {
        self.needs_save
    }

    pub fn insert_text(
        &mut self,
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        position: BufferPosition,
        text: &str,
        cursor_index: usize,
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

        let range = self.content.insert_text(position, text);

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
        self.history.add_edit(Edit {
            kind: EditKind::Insert,
            range,
            text,
            cursor_index: cursor_index.min(u8::MAX as _) as _,
        });
        range
    }

    pub fn delete_range(
        &mut self,
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        range: BufferRange,
        cursor_index: usize,
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

        let deleted_text = self.content.delete_range(range);

        for word in WordIter::new(self.content.line_at(range.from.line_index).as_str())
            .of_kind(WordKind::Identifier)
        {
            word_database.add_word(word);
        }

        self.highlighted
            .on_delete(syntaxes.get(self.syntax_handle), &self.content, range);
        self.history.add_edit(Edit {
            kind: EditKind::Delete,
            range,
            text: deleted_text.as_str(),
            cursor_index: cursor_index.min(u8::MAX as _) as _,
        });
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
                    let range = self.content.insert_text(edit.range.from, edit.text);
                    self.highlighted.on_insert(syntax, &self.content, range);
                }
                EditKind::Delete => {
                    self.content.delete_range(edit.range);
                    self.highlighted
                        .on_insert(syntax, &self.content, edit.range);
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

    pub fn set_search_with<F>(&mut self, selector: F)
    where
        F: FnOnce(&BufferContent) -> &str,
    {
        self.search_ranges.clear();
        let text = selector(&self.content);
        self.content
            .find_search_ranges(text, &mut self.search_ranges);
    }

    pub fn search_ranges(&self) -> &[BufferRange] {
        &self.search_ranges
    }

    pub fn save_to_file(&mut self) -> Result<(), String> {
        match self.path() {
            Some(path) => {
                let mut file = File::create(path)
                    .map_err(|e| format!("could not create file {:?}: {:?}", path, e))?;

                self.content
                    .write(&mut file)
                    .map_err(|e| format!("could not write to file {:?}: {:?}", path, e))?;

                self.needs_save = false;
                Ok(())
            }
            None => Err("buffer has no path".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BufferHandle(usize);

impl_from_script!(BufferHandle, from => match from {
    ScriptValue::Integer(n) if n >= 0 => Some(Self(n as _)),
    _ => None,
});
impl_to_script!(BufferHandle, self => ScriptValue::Integer(self.0 as _));

#[derive(Default)]
pub struct BufferCollection {
    buffers: Vec<Option<Buffer>>,
}

impl BufferCollection {
    pub fn add(&mut self, buffer: Buffer) -> BufferHandle {
        for (i, slot) in self.buffers.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(buffer);
                return BufferHandle(i);
            }
        }

        let handle = BufferHandle(self.buffers.len());
        self.buffers.push(Some(buffer));
        handle
    }

    pub fn get(&self, handle: BufferHandle) -> Option<&Buffer> {
        self.buffers[handle.0].as_ref()
    }

    pub fn get_mut(&mut self, handle: BufferHandle) -> Option<&mut Buffer> {
        self.buffers[handle.0].as_mut()
    }

    pub fn find_with_path(&self, path: &Path) -> Option<BufferHandle> {
        if path.as_os_str().len() == 0 {
            return None;
        }

        for (handle, buffer) in self.iter_with_handles() {
            if buffer.path == path {
                return Some(handle);
            }
        }

        None
    }

    pub fn iter(&self) -> impl Iterator<Item = &Buffer> {
        self.buffers.iter().filter_map(|b| b.as_ref())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Buffer> {
        self.buffers.iter_mut().filter_map(|b| b.as_mut())
    }

    pub fn iter_with_handles(&self) -> impl Iterator<Item = (BufferHandle, &Buffer)> {
        self.buffers
            .iter()
            .enumerate()
            .filter_map(|(i, b)| Some(BufferHandle(i)).zip(b.as_ref()))
    }

    pub fn remove_where<F>(
        &mut self,
        clients: &mut ClientCollection,
        word_database: &mut WordDatabase,
        predicate: F,
    ) where
        F: Fn(BufferHandle, &Buffer) -> bool,
    {
        for i in 0..self.buffers.len() {
            if let Some(buffer) = &mut self.buffers[i] {
                let handle = BufferHandle(i);
                if predicate(handle, buffer) {
                    for line in buffer.content.lines() {
                        for word in WordIter::new(line.as_str()).of_kind(WordKind::Identifier) {
                            word_database.remove_word(word);
                        }
                    }

                    for client in clients.iter_mut() {
                        client
                            .navigation_history
                            .remove_snapshots_with_buffer_handle(handle);
                    }

                    self.buffers[i] = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer_position::BufferPosition;

    fn buffer_to_string(buffer: &BufferContent) -> String {
        let mut buf = Vec::new();
        buffer.write(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
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
        let mut line = BufferLine::new("abc".into());
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
        let mut buffer = BufferContent::from_str("abd");
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
        let mut buffer = BufferContent::from_str("");

        assert_eq!(1, buffer.line_count());
        assert_eq!("", buffer_to_string(&buffer));

        buffer.insert_text(BufferPosition::line_col(0, 0), "hold");
        buffer.insert_text(BufferPosition::line_col(0, 2), "r");
        buffer.insert_text(BufferPosition::line_col(0, 1), "ello w");
        assert_eq!(1, buffer.line_count());
        assert_eq!("hello world", buffer_to_string(&buffer));

        buffer.insert_text(BufferPosition::line_col(0, 5), "\n");
        buffer.insert_text(
            BufferPosition::line_col(1, 6),
            " appending more\nand more\nand even more\nlines",
        );
        assert_eq!(5, buffer.line_count());
        assert_eq!(
            "hello\n world appending more\nand more\nand even more\nlines",
            buffer_to_string(&buffer)
        );

        let mut buffer = BufferContent::from_str("this is content");
        buffer.insert_text(BufferPosition::line_col(0, 8), "some\nmultiline ");
        assert_eq!(2, buffer.line_count());
        assert_eq!("this is some\nmultiline content", buffer_to_string(&buffer));

        let mut buffer = BufferContent::from_str("this is content");
        buffer.insert_text(
            BufferPosition::line_col(0, 8),
            "some\nmore\nextensive\nmultiline ",
        );
        assert_eq!(4, buffer.line_count());
        assert_eq!(
            "this is some\nmore\nextensive\nmultiline content",
            buffer_to_string(&buffer)
        );
    }

    #[test]
    fn buffer_content_delete_range() {
        let mut buffer = BufferContent::from_str("this is the initial\ncontent of the buffer");

        assert_eq!(2, buffer.line_count());
        assert_eq!(
            "this is the initial\ncontent of the buffer",
            buffer_to_string(&buffer)
        );

        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 0),
            BufferPosition::line_col(0, 0),
        ));
        assert_eq!(2, buffer.line_count());
        assert_eq!(
            "this is the initial\ncontent of the buffer",
            buffer_to_string(&buffer)
        );
        assert_eq!("", deleted_text.as_str());

        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 11),
            BufferPosition::line_col(0, 19),
        ));
        assert_eq!(2, buffer.line_count());
        assert_eq!(
            "this is the\ncontent of the buffer",
            buffer_to_string(&buffer)
        );
        assert_eq!(" initial", deleted_text.as_str());

        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 8),
            BufferPosition::line_col(1, 15),
        ));
        assert_eq!(1, buffer.line_count());
        assert_eq!("this is buffer", buffer_to_string(&buffer));
        assert_eq!("the\ncontent of the ", deleted_text.as_str());

        let mut buffer = BufferContent::from_str("this\nbuffer\ncontains\nmultiple\nlines\nyes");
        assert_eq!(6, buffer.line_count());
        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 4),
            BufferPosition::line_col(4, 1),
        ));
        assert_eq!("this\nbuffines\nyes", buffer_to_string(&buffer));
        assert_eq!("er\ncontains\nmultiple\nl", deleted_text.as_str());
    }

    #[test]
    fn buffer_content_delete_lines() {
        let mut buffer = BufferContent::from_str("first line\nsecond line\nthird line");
        assert_eq!(3, buffer.line_count());
        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 0),
            BufferPosition::line_col(2, 0),
        ));
        assert_eq!("first line\nthird line", buffer_to_string(&buffer));
        assert_eq!("second line\n", deleted_text.as_str());

        let mut buffer = BufferContent::from_str("first line\nsecond line\nthird line");
        assert_eq!(3, buffer.line_count());
        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 0),
            BufferPosition::line_col(1, 11),
        ));
        assert_eq!("first line\n\nthird line", buffer_to_string(&buffer));
        assert_eq!("second line", deleted_text.as_str());
    }

    #[test]
    fn buffer_delete_undo_redo_single_line() {
        let mut word_database = WordDatabase::new();
        let syntaxes = SyntaxCollection::new();

        let mut buffer = Buffer::new(
            &mut word_database,
            &syntaxes,
            None,
            BufferContent::from_str("single line content"),
        );
        let range = BufferRange::between(
            BufferPosition::line_col(0, 7),
            BufferPosition::line_col(0, 12),
        );
        buffer.delete_range(&mut word_database, &syntaxes, range, 0);

        assert_eq!("single content", buffer_to_string(&buffer.content));
        {
            let mut ranges = buffer.undo(&syntaxes);
            assert_eq!(range, ranges.next().unwrap().range);
            assert!(ranges.next().is_none());
        }
        assert_eq!("single line content", buffer_to_string(&buffer.content));
        for _ in buffer.redo(&syntaxes) {}
        assert_eq!("single content", buffer_to_string(&buffer.content));
    }

    #[test]
    fn buffer_delete_undo_redo_multi_line() {
        let mut word_database = WordDatabase::new();
        let syntaxes = SyntaxCollection::new();

        let mut buffer = Buffer::new(
            &mut word_database,
            &syntaxes,
            None,
            BufferContent::from_str("multi\nline\ncontent"),
        );
        let range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(1, 3),
        );
        buffer.delete_range(&mut word_database, &syntaxes, range, 0);

        assert_eq!("me\ncontent", buffer_to_string(&buffer.content));
        {
            let mut ranges = buffer.undo(&syntaxes);
            assert_eq!(range, ranges.next().unwrap().range);
            assert!(ranges.next().is_none());
        }
        assert_eq!("multi\nline\ncontent", buffer_to_string(&buffer.content));
        for _ in buffer.redo(&syntaxes) {}
        assert_eq!("me\ncontent", buffer_to_string(&buffer.content));
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

        let buffer = BufferContent::from_str("word");
        assert_word!(buffer.word_at(col(0)), col(0), WordKind::Identifier, "word");
        assert_word!(buffer.word_at(col(2)), col(0), WordKind::Identifier, "word");
        assert_word!(buffer.word_at(col(4)), col(4), WordKind::Whitespace, "");

        let buffer = BufferContent::from_str("asd word+? asd");
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

        let buffer = BufferContent::from_str("word");
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

        let buffer = BufferContent::from_str("first second third");
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
        let buffer = BufferContent::from_str("(\n(\na\n)\nbc)");

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

        let buffer = BufferContent::from_str("|\n|\na\n|\nbc|");

        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(1, 0)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(0, 0), '|', '|')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(1, 1),
                BufferPosition::line_col(3, 0)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(2, 0), '|', '|')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(1, 0)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(0, 1), '|', '|')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(3, 1),
                BufferPosition::line_col(4, 2)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(4, 0), '|', '|')
        );
        assert_eq!(
            Some(BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(1, 0)
            )),
            buffer.find_balanced_chars_at(BufferPosition::line_col(0, 0), '|', '|')
        );
        assert_eq!(
            None,
            buffer.find_balanced_chars_at(BufferPosition::line_col(4, 2), '|', '|')
        );
    }
}
