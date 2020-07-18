use std::{
    io,
    path::{Path, PathBuf},
};

use serde_derive::{Deserialize, Serialize};

use crate::{
    buffer_position::{BufferPosition, BufferRange},
    history::{Edit, EditKind, EditRef, History},
};

#[derive(Debug, Serialize, Deserialize)]
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
    pub text: String,
}

impl BufferLine {
    pub fn new(text: String) -> Self {
        Self { text }
    }

    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}

pub struct BufferContent {
    lines: Vec<BufferLine>,
}

impl BufferContent {
    pub fn from_str(text: &str) -> Self {
        let mut this = Self { lines: Vec::new() };
        this.lines.push(BufferLine::new(String::new()));
        this.insert_text(BufferPosition::line_col(0, 0), TextRef::Str(text));
        this
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn lines_from(&self, start_index: usize) -> impl Iterator<Item = &BufferLine> {
        self.lines[start_index..].iter()
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
            writeln!(write, "{}", line.text)?;
        }
        write!(write, "{}", self.lines[last_index].text)?;
        Ok(())
    }

    pub fn append_range_to_string(&self, mut range: BufferRange, text: &mut String) {
        self.clamp_position(&mut range.from);
        self.clamp_position(&mut range.to);

        if range.from.line_index == range.to.line_index {
            let range_text = &self.lines[range.from.line_index].text
                [range.from.column_index..range.to.column_index];
            text.push_str(range_text);
        } else {
            text.push_str(&self.lines[range.from.line_index].text[range.from.column_index..]);
            let lines_range = (range.from.line_index + 1)..range.to.line_index;
            if lines_range.start < lines_range.end {
                for line in &self.lines[lines_range] {
                    text.push('\n');
                    text.push_str(&line.text[..]);
                }
            }
            let to_line_index = range.from.line_index + 1;
            if to_line_index < self.lines.len() {
                let to_line = &self.lines[to_line_index];
                text.push('\n');
                text.push_str(&to_line.text[..range.to.column_index]);
            }
        }
    }

    pub fn find_search_ranges(&self, text: &str, ranges: &mut Vec<BufferRange>) {
        let char_count = text.chars().count();
        if char_count == 0 {
            return;
        }

        for (i, line) in self.lines.iter().enumerate() {
            for (j, _) in line.text.match_indices(text) {
                ranges.push(BufferRange::between(
                    BufferPosition::line_col(i, j),
                    BufferPosition::line_col(i, j + char_count - 1),
                ));
            }
        }
    }

    fn clamp_position(&self, position: &mut BufferPosition) {
        let line_count = self.line_count();
        if position.line_index >= line_count {
            position.line_index = line_count - 1;
        }

        let char_count = self.lines[position.line_index].char_count();
        if position.column_index > char_count {
            position.column_index = char_count;
        }
    }

    pub fn insert_text(&mut self, mut position: BufferPosition, text: TextRef) -> BufferRange {
        self.clamp_position(&mut position);

        let end_position = match text {
            TextRef::Char(c) => {
                if c == '\n' {
                    let split_line = self.lines[position.line_index]
                        .text
                        .split_off(position.column_index);
                    self.lines
                        .insert(position.line_index + 1, BufferLine::new(split_line));

                    BufferPosition::line_col(position.line_index + 1, 0)
                } else {
                    self.lines[position.line_index]
                        .text
                        .insert(position.column_index, c);

                    BufferPosition::line_col(position.line_index, position.column_index + 1)
                }
            }
            TextRef::Str(text) => {
                let split_line = self.lines[position.line_index]
                    .text
                    .split_off(position.column_index);

                let mut line_count = 0;
                let mut lines = text.lines();
                if let Some(line) = lines.next() {
                    self.lines[position.line_index].text.push_str(&line[..]);
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
                    self.lines.insert(
                        position.line_index + line_count,
                        BufferLine::new(split_line),
                    );

                    BufferPosition::line_col(position.line_index + line_count, 0)
                } else {
                    let line = &mut self.lines[position.line_index + line_count];
                    let column_index = line.char_count();
                    line.text.push_str(&split_line[..]);

                    BufferPosition::line_col(position.line_index + line_count, column_index)
                }
            }
        };

        BufferRange::between(position, end_position)
    }

    pub fn delete_range(&mut self, mut range: BufferRange) -> Text {
        self.clamp_position(&mut range.from);
        self.clamp_position(&mut range.to);

        if range.from.line_index == range.to.line_index {
            let deleted_chars = self.lines[range.from.line_index]
                .text
                .drain(range.from.column_index..range.to.column_index);
            Text::from_chars(deleted_chars)
        } else {
            let mut deleted_text = String::new();
            deleted_text.extend(
                self.lines[range.from.line_index]
                    .text
                    .drain(range.from.column_index..),
            );
            let lines_range = (range.from.line_index + 1)..range.to.line_index;
            if lines_range.start < lines_range.end {
                for line in self.lines.drain(lines_range) {
                    deleted_text.push('\n');
                    deleted_text.push_str(&line.text[..]);
                }
            }
            let to_line_index = range.from.line_index + 1;
            if to_line_index < self.lines.len() {
                let to_line = self.lines.remove(to_line_index);
                self.lines[range.from.line_index]
                    .text
                    .push_str(&to_line.text[range.to.column_index..]);
                deleted_text.push('\n');
                deleted_text.push_str(&to_line.text[..range.to.column_index]);
            }

            Text::String(deleted_text)
        }
    }

    fn apply_edits<'a, I: 'a>(&'a mut self, edits: I) -> impl 'a + Iterator<Item = EditRef<'a>>
    where
        I: Iterator<Item = EditRef<'a>>,
    {
        edits.map(move |e| match e.kind {
            EditKind::Insert => {
                self.insert_text(e.range.from, e.text);
                e
            }
            EditKind::Delete => {
                self.delete_range(e.range);
                e
            }
        })
    }
}

pub struct Buffer {
    pub path: Option<PathBuf>,
    pub content: BufferContent,
    pub history: History,
    search_ranges: Vec<BufferRange>,
}

impl Buffer {
    pub fn new(path: Option<PathBuf>, content: BufferContent) -> Self {
        Self {
            path,
            content,
            history: History::new(),
            search_ranges: Vec::new(),
        }
    }

    pub fn insert_text(&mut self, position: BufferPosition, text: TextRef) -> BufferRange {
        let range = self.content.insert_text(position, text);
        self.history.push_edit(Edit {
            kind: EditKind::Insert,
            range,
            text: text.to_text(),
        });
        range
    }

    pub fn delete_range(&mut self, range: BufferRange) {
        let deleted_text = self.content.delete_range(range);
        self.history.push_edit(Edit {
            kind: EditKind::Delete,
            range,
            text: deleted_text,
        });
    }

    pub fn undo<'a>(&'a mut self) -> impl 'a + Iterator<Item = EditRef<'a>> {
        self.content.apply_edits(self.history.undo_edits())
    }

    pub fn redo<'a>(&'a mut self) -> impl 'a + Iterator<Item = EditRef<'a>> {
        self.content.apply_edits(self.history.redo_edits())
    }

    pub fn set_search(&mut self, text: &str) {
        self.search_ranges.clear();
        self.content
            .find_search_ranges(text, &mut self.search_ranges);
    }

    pub fn search_ranges(&self) -> &[BufferRange] {
        &self.search_ranges[..]
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
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
        for (handle, buffer) in self.iter_with_handles() {
            if let Some(ref buffer_path) = buffer.path {
                if buffer_path == path {
                    return Some(handle);
                }
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
            .filter_map(|(i, b)| match b {
                Some(b) => Some((BufferHandle(i), b)),
                None => None,
            })
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
        let mut buffer = Buffer::new(None, BufferContent::from_str("single line content"));
        let range = BufferRange::between(
            BufferPosition::line_col(0, 7),
            BufferPosition::line_col(0, 12),
        );
        buffer.delete_range(range);

        assert_eq!("single content", buffer_to_string(&buffer.content));
        {
            let mut ranges = buffer.undo();
            assert_eq!(range, ranges.next().unwrap().range);
            assert!(ranges.next().is_none());
        }
        assert_eq!("single line content", buffer_to_string(&buffer.content));
        for _ in buffer.redo() {}
        assert_eq!("single content", buffer_to_string(&buffer.content));
    }

    #[test]
    fn buffer_delete_undo_redo_multi_line() {
        let mut buffer = Buffer::new(None, BufferContent::from_str("multi\nline\ncontent"));
        let range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(1, 3),
        );
        buffer.delete_range(range);

        assert_eq!("me\ncontent", buffer_to_string(&buffer.content));
        {
            let mut ranges = buffer.undo();
            assert_eq!(range, ranges.next().unwrap().range);
            assert!(ranges.next().is_none());
        }
        assert_eq!("multi\nline\ncontent", buffer_to_string(&buffer.content));
        for _ in buffer.redo() {}
        assert_eq!("me\ncontent", buffer_to_string(&buffer.content));
    }
}
