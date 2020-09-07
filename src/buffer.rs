use std::{
    fs::File,
    io,
    ops::{Bound, Range, RangeBounds},
    path::{Path, PathBuf},
};

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    history::{Edit, EditKind, EditRef, History},
    syntax::{self, HighlightedBuffer, SyntaxCollection, SyntaxHandle},
};

#[derive(Debug)]
pub enum Text {
    Char(char),
    String(String),
}

impl Text {
    pub fn from_chars<I>(mut chars: I) -> Self
    where
        I: Iterator<Item = char>,
    {
        if let Some(first_char) = chars.next() {
            if let Some(second_char) = chars.next() {
                let mut text = String::new();
                text.push(first_char);
                text.push(second_char);
                text.extend(chars);
                Text::String(text)
            } else {
                Text::Char(first_char)
            }
        } else {
            Text::String(String::new())
        }
    }

    pub fn as_text_ref(&self) -> TextRef {
        match self {
            Text::Char(c) => TextRef::Char(*c),
            Text::String(s) => TextRef::Str(&s[..]),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TextRef<'a> {
    Char(char),
    Str(&'a str),
}

impl<'a> TextRef<'a> {
    pub fn to_text(&self) -> Text {
        match self {
            TextRef::Char(c) => Text::Char(*c),
            TextRef::Str(s) => Text::String(s.to_string()),
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
        &self.text[..]
    }

    pub fn slice<R>(&self, range: R) -> &str
    where
        R: RangeBounds<usize>,
    {
        &self.text[self.column_range_to_index_range(range)]
    }

    pub fn split_off(&mut self, index: usize) -> BufferLine {
        let index = self.column_to_index(index);
        let splitted = BufferLine::new(self.text.split_off(index));
        self.sync_state();
        splitted
    }

    pub fn find_word_at<F>(&self, index: usize, mut predicate: F) -> (Range<usize>, &str)
    where
        F: FnMut(char) -> bool,
    {
        let index = self.column_to_index(index);

        let start_index = self.text[..index]
            .char_indices()
            .rev()
            .take_while(|(_i, c)| predicate(*c))
            .last()
            .map(|(i, _c)| i)
            .unwrap_or(index);

        let end_index = self.text[index..]
            .char_indices()
            .take_while(|(_i, c)| predicate(*c))
            .last()
            .map(|(i, _c)| i + index + 1)
            .unwrap_or(index);

        let index_range = start_index..end_index;
        let column_range = self.index_range_to_column_rangge(index_range.clone());
        (column_range, &self.text[index_range])
    }

    pub fn insert(&mut self, index: usize, c: char) {
        let index = self.column_to_index(index);
        self.text.insert(index, c);
        self.sync_state();
    }

    pub fn push_str(&mut self, s: &str) {
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
        this.insert_text(BufferPosition::line_col(0, 0), TextRef::Str(text));
        this
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn lines(&self) -> impl Iterator<Item = &BufferLine> {
        self.lines.iter()
    }

    pub fn line(&self, index: usize) -> &BufferLine {
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

    pub fn append_range_text_to_string(&self, range: BufferRange, text: &mut String) {
        let from = self.clamp_position(range.from);
        let to = self.clamp_position(range.to);

        if from.line_index == to.line_index {
            let range_text = &self.lines[from.line_index].slice(from.column_index..to.column_index);
            text.push_str(range_text);
        } else {
            text.push_str(&self.lines[from.line_index].slice(from.column_index..));
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
                text.push_str(&to_line.slice(..to.column_index));
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
        if position.column_index > char_count {
            position.column_index = char_count;
        }

        position
    }

    pub fn insert_text(&mut self, position: BufferPosition, text: TextRef) -> BufferRange {
        let position = self.clamp_position(position);

        let end_position = match text {
            TextRef::Char(c) => {
                if c == '\n' {
                    let split_line =
                        self.lines[position.line_index].split_off(position.column_index);
                    self.lines.insert(position.line_index + 1, split_line);

                    BufferPosition::line_col(position.line_index + 1, 0)
                } else {
                    self.lines[position.line_index].insert(position.column_index, c);
                    BufferPosition::line_col(position.line_index, position.column_index + 1)
                }
            }
            TextRef::Str(text) => {
                let split_line = self.lines[position.line_index].split_off(position.column_index);

                let mut line_count = 0;
                let mut lines = text.lines();
                if let Some(line) = lines.next() {
                    self.lines[position.line_index].push_str(&line[..]);
                }
                for line in lines {
                    line_count += 1;
                    self.lines.insert(
                        position.line_index + line_count,
                        BufferLine::new(line.into()),
                    );
                }

                if text.ends_with('\n') {
                    line_count += 1;
                    self.lines
                        .insert(position.line_index + line_count, split_line);

                    BufferPosition::line_col(position.line_index + line_count, 0)
                } else {
                    let line = &mut self.lines[position.line_index + line_count];
                    let column_index = line.char_count();
                    line.push_str(split_line.as_str());

                    BufferPosition::line_col(position.line_index + line_count, column_index)
                }
            }
        };

        BufferRange::between(position, end_position)
    }

    pub fn delete_range(&mut self, range: BufferRange) -> Text {
        let from = self.clamp_position(range.from);
        let to = self.clamp_position(range.to);

        if from.line_index == to.line_index {
            let line = &mut self.lines[from.line_index];
            let range = from.column_index..to.column_index;
            let deleted_chars = line.slice(range.clone()).chars();
            let text = Text::from_chars(deleted_chars);
            line.delete_range(range);
            text
        } else {
            let mut deleted_text = String::new();

            let line = &mut self.lines[from.line_index];
            let delete_range = from.column_index..;
            deleted_text.push_str(line.slice(delete_range.clone()));
            line.delete_range(delete_range);
            drop(line);

            let lines_range = (from.line_index + 1)..to.line_index;
            if lines_range.start < lines_range.end {
                for line in self.lines.drain(lines_range) {
                    deleted_text.push('\n');
                    deleted_text.push_str(line.as_str());
                }
            }
            let to_line_index = from.line_index + 1;
            if to_line_index < self.lines.len() {
                let to_line = self.lines.remove(to_line_index);
                self.lines[from.line_index].push_str(&to_line.slice(to.column_index..));
                deleted_text.push('\n');
                deleted_text.push_str(&to_line.slice(..to.column_index));
            }

            Text::String(deleted_text)
        }
    }

    pub fn find_word_at(&self, position: BufferPosition) -> (BufferRange, &str) {
        let position = self.clamp_position(position);
        let (range, word) = self
            .line(position.line_index)
            .find_word_at(position.column_index, |c| c.is_alphanumeric() || c == '_');
        let range = BufferRange::between(
            BufferPosition::line_col(position.line_index, range.start),
            BufferPosition::line_col(position.line_index, range.end),
        );
        (range, word)
    }
}

pub struct Buffer {
    path: PathBuf,
    pub content: BufferContent,
    syntax_handle: SyntaxHandle,
    pub highlighted: HighlightedBuffer,
    pub history: History,
    search_ranges: Vec<BufferRange>,
}

impl Buffer {
    pub fn new(syntaxes: &SyntaxCollection, path: PathBuf, content: BufferContent) -> Self {
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
        syntaxes: &SyntaxCollection,
        position: BufferPosition,
        text: TextRef,
    ) -> BufferRange {
        self.search_ranges.clear();
        let range = self.content.insert_text(position, text);
        self.highlighted
            .on_insert(syntaxes.get(self.syntax_handle), &self.content, range);
        self.history.push_edit(Edit {
            kind: EditKind::Insert,
            range,
            text: text.to_text(),
        });
        range
    }

    pub fn delete_range(&mut self, syntaxes: &SyntaxCollection, range: BufferRange) {
        self.search_ranges.clear();
        let deleted_text = self.content.delete_range(range);
        self.highlighted
            .on_delete(syntaxes.get(self.syntax_handle), &self.content, range);
        self.history.push_edit(Edit {
            kind: EditKind::Delete,
            range,
            text: deleted_text,
        });
    }

    pub fn undo<'a>(
        &'a mut self,
        syntaxes: &'a SyntaxCollection,
    ) -> impl 'a + Iterator<Item = EditRef<'a>> {
        self.history_edits(syntaxes, |h| h.undo_edits())
    }

    pub fn redo<'a>(
        &'a mut self,
        syntaxes: &SyntaxCollection,
    ) -> impl 'a + Iterator<Item = EditRef<'a>> {
        self.history_edits(syntaxes, |h| h.redo_edits())
    }

    fn history_edits<'a, F, I>(&'a mut self, syntaxes: &SyntaxCollection, selector: F) -> I
    where
        F: FnOnce(&'a mut History) -> I,
        I: 'a + Clone + Iterator<Item = EditRef<'a>>,
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
        &self.search_ranges[..]
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

    pub fn remove_where<F>(&mut self, predicate: F)
    where
        F: Fn(BufferHandle, &Buffer) -> bool,
    {
        for i in 0..self.buffers.len() {
            if let Some(buffer) = &self.buffers[i] {
                let handle = BufferHandle(i);
                if predicate(handle, &buffer) {
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
    fn buffer_content_insert_text() {
        let mut buffer = BufferContent::from_str("");

        assert_eq!(1, buffer.line_count());
        assert_eq!("", buffer_to_string(&buffer));

        buffer.insert_text(BufferPosition::line_col(0, 0), TextRef::Str("hold"));
        buffer.insert_text(BufferPosition::line_col(0, 2), TextRef::Char('r'));
        buffer.insert_text(BufferPosition::line_col(0, 1), TextRef::Str("ello w"));
        assert_eq!(1, buffer.line_count());
        assert_eq!("hello world", buffer_to_string(&buffer));

        buffer.insert_text(BufferPosition::line_col(0, 5), TextRef::Char('\n'));
        buffer.insert_text(
            BufferPosition::line_col(1, 6),
            TextRef::Str(" appending more\nand more\nand even more\nlines"),
        );
        assert_eq!(5, buffer.line_count());
        assert_eq!(
            "hello\n world appending more\nand more\nand even more\nlines",
            buffer_to_string(&buffer)
        );

        let mut buffer = BufferContent::from_str("this is content");
        buffer.insert_text(
            BufferPosition::line_col(0, 8),
            TextRef::Str("some\nmultiline "),
        );
        assert_eq!(2, buffer.line_count());
        assert_eq!("this is some\nmultiline content", buffer_to_string(&buffer));

        let mut buffer = BufferContent::from_str("this is content");
        buffer.insert_text(
            BufferPosition::line_col(0, 8),
            TextRef::Str("some\nmore\nextensive\nmultiline "),
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
        match deleted_text {
            Text::String(s) => assert_eq!("", s),
            Text::Char(_c) => unreachable!(),
        }

        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 11),
            BufferPosition::line_col(0, 19),
        ));
        assert_eq!(2, buffer.line_count());
        assert_eq!(
            "this is the\ncontent of the buffer",
            buffer_to_string(&buffer)
        );
        match deleted_text {
            Text::String(s) => assert_eq!(" initial", s),
            Text::Char(_c) => unreachable!(),
        }

        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 8),
            BufferPosition::line_col(1, 15),
        ));
        assert_eq!(1, buffer.line_count());
        assert_eq!("this is buffer", buffer_to_string(&buffer));
        match deleted_text {
            Text::String(s) => assert_eq!("the\ncontent of the ", s),
            Text::Char(_c) => unreachable!(),
        }

        let mut buffer = BufferContent::from_str("this\nbuffer\ncontains\nmultiple\nlines\nyes");
        assert_eq!(6, buffer.line_count());
        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 4),
            BufferPosition::line_col(4, 1),
        ));
        assert_eq!("this\nbuffines\nyes", buffer_to_string(&buffer));
        match deleted_text {
            Text::String(s) => assert_eq!("er\ncontains\nmultiple\nl", s),
            Text::Char(_c) => unreachable!(),
        }
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
        match deleted_text {
            Text::String(s) => assert_eq!("second line\n", s),
            Text::Char(_c) => unreachable!(),
        }

        let mut buffer = BufferContent::from_str("first line\nsecond line\nthird line");
        assert_eq!(3, buffer.line_count());
        let deleted_text = buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 0),
            BufferPosition::line_col(1, 11),
        ));
        assert_eq!("first line\n\nthird line", buffer_to_string(&buffer));
        match deleted_text {
            Text::String(s) => assert_eq!("second line", s),
            Text::Char(_c) => unreachable!(),
        }
    }

    #[test]
    fn buffer_delete_undo_redo_single_line() {
        let syntaxes = SyntaxCollection::new();

        let mut buffer = Buffer::new(
            &syntaxes,
            PathBuf::new(),
            BufferContent::from_str("single line content"),
        );
        let range = BufferRange::between(
            BufferPosition::line_col(0, 7),
            BufferPosition::line_col(0, 12),
        );
        buffer.delete_range(&syntaxes, range);

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
        let syntaxes = SyntaxCollection::new();

        let mut buffer = Buffer::new(
            &syntaxes,
            PathBuf::new(),
            BufferContent::from_str("multi\nline\ncontent"),
        );
        let range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(1, 3),
        );
        buffer.delete_range(&syntaxes, range);

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
    fn utf8_support() {
        let mut line = BufferLine::new("0ñà".into());
        assert_eq!(3, line.char_count());
        line.delete_range(1..2);
        assert_eq!(2, line.char_count());
        line.push_str("éç");
        assert_eq!(4, line.char_count());
        line.insert(2, 'è');
        assert_eq!(5, line.char_count());
        let other_line = line.split_off(3);
        assert_eq!("àè", line.slice(1..));
        assert_eq!("éç", other_line.as_str());
    }

    #[test]
    fn buffer_line_find_word() {
        fn is_word(c: char) -> bool {
            c.is_alphanumeric()
        }

        let line = BufferLine::new("word".into());
        assert_eq!((0..4, "word"), line.find_word_at(0, is_word));
        assert_eq!((0..4, "word"), line.find_word_at(2, is_word));
        assert_eq!((0..4, "word"), line.find_word_at(4, is_word));

        let line = BufferLine::new("asd word+? asd".into());
        assert_eq!((0..3, "asd"), line.find_word_at(3, is_word));
        assert_eq!((4..8, "word"), line.find_word_at(4, is_word));
        assert_eq!((4..8, "word"), line.find_word_at(6, is_word));
        assert_eq!((4..8, "word"), line.find_word_at(8, is_word));
        assert_eq!((9..9, ""), line.find_word_at(9, is_word));
        assert_eq!((10..10, ""), line.find_word_at(10, is_word));
    }
}
