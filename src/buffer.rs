use std::{
    convert::From,
    fs::File,
    io,
    ops::{Bound, Range, RangeBounds},
    path::{Path, PathBuf},
};

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    history::{Edit, EditKind, History},
    syntax::{self, HighlightedBuffer, SyntaxCollection, SyntaxHandle},
    word_database::{CharKind, WordDatabase, WordIter},
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

pub struct BufferLine {
    text: String,
    char_count: usize,
    char_extra_lengths: Vec<(usize, u8)>,
}

impl BufferLine {
    pub fn new(text: String) -> Self {
        let mut this = Self {
            text,
            char_count: 0,
            char_extra_lengths: Vec::new(),
        };
        this.sync_state();
        this
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn slice<R>(&self, range: R) -> &str
    where
        R: RangeBounds<usize>,
    {
        &self.text[self.column_range_to_index_range(range)]
    }

    pub fn split_off(&mut self, column: usize) -> BufferLine {
        let index = self.column_to_index(column);
        let splitted = BufferLine::new(self.text.split_off(index));
        self.sync_state();
        splitted
    }

    pub fn next_char_from(&self, column: usize, c: char) -> Option<usize> {
        let next_column = self.char_count().min(column + 1);
        let index = self.column_to_index(next_column);
        self.text[index..]
            .find(c)
            .map(|i| self.index_to_column(index + i))
    }

    pub fn previous_char_from(&self, column: usize, c: char) -> Option<usize> {
        let index = self.column_to_index(column);
        self.text[..index].rfind(c).map(|i| self.index_to_column(i))
    }

    pub fn first_word_start(&self) -> usize {
        match self
            .text
            .chars()
            .enumerate()
            .skip_while(|(_, c)| c.is_whitespace())
            .next()
        {
            Some((i, _)) => i,
            None => 0,
        }
    }

    pub fn next_word_start_from(&self, column: usize) -> usize {
        let index = self.column_to_index(column);

        let mut kinds = self.text[index..]
            .char_indices()
            .map(|(i, c)| (i, CharKind::new(c)));

        let first_kind = match kinds.next() {
            Some((_, k)) => k,
            None => return self.index_to_column(self.text.len()),
        };

        let index = match kinds
            .skip_while(|(_, k)| *k == first_kind)
            .skip_while(|(_, k)| *k == CharKind::Whitespace)
            .next()
        {
            Some((i, _)) => index + i,
            None => self.text.len(),
        };

        self.index_to_column(index)
    }

    pub fn previous_word_start_from(&self, column: usize) -> usize {
        let index = self.column_to_index(column);

        let mut kinds = self.text[..index]
            .char_indices()
            .rev()
            .map(|(i, c)| (i, CharKind::new(c)));

        let (first_index, first_kind) = match (&mut kinds)
            .skip_while(|(_, k)| *k == CharKind::Whitespace)
            .next()
        {
            Some((i, k)) => (i, k),
            None => return 0,
        };

        let index = match kinds.take_while(|(_, k)| *k == first_kind).last() {
            Some((i, _)) => i,
            None => first_index,
        };

        self.index_to_column(index)
    }

    pub fn find_word_at(&self, column: usize) -> (Range<usize>, &str) {
        let index = self.column_to_index(column);
        let (start, end) = self.text.split_at(index);

        let mut end_kinds = end.char_indices().map(|(i, c)| (i, CharKind::new(c)));
        let kind = match end_kinds.next() {
            Some((_, CharKind::Whitespace)) | None => return (column..column, ""),
            Some((_, k)) => k,
        };
        let end_index = end_kinds
            .take_while(|(_, k)| *k == kind)
            .last()
            .unwrap_or((0, CharKind::Whitespace))
            .0
            + index
            + 1;

        let start_index = start
            .char_indices()
            .rev()
            .map(|(i, c)| (i, CharKind::new(c)))
            .take_while(|(_, k)| *k == kind)
            .last()
            .unwrap_or((index, CharKind::Whitespace))
            .0;

        let index_range = start_index..end_index;
        let column_range = self.index_range_to_column_rangge(index_range.clone());
        (column_range, &self.text[index_range])
    }

    pub fn insert_text(&mut self, column: usize, text: &str) {
        let index = self.column_to_index(column);
        self.text.insert_str(index, text);
        self.sync_state();
    }

    pub fn push_text(&mut self, s: &str) {
        self.text.push_str(s);
        self.sync_state();
    }

    pub fn delete_range<R>(&mut self, range: R)
    where
        R: RangeBounds<usize>,
    {
        self.text.drain(self.column_range_to_index_range(range));
        self.sync_state();
    }

    pub fn char_count(&self) -> usize {
        self.char_count
    }

    fn sync_state(&mut self) {
        self.char_count = 0;
        self.char_extra_lengths.clear();

        for (i, c) in self.text.char_indices() {
            let char_len = c.len_utf8();
            if char_len > 1 {
                self.char_extra_lengths.push((i, (char_len - 1) as _));
            }

            self.char_count += 1;
        }
    }

    fn column_to_index(&self, column: usize) -> usize {
        let mut index = column;
        for &(i, len) in &self.char_extra_lengths {
            if i >= index {
                break;
            }

            index += len as usize;
        }

        index
    }

    fn index_to_column(&self, index: usize) -> usize {
        let mut column = index;
        for &(i, len) in &self.char_extra_lengths {
            if i >= index {
                break;
            }

            column -= len as usize;
        }

        column
    }

    fn column_range_to_index_range<R>(&self, range: R) -> Range<usize>
    where
        R: RangeBounds<usize>,
    {
        let start = match range.start_bound() {
            Bound::Included(&c) => self.column_to_index(c),
            Bound::Excluded(&c) => self.column_to_index(c + 1),
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&c) => self.column_to_index(c + 1),
            Bound::Excluded(&c) => self.column_to_index(c),
            Bound::Unbounded => self.text.len(),
        };

        Range { start, end }
    }

    fn index_range_to_column_rangge<R>(&self, range: R) -> Range<usize>
    where
        R: RangeBounds<usize>,
    {
        let start = match range.start_bound() {
            Bound::Included(&i) => self.index_to_column(i),
            Bound::Excluded(&i) => self.index_to_column(i) + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&i) => self.column_to_index(i) + 1,
            Bound::Excluded(&i) => self.column_to_index(i),
            Bound::Unbounded => self.text.len(),
        };

        Range { start, end }
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
            .char_count()
            .min(position.column_byte_index);
        position
    }

    pub fn append_range_text_to_string(&self, range: BufferRange, text: &mut String) {
        let from = self.clamp_position(range.from);
        let to = self.clamp_position(range.to);

        if from.line_index == to.line_index {
            let range_text = &self.lines[from.line_index].slice(from.column_byte_index..to.column_byte_index);
            text.push_str(range_text);
        } else {
            text.push_str(&self.lines[from.line_index].slice(from.column_byte_index..));
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
                text.push_str(&to_line.slice(..to.column_byte_index));
            }
        }
    }

    pub fn find_search_ranges(&self, text: &str, ranges: &mut Vec<BufferRange>) {
        let char_count = text.chars().count();
        if char_count == 0 {
            return;
        }

        for (i, line) in self.lines.iter().enumerate() {
            for (j, _) in line.as_str().match_indices(text) {
                ranges.push(BufferRange::between(
                    BufferPosition::line_col(i, j),
                    BufferPosition::line_col(i, j + char_count - 1),
                ));
            }
        }
    }

    fn clamp_position(&self, mut position: BufferPosition) -> BufferPosition {
        let line_count = self.line_count();
        if position.line_index >= line_count {
            position.line_index = line_count - 1;
        }

        let char_count = self.lines[position.line_index].char_count();
        if position.column_byte_index > char_count {
            position.column_byte_index = char_count;
        }

        position
    }

    fn insert_text(&mut self, position: BufferPosition, text: &str) -> BufferRange {
        let position = self.clamp_position(position);

        if let None = text.find('\n') {
            let line = &mut self.lines[position.line_index];
            let previous_char_count = line.char_count();
            line.insert_text(position.column_byte_index, text);
            let char_count_diff = line.char_count() - previous_char_count;

            let end_position = BufferPosition::line_col(
                position.line_index,
                position.column_byte_index + char_count_diff,
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
                let column_byte_index = line.char_count();
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
            let deleted_text = line.slice(range.clone());
            let text = Text::from(deleted_text);
            line.delete_range(range);

            text
        } else {
            let mut deleted_text = Text::new();

            let line = &mut self.lines[from.line_index];
            let delete_range = from.column_byte_index..;
            deleted_text.push_str(line.slice(delete_range.clone()));
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
                self.lines[from.line_index].push_text(&to_line.slice(to.column_byte_index..));
                deleted_text.push_str("\n");
                deleted_text.push_str(&to_line.slice(..to.column_byte_index));
            }

            deleted_text
        }
    }

    pub fn find_word_at(&self, position: BufferPosition) -> (BufferRange, &str) {
        let position = self.clamp_position(position);
        let (range, word) = self
            .line_at(position.line_index)
            .find_word_at(position.column_byte_index);
        let range = BufferRange::between(
            BufferPosition::line_col(position.line_index, range.start),
            BufferPosition::line_col(position.line_index, range.end),
        );
        (range, word)
    }

    pub fn find_balanced_chars_at(
        &self,
        position: BufferPosition,
        left: char,
        right: char,
    ) -> Option<BufferRange> {
        fn count_to_char<I>(iter: I, target: char, other: char) -> Option<usize>
        where
            I: Iterator<Item = char>,
        {
            let mut char_count = 0;
            let mut balance_count = 0;
            for c in iter {
                if c == target {
                    if balance_count == 0 {
                        return Some(char_count);
                    } else {
                        balance_count -= 1;
                    }
                } else if c == other {
                    balance_count += 1;
                }
                char_count += 1;
            }
            None
        }

        let position = self.clamp_position(position);
        let line = self.line_at(position.line_index);
        let split_index = line.column_to_index(position.column_byte_index);
        let (before, after) = line.as_str().split_at(split_index);

        let left_position = match count_to_char(before.chars().rev(), left, right) {
            Some(column_byte_index) => {
                let column_byte_index = position.column_byte_index - column_byte_index;
                BufferPosition::line_col(position.line_index, column_byte_index)
            }
            None => {
                let mut pos = None;
                for line_index in (0..(position.line_index.saturating_sub(1))).rev() {
                    let line = self.line_at(line_index);
                    if let Some(column_byte_index) =
                        count_to_char(line.as_str().chars().rev(), left, right)
                    {
                        let column_byte_index = line.char_count() - column_byte_index;
                        pos = Some(BufferPosition::line_col(line_index, column_byte_index));
                        break;
                    }
                }
                pos?
            }
        };

        let right_position = match count_to_char(after.chars(), right, left) {
            Some(column_byte_index) => {
                let column_byte_index = position.column_byte_index + column_byte_index;
                BufferPosition::line_col(position.line_index, column_byte_index)
            }
            None => {
                let mut pos = None;
                for line_index in (position.line_index + 1)..self.line_count() {
                    let line = self.line_at(line_index);
                    if let Some(column_byte_index) = count_to_char(line.as_str().chars(), left, right) {
                        let column_byte_index = line.char_count() - column_byte_index;
                        pos = Some(BufferPosition::line_col(line_index, column_byte_index));
                        break;
                    }
                }
                pos?
            }
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
}

impl Buffer {
    pub fn new(
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        path: PathBuf,
        content: BufferContent,
    ) -> Self {
        for line in content.lines() {
            for word in WordIter::new(line.as_str()) {
                word_database.add_word(word);
            }
        }

        let syntax_handle = syntaxes
            .find_handle_by_extension(syntax::get_path_extension(&path))
            .unwrap_or(SyntaxHandle::default());

        let mut highlighted = HighlightedBuffer::new();
        highlighted.highligh_all(syntaxes.get(syntax_handle), &content);

        Self {
            path,
            content,
            syntax_handle,
            highlighted,
            history: History::new(),
            search_ranges: Vec::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn set_path(&mut self, syntaxes: &SyntaxCollection, path: &Path) {
        self.path.clear();
        self.path.push(path);

        let syntax_handle = syntaxes
            .find_handle_by_extension(syntax::get_path_extension(&path))
            .unwrap_or(SyntaxHandle::default());
        if self.syntax_handle != syntax_handle {
            self.syntax_handle = syntax_handle;
            self.highlighted
                .highligh_all(syntaxes.get(self.syntax_handle), &self.content);
        }
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

        for word in WordIter::new(self.content.line_at(position.line_index).as_str()) {
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
            for word in WordIter::new(line.as_str()) {
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

        let line_count = range.to.line_index - range.from.line_index + 1;
        for line in self
            .content
            .lines()
            .skip(range.from.line_index)
            .take(line_count)
        {
            for word in WordIter::new(line.as_str()) {
                word_database.remove_word(word);
            }
        }

        let deleted_text = self.content.delete_range(range);

        for word in WordIter::new(self.content.line_at(range.from.line_index).as_str()) {
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

    pub fn search_ranges(&self) -> &[BufferRange] {
        &self.search_ranges
    }

    pub fn save_to_file(&self) -> Result<(), String> {
        if self.path.as_os_str().is_empty() {
            return Err("buffer has no path".into());
        }

        let mut file = File::create(&self.path)
            .map_err(|e| format!("could not create file {:?}: {:?}", &self.path, e))?;

        self.content
            .write(&mut file)
            .map_err(|e| format!("could not write to file {:?}: {:?}", &self.path, e))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BufferHandle(usize);

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

    pub fn iter_with_handles(&self) -> impl Iterator<Item = (BufferHandle, &Buffer)> {
        self.buffers
            .iter()
            .enumerate()
            .filter_map(|(i, b)| Some(BufferHandle(i)).zip(b.as_ref()))
    }

    pub fn remove_where<F>(&mut self, word_database: &mut WordDatabase, predicate: F)
    where
        F: Fn(BufferHandle, &Buffer) -> bool,
    {
        for i in 0..self.buffers.len() {
            if let Some(buffer) = &mut self.buffers[i] {
                let handle = BufferHandle(i);
                if predicate(handle, buffer) {
                    for line in buffer.content.lines() {
                        for word in WordIter::new(line.as_str()) {
                            word_database.remove_word(word);
                        }
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
            PathBuf::new(),
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
            PathBuf::new(),
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

    //#[test]
    fn utf8_support() {
        let mut line = BufferLine::new("0ñà".into());
        assert_eq!(3, line.char_count());
        line.delete_range(1..2);
        assert_eq!(2, line.char_count());
        line.push_text("éç");
        assert_eq!(4, line.char_count());
        line.insert_text(2, "è");
        assert_eq!(5, line.char_count());
        let other_line = line.split_off(3);
        assert_eq!("àè", line.slice(1..));
        assert_eq!("éç", other_line.as_str());
    }

    #[test]
    fn buffer_line_find_word() {
        let line = BufferLine::new("word".into());
        assert_eq!((0..4, "word"), line.find_word_at(0));
        assert_eq!((0..4, "word"), line.find_word_at(2));
        assert_eq!((4..4, ""), line.find_word_at(4));

        let line = BufferLine::new("asd word+? asd".into());
        assert_eq!((3..3, ""), line.find_word_at(3));
        assert_eq!((4..8, "word"), line.find_word_at(4));
        assert_eq!((4..8, "word"), line.find_word_at(6));
        assert_eq!((8..10, "+?"), line.find_word_at(8));
        assert_eq!((8..10, "+?"), line.find_word_at(9));
        assert_eq!((10..10, ""), line.find_word_at(10));
    }
}
