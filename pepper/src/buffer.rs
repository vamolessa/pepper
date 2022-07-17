use std::{
    fmt,
    fs::File,
    io,
    ops::{Add, Range, RangeBounds, Sub},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    str::CharIndices,
};

use crate::{
    buffer_history::{BufferHistory, Edit, EditKind},
    buffer_position::{BufferPosition, BufferPositionIndex, BufferRange},
    cursor::Cursor,
    editor_utils::{find_delimiter_pair_at, ResidualStrBytes},
    events::{
        BufferRangeDeletesMutGuard, BufferTextInsertsMutGuard, EditorEvent, EditorEventTextInsert,
        EditorEventWriter,
    },
    help,
    pattern::Pattern,
    platform::{Platform, PlatformProcessHandle, PlatformRequest, PooledBuf, ProcessTag},
    plugin::PluginHandle,
    syntax::{HighlightResult, HighlightedBuffer, SyntaxCollection, SyntaxHandle},
    word_database::{WordDatabase, WordIter, WordKind},
};

// TODO: parse unicode database and implement this
pub fn char_display_len(_: char) -> u8 {
    1
}

#[derive(Clone, Copy)]
pub struct DisplayLen {
    pub len: u32,
    pub tab_count: u32,
}
impl DisplayLen {
    pub const fn zero() -> Self {
        Self {
            len: 0,
            tab_count: 0,
        }
    }

    pub fn total_len(&self, tab_size: u8) -> usize {
        self.len as usize + self.tab_count as usize * tab_size as usize
    }
}
impl<'a> From<&'a str> for DisplayLen {
    fn from(s: &'a str) -> Self {
        let mut len = 0;
        let mut tab_count = 0;
        for c in s.chars() {
            match c {
                '\t' => tab_count += 1,
                _ => len += char_display_len(c) as u32,
            }
        }
        Self { len, tab_count }
    }
}
impl Add for DisplayLen {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            len: self.len + other.len,
            tab_count: self.tab_count + other.tab_count,
        }
    }
}
impl Sub for DisplayLen {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self {
            len: self.len - other.len,
            tab_count: self.tab_count - other.tab_count,
        }
    }
}

pub struct CharDisplayDistance {
    pub distance: u32,
    pub char: char,
    pub char_index: u32,
}
pub struct CharDisplayDistances<'a> {
    char_indices: CharIndices<'a>,
    len: u32,
    tab_size: u8,
}
impl<'a> CharDisplayDistances<'a> {
    pub fn new(text: &'a str, tab_size: u8) -> Self {
        Self {
            char_indices: text.char_indices(),
            len: 0,
            tab_size,
        }
    }
}
impl<'a> CharDisplayDistances<'a> {
    fn calc_next(&mut self, char_index: usize, c: char) -> CharDisplayDistance {
        self.len += match c {
            '\t' => self.tab_size as u32,
            _ => char_display_len(c) as u32,
        };
        CharDisplayDistance {
            distance: self.len,
            char: c,
            char_index: char_index as _,
        }
    }
}
impl<'a> Iterator for CharDisplayDistances<'a> {
    type Item = CharDisplayDistance;
    fn next(&mut self) -> Option<Self::Item> {
        let (i, c) = self.char_indices.next()?;
        Some(self.calc_next(i, c))
    }
}
impl<'a> DoubleEndedIterator for CharDisplayDistances<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let (i, c) = self.char_indices.next_back()?;
        Some(self.calc_next(i, c))
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
            position: BufferPosition::line_col(line_index as _, self.index as _),
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
            self.position.column_byte_index + self.text.len() as BufferPositionIndex,
        )
    }
}

pub struct BufferLint {
    pub message_range: Range<u32>,
    pub range: BufferRange,
    pub plugin_handle: PluginHandle,
}
impl BufferLint {
    pub fn message<'a>(&self, buffer_lints: &'a BufferLintCollection) -> &'a str {
        let message_range = self.message_range.start as usize..self.message_range.end as usize;
        let plugin_messages = &buffer_lints.plugin_messages[self.plugin_handle.0 as usize];
        &plugin_messages[message_range]
    }
}

#[derive(Default)]
pub struct BufferLintCollection {
    lints: Vec<BufferLint>,
    plugin_messages: Vec<String>,
}
impl BufferLintCollection {
    pub fn all(&self) -> &[BufferLint] {
        &self.lints
    }

    fn insert_range(&mut self, range: BufferRange) {
        for lint in &mut self.lints {
            lint.range.from = lint.range.from.insert(range);
            lint.range.to = lint.range.to.insert(range);
        }
    }

    fn delete_range(&mut self, range: BufferRange) {
        for lint in &mut self.lints {
            lint.range.from = lint.range.from.delete(range);
            lint.range.to = lint.range.to.delete(range);
        }
    }

    pub fn mut_guard(&mut self, plugin_handle: PluginHandle) -> BufferLintCollectionMutGuard {
        let min_messages_per_plugin_len = plugin_handle.0 as usize + 1;
        if self.plugin_messages.len() < min_messages_per_plugin_len {
            self.plugin_messages
                .resize(min_messages_per_plugin_len, String::new());
        }
        BufferLintCollectionMutGuard {
            inner: self,
            plugin_handle,
        }
    }
}

pub struct BufferLintCollectionMutGuard<'a> {
    inner: &'a mut BufferLintCollection,
    plugin_handle: PluginHandle,
}
impl<'a> BufferLintCollectionMutGuard<'a> {
    pub fn clear(&mut self) {
        self.inner.plugin_messages[self.plugin_handle.0 as usize].clear();
        for i in (0..self.inner.lints.len()).rev() {
            if self.inner.lints[i].plugin_handle == self.plugin_handle {
                self.inner.lints.swap_remove(i);
            }
        }
    }

    pub fn add(&mut self, message: &str, range: BufferRange) {
        let plugin_messages = &mut self.inner.plugin_messages[self.plugin_handle.0 as usize];
        let message_start = plugin_messages.len() as _;
        plugin_messages.push_str(message);
        let message_end = plugin_messages.len() as _;

        self.inner.lints.push(BufferLint {
            message_range: message_start..message_end,
            range,
            plugin_handle: self.plugin_handle,
        });
    }
}
impl<'a> Drop for BufferLintCollectionMutGuard<'a> {
    fn drop(&mut self) {
        self.inner.lints.sort_unstable_by_key(|l| l.range.from);
    }
}

#[derive(Clone, Copy)]
pub struct BufferBreakpoint {
    pub line_index: BufferPositionIndex,
}

#[derive(Default)]
pub struct BufferBreakpointCollection {
    breakpoints: Vec<BufferBreakpoint>,
}
impl BufferBreakpointCollection {
    fn insert_range(&mut self, range: BufferRange) -> bool {
        let line_count = range.to.line_index - range.from.line_index;
        if line_count == 0 {
            return false;
        }

        let mut changed = false;
        for breakpoint in &mut self.breakpoints {
            if range.from.line_index < breakpoint.line_index {
                breakpoint.line_index += line_count;
                changed = true;
            } else {
                break;
            }
        }

        return changed;
    }

    fn delete_range(&mut self, range: BufferRange) -> bool {
        let line_count = range.to.line_index - range.from.line_index;
        if line_count == 0 {
            return false;
        }

        let mut changed = false;
        let mut removed_breakpoint = false;
        for i in (0..self.breakpoints.len()).rev() {
            let breakpoint_line_index = self.breakpoints[i].line_index;
            if range.to.line_index < breakpoint_line_index {
                changed = true;
                self.breakpoints[i].line_index -= line_count;
            } else if range.from.line_index < breakpoint_line_index
                || range.from.line_index == breakpoint_line_index
                    && range.from.column_byte_index == 0
            {
                self.breakpoints.swap_remove(i);
                changed = true;
                removed_breakpoint = true;
            } else {
                break;
            }
        }

        if removed_breakpoint {
            self.breakpoints.sort_unstable_by_key(|b| b.line_index);
        }

        return changed;
    }
}

pub struct BufferBreakpointMutCollection<'a> {
    inner: &'a mut BufferBreakpointCollection,
    buffer_handle: BufferHandle,
}
impl<'a> BufferBreakpointMutCollection<'a> {
    pub fn clear(&mut self, events: &mut EditorEventWriter) {
        if self.inner.breakpoints.len() > 0 {
            events.enqueue(EditorEvent::BufferBreakpointsChanged {
                handle: self.buffer_handle,
            });
        }
        self.inner.breakpoints.clear();
    }

    pub fn remove_under_cursors(&mut self, cursors: &[Cursor], events: &mut EditorEventWriter) {
        let previous_breakpoints_len = self.inner.breakpoints.len();

        let mut breakpoint_index = self.inner.breakpoints.len().saturating_sub(1);
        'cursors_loop: for cursor in cursors.iter().rev() {
            let range = cursor.to_range();

            loop {
                if self.inner.breakpoints.is_empty() {
                    break 'cursors_loop;
                }

                let breakpoint_line_index = self.inner.breakpoints[breakpoint_index].line_index;
                if breakpoint_line_index < range.from.line_index {
                    break;
                }

                if breakpoint_line_index <= range.to.line_index {
                    self.inner.breakpoints.swap_remove(breakpoint_index);
                }

                if breakpoint_index == 0 {
                    break 'cursors_loop;
                }
                breakpoint_index -= 1;
            }
        }

        if self.inner.breakpoints.len() < previous_breakpoints_len {
            events.enqueue(EditorEvent::BufferBreakpointsChanged {
                handle: self.buffer_handle,
            });
        }
    }

    pub fn toggle_under_cursors(&mut self, cursors: &[Cursor], events: &mut EditorEventWriter) {
        let mut last_line_index = BufferPositionIndex::MAX;
        for cursor in cursors {
            let range = cursor.to_range();

            let mut from_line_index = range.from.line_index;
            from_line_index += (from_line_index == last_line_index) as BufferPositionIndex;
            let to_line_index = range.to.line_index;

            for line_index in from_line_index..=to_line_index {
                self.inner.breakpoints.push(BufferBreakpoint { line_index });
            }

            last_line_index = to_line_index;
        }

        self.inner
            .breakpoints
            .sort_unstable_by_key(|b| b.line_index);

        self.inner.breakpoints.push(BufferBreakpoint {
            line_index: BufferPositionIndex::MAX,
        });
        let breakpoints = &mut self.inner.breakpoints[..];
        let mut write_needle = 0;
        let mut check_needle = 0;

        let breakpoints_len = breakpoints.len() - 1;
        while check_needle < breakpoints_len {
            let left_breakpoint_line_index = breakpoints[check_needle].line_index;
            if left_breakpoint_line_index == breakpoints[check_needle + 1].line_index {
                check_needle += 2;
            } else {
                breakpoints[write_needle].line_index = left_breakpoint_line_index;
                check_needle += 1;
                write_needle += 1;
            }
        }

        self.inner.breakpoints.truncate(write_needle);

        events.enqueue(EditorEvent::BufferBreakpointsChanged {
            handle: self.buffer_handle,
        });
    }
}

struct BufferLinePool {
    pool: Vec<BufferLine>,
}

impl BufferLinePool {
    pub const fn new() -> Self {
        Self { pool: Vec::new() }
    }

    pub fn acquire(&mut self) -> BufferLine {
        match self.pool.pop() {
            Some(mut line) => {
                line.0.clear();
                line
            }
            None => BufferLine::new(),
        }
    }

    pub fn release(&mut self, line: BufferLine) {
        self.pool.push(line);
    }
}

pub struct BufferLine(String);

impl BufferLine {
    fn new() -> Self {
        Self(String::new())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn chars_from(
        &self,
        index: usize,
    ) -> (
        impl '_ + Iterator<Item = (usize, char)>,
        impl '_ + Iterator<Item = (usize, char)>,
    ) {
        let (left, right) = self.0.split_at(index);
        let left_chars = left.char_indices().rev();
        let right_chars = right.char_indices().map(move |(i, c)| (index + i, c));
        (left_chars, right_chars)
    }

    pub fn words_from(
        &self,
        index: usize,
    ) -> (
        WordRefWithIndex,
        impl Iterator<Item = WordRefWithIndex>,
        impl Iterator<Item = WordRefWithIndex>,
    ) {
        let mid_word = self.word_at(index);
        let mid_start_index = mid_word.index;
        let mid_end_index = mid_start_index + mid_word.text.len();

        let left = &self.0[..mid_start_index];
        let right = &self.0[mid_end_index..];

        let mut left_column_index = mid_start_index;
        let left_words = WordIter(left).rev().map(move |w| {
            left_column_index -= w.text.len();
            WordRefWithIndex {
                kind: w.kind,
                text: w.text,
                index: left_column_index,
            }
        });

        let mut right_column_index = mid_end_index;
        let right_words = WordIter(right).map(move |w| {
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
        let (before, after) = self.0.split_at(index);
        match WordIter(after).next() {
            Some(right) => match WordIter(before).next_back() {
                Some(left) => {
                    if left.kind == right.kind {
                        let end_index = index + right.text.len();
                        let index = index - left.text.len();
                        WordRefWithIndex {
                            kind: left.kind,
                            text: &self.0[index..end_index],
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

    pub fn split_off(
        &mut self,
        self_display_len: &mut DisplayLen,
        other: &mut BufferLine,
        other_display_len: &mut DisplayLen,
        index: usize,
    ) {
        other.0.clear();
        other.0.push_str(&self.0[index..]);

        if index < other.0.len() {
            let display_len = DisplayLen::from(&self.0[..index]);
            *other_display_len = *self_display_len - display_len;
            *self_display_len = display_len;
        } else {
            *other_display_len = DisplayLen::from(&other.0[..]);
            *self_display_len = *self_display_len - *other_display_len;
        }

        self.0.truncate(index);
    }

    pub fn insert_text(&mut self, display_len: &mut DisplayLen, index: usize, text: &str) {
        self.0.insert_str(index, text);
        *display_len = *display_len + DisplayLen::from(text);
    }

    pub fn push_text(&mut self, display_len: &mut DisplayLen, text: &str) {
        self.0.push_str(text);
        *display_len = *display_len + DisplayLen::from(text);
    }

    pub fn delete_range<R>(&mut self, display_len: &mut DisplayLen, range: R)
    where
        R: RangeBounds<usize>,
    {
        let deleted = self.0.drain(range);
        *display_len = *display_len - DisplayLen::from(deleted.as_str());
    }
}

pub struct TextRangeIter<'a> {
    content: &'a BufferContent,
    from: BufferPosition,
    to: BufferPosition,
}
impl<'a> Iterator for TextRangeIter<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        if self.from == self.to {
            return None;
        }

        let line = self.content.lines[self.from.line_index as usize].as_str();

        if self.from.column_byte_index == line.len() as _ {
            self.from.line_index += 1;
            self.from.column_byte_index = 0;
            Some("\n")
        } else if self.from.line_index == self.to.line_index {
            let text =
                &line[self.from.column_byte_index as usize..self.to.column_byte_index as usize];
            self.from = self.to;
            Some(text)
        } else {
            let text = &line[self.from.column_byte_index as usize..];
            self.from.column_byte_index = line.len() as _;
            Some(text)
        }
    }
}

pub struct BufferContent {
    lines: Vec<BufferLine>,
    line_display_lens: Vec<DisplayLen>,
    line_pool: BufferLinePool,
}

impl BufferContent {
    pub fn new() -> Self {
        Self {
            lines: vec![BufferLine::new()],
            line_display_lens: vec![DisplayLen::zero()],
            line_pool: BufferLinePool::new(),
        }
    }

    pub fn lines(&self) -> &[BufferLine] {
        &self.lines
    }

    pub fn line_display_lens(&self) -> &[DisplayLen] {
        &self.line_display_lens
    }

    pub fn end(&self) -> BufferPosition {
        let last_line_index = self.lines.len() - 1;
        BufferPosition::line_col(
            last_line_index as _,
            self.lines[last_line_index].as_str().len() as _,
        )
    }

    pub fn read<R>(&mut self, read: &mut R) -> io::Result<()>
    where
        R: io::BufRead,
    {
        for line in self.lines.drain(..) {
            self.line_pool.release(line);
        }
        self.line_display_lens.clear();

        loop {
            let mut line = self.line_pool.acquire();
            match read.read_line(&mut line.0) {
                Ok(0) => {
                    self.line_pool.release(line);
                    break;
                }
                Ok(_) => {
                    if line.0.ends_with('\n') {
                        line.0.pop();
                    }
                    if line.0.ends_with('\r') {
                        line.0.pop();
                    }
                    let display_len = DisplayLen::from(&line.0[..]);

                    self.lines.push(line);
                    self.line_display_lens.push(display_len);
                }
                Err(e) => {
                    for line in self.lines.drain(..) {
                        self.line_pool.release(line);
                    }
                    self.lines.push(self.line_pool.acquire());
                    self.line_display_lens.clear();
                    self.line_display_lens.push(DisplayLen::zero());
                    return Err(e);
                }
            }
        }

        if self.lines.is_empty() {
            self.lines.push(self.line_pool.acquire());
            self.line_display_lens.push(DisplayLen::zero());
        }

        let byte_order_mark = b"\xef\xbb\xbf";
        if self.lines[0]
            .as_str()
            .as_bytes()
            .starts_with(byte_order_mark)
        {
            self.lines[0].delete_range(&mut self.line_display_lens[0], ..byte_order_mark.len());
        }

        Ok(())
    }

    pub fn write<W>(&self, write: &mut W) -> io::Result<()>
    where
        W: io::Write,
    {
        for line in &self.lines {
            write!(write, "{}\n", line.as_str())?;
        }
        Ok(())
    }

    pub fn saturate_position(&self, mut position: BufferPosition) -> BufferPosition {
        position.line_index = position.line_index.min((self.lines.len() - 1) as _);
        let line = self.lines[position.line_index as usize].as_str();
        position.column_byte_index = position.column_byte_index.min(line.len() as _);
        position
    }

    pub fn text_range(&self, range: BufferRange) -> TextRangeIter {
        let from = self.saturate_position(range.from);
        let to = self.saturate_position(range.to);
        TextRangeIter {
            content: self,
            from,
            to,
        }
    }

    pub fn find_search_ranges(&self, pattern: &Pattern, ranges: &mut Vec<BufferRange>) {
        if pattern.is_empty() {
            return;
        }
        let search_anchor = pattern.search_anchor();
        for (line_index, line) in self.lines.iter().enumerate() {
            let line = line.as_str();
            for range in pattern.match_indices(line, search_anchor) {
                let from = BufferPosition::line_col(line_index as _, range.start as _);
                let to = BufferPosition::line_col(line_index as _, range.end as _);
                ranges.push(BufferRange::between(from, to));
            }
        }
    }

    pub fn insert_text(&mut self, position: BufferPosition, text: &str) -> BufferRange {
        if !text.contains(&['\n', '\r'][..]) {
            let line = &mut self.lines[position.line_index as usize];
            let display_len = &mut self.line_display_lens[position.line_index as usize];

            let previous_len = line.as_str().len();
            line.insert_text(display_len, position.column_byte_index as _, text);
            let len_diff = line.as_str().len() - previous_len;

            let end_position = BufferPosition::line_col(
                position.line_index,
                position.column_byte_index + len_diff as BufferPositionIndex,
            );
            BufferRange::between(position, end_position)
        } else {
            let mut split_line = self.line_pool.acquire();
            let mut split_display_len = DisplayLen::zero();

            let position_line = &mut self.lines[position.line_index as usize];
            let position_display_len = &mut self.line_display_lens[position.line_index as usize];

            position_line.split_off(
                position_display_len,
                &mut split_line,
                &mut split_display_len,
                position.column_byte_index as _,
            );

            let mut line_count = 0 as BufferPositionIndex;
            let mut lines = text.lines();
            if let Some(line) = lines.next() {
                position_line.push_text(position_display_len, line);
            }
            for line_text in lines {
                line_count += 1;

                let mut line = self.line_pool.acquire();
                let mut display_len = DisplayLen::zero();
                line.push_text(&mut display_len, line_text);

                let insert_index = (position.line_index + line_count) as _;
                self.lines.insert(insert_index, line);
                self.line_display_lens.insert(insert_index, display_len);
            }

            let end_position = if text.ends_with('\n') {
                line_count += 1;

                let insert_index = (position.line_index + line_count) as _;
                self.lines.insert(insert_index, split_line);
                self.line_display_lens
                    .insert(insert_index, split_display_len);

                BufferPosition::line_col(position.line_index + line_count, 0)
            } else {
                let index = (position.line_index + line_count) as usize;
                let line = &mut self.lines[index];
                let display_len = &mut self.line_display_lens[index];

                let column_byte_index = line.as_str().len() as _;
                line.push_text(display_len, split_line.as_str());

                self.line_pool.release(split_line);
                BufferPosition::line_col(position.line_index + line_count, column_byte_index)
            };

            BufferRange::between(position, end_position)
        }
    }

    pub fn delete_range(&mut self, range: BufferRange) {
        let from = range.from;
        let to = range.to;

        if from.line_index == to.line_index {
            let line = &mut self.lines[from.line_index as usize];
            let display_len = &mut self.line_display_lens[from.line_index as usize];

            line.delete_range(
                display_len,
                from.column_byte_index as usize..to.column_byte_index as usize,
            );
        } else {
            let from_line = &mut self.lines[from.line_index as usize];
            let from_display_len = &mut self.line_display_lens[from.line_index as usize];
            from_line.delete_range(from_display_len, from.column_byte_index as usize..);

            let lines_range = (from.line_index as usize + 1)..to.line_index as usize;
            if lines_range.start < lines_range.end {
                for line in self.lines.drain(lines_range.clone()) {
                    self.line_pool.release(line);
                }
                self.line_display_lens.drain(lines_range);
            }

            let to_line_index = from.line_index as usize + 1;
            if to_line_index < self.lines.len() {
                let to_line = self.lines.remove(to_line_index);
                self.line_display_lens.remove(to_line_index);

                let from_line = &mut self.lines[from.line_index as usize];
                let from_display_len = &mut self.line_display_lens[from.line_index as usize];

                from_line.push_text(
                    from_display_len,
                    &to_line.as_str()[to.column_byte_index as usize..],
                );
            }
        }
    }

    pub fn clear(&mut self) {
        for line in self.lines.drain(..) {
            self.line_pool.release(line);
        }
        self.lines.push(self.line_pool.acquire());
        self.line_display_lens.clear();
        self.line_display_lens.push(DisplayLen::zero());
    }

    pub fn words_from(
        &self,
        position: BufferPosition,
    ) -> (
        WordRefWithPosition,
        impl Iterator<Item = WordRefWithPosition>,
        impl Iterator<Item = WordRefWithPosition>,
    ) {
        let position = self.saturate_position(position);
        let line_index = position.line_index as _;
        let column_byte_index = position.column_byte_index as _;

        let (mid_word, left_words, right_words) =
            self.lines[line_index as usize].words_from(column_byte_index);

        (
            mid_word.to_word_ref_with_position(line_index),
            left_words.map(move |w| w.to_word_ref_with_position(line_index)),
            right_words.map(move |w| w.to_word_ref_with_position(line_index)),
        )
    }

    pub fn word_at(&self, position: BufferPosition) -> WordRefWithPosition {
        let position = self.saturate_position(position);
        self.lines[position.line_index as usize]
            .word_at(position.column_byte_index as _)
            .to_word_ref_with_position(position.line_index as _)
    }

    pub fn position_before(&self, mut position: BufferPosition) -> BufferPosition {
        position.column_byte_index = self.lines[position.line_index as usize].as_str()
            [..position.column_byte_index as usize]
            .char_indices()
            .next_back()
            .map(|(i, _)| i as _)
            .unwrap_or(0);
        position
    }

    pub fn find_delimiter_pair_at(
        &self,
        position: BufferPosition,
        delimiter: char,
    ) -> Option<BufferRange> {
        let position = self.saturate_position(position);
        let line = self.lines[position.line_index as usize].as_str();
        let range = find_delimiter_pair_at(line, position.column_byte_index as _, delimiter)?;
        Some(BufferRange::between(
            BufferPosition::line_col(position.line_index, range.0 as _),
            BufferPosition::line_col(position.line_index, range.1 as _),
        ))
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

        let position = self.saturate_position(position);
        let line = self.lines[position.line_index as usize].as_str();
        let (before, after) = line.split_at(position.column_byte_index as _);

        let mut balance = 0;

        let mut left_position = None;
        let mut right_position = None;

        let mut after_chars = after.char_indices();
        if let Some((i, c)) = after_chars.next() {
            if c == left {
                left_position = Some(position.column_byte_index as usize + i + c.len_utf8());
            } else if c == right {
                right_position = Some(position.column_byte_index as usize + i);
            }
        }

        let right_position = match right_position {
            Some(column_index) => BufferPosition::line_col(position.line_index, column_index as _),
            None => match find(after_chars, right, left, &mut balance) {
                Some(column_byte_index) => {
                    let column_byte_index = position.column_byte_index as usize + column_byte_index;
                    BufferPosition::line_col(position.line_index, column_byte_index as _)
                }
                None => {
                    let mut pos = None;
                    for line_index in (position.line_index as usize + 1)..self.lines.len() {
                        let line = self.lines[line_index].as_str();
                        if let Some(column_byte_index) =
                            find(line.char_indices(), right, left, &mut balance)
                        {
                            pos = Some(BufferPosition::line_col(
                                line_index as _,
                                column_byte_index as _,
                            ));
                            break;
                        }
                    }
                    pos?
                }
            },
        };

        balance = 0;

        let left_position = match left_position {
            Some(column_index) => BufferPosition::line_col(position.line_index, column_index as _),
            None => match find(before.char_indices().rev(), left, right, &mut balance) {
                Some(column_byte_index) => {
                    let column_byte_index = column_byte_index + left.len_utf8();
                    BufferPosition::line_col(position.line_index, column_byte_index as _)
                }
                None => {
                    let mut pos = None;
                    for line_index in (0..position.line_index).rev() {
                        let line = self.lines[line_index as usize].as_str();
                        if let Some(column_byte_index) =
                            find(line.char_indices().rev(), left, right, &mut balance)
                        {
                            let column_byte_index = column_byte_index + left.len_utf8();
                            pos =
                                Some(BufferPosition::line_col(line_index, column_byte_index as _));
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

pub enum BufferReadError {
    FileNotFound,
    InvalidData,
    Other,
}
impl fmt::Display for BufferReadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::FileNotFound => f.write_str("file not found"),
            Self::InvalidData => f.write_str("invalid data while reading from file"),
            Self::Other => f.write_str("could not read from file"),
        }
    }
}
impl From<io::Error> for BufferReadError {
    fn from(other: io::Error) -> Self {
        match other.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::Unsupported => Self::FileNotFound,
            io::ErrorKind::InvalidData => Self::InvalidData,
            _ => Self::Other,
        }
    }
}

pub enum BufferWriteError {
    SavingDisabled,
    CouldNotWriteToFile,
}
impl fmt::Display for BufferWriteError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::SavingDisabled => f.write_str("buffer has saving disabled"),
            Self::CouldNotWriteToFile => f.write_str("could not write to file"),
        }
    }
}
impl From<io::Error> for BufferWriteError {
    fn from(_: io::Error) -> Self {
        Self::CouldNotWriteToFile
    }
}

#[derive(Default)]
pub struct BufferProperties {
    pub history_enabled: bool,
    pub saving_enabled: bool,
    pub is_file: bool,
    pub word_database_enabled: bool,
}
impl BufferProperties {
    pub fn text() -> Self {
        Self {
            history_enabled: true,
            saving_enabled: true,
            is_file: true,
            word_database_enabled: true,
        }
    }

    pub fn scratch() -> Self {
        Self {
            history_enabled: false,
            saving_enabled: false,
            is_file: true,
            word_database_enabled: false,
        }
    }
}

pub struct Buffer {
    alive: bool,
    handle: BufferHandle,
    pub path: PathBuf,
    content: BufferContent,
    syntax_handle: SyntaxHandle,
    highlighted: HighlightedBuffer,
    history: BufferHistory,
    pub lints: BufferLintCollection,
    breakpoints: BufferBreakpointCollection,
    search_ranges: Vec<BufferRange>,
    needs_save: bool,
    pub properties: BufferProperties,
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
            history: BufferHistory::new(),
            lints: BufferLintCollection::default(),
            breakpoints: BufferBreakpointCollection::default(),
            search_ranges: Vec::new(),
            needs_save: false,
            properties: BufferProperties::default(),
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
        self.properties = BufferProperties::default();
    }

    fn remove_all_words_from_database(&mut self, word_database: &mut WordDatabase) {
        if self.properties.word_database_enabled {
            for line in &self.content.lines {
                for word in WordIter(line.as_str()).of_kind(WordKind::Identifier) {
                    word_database.remove(word);
                }
            }
        }
    }

    pub fn handle(&self) -> BufferHandle {
        self.handle
    }

    pub fn set_path(&mut self, path: &Path) {
        self.path.clear();
        let mut components = path.components();
        match components.next() {
            Some(Component::CurDir) => self.path.push(components.as_path()),
            Some(_) => self.path.push(path),
            None => (),
        }
    }

    pub fn content(&self) -> &BufferContent {
        &self.content
    }

    pub fn highlighted(&self) -> &HighlightedBuffer {
        &self.highlighted
    }

    pub fn update_highlighting(&mut self, syntaxes: &SyntaxCollection) -> HighlightResult {
        self.highlighted
            .highlight_dirty_lines(syntaxes.get(self.syntax_handle), &self.content)
    }

    pub fn refresh_syntax(&mut self, syntaxes: &SyntaxCollection) {
        let path = self.path.to_str().unwrap_or("");
        if path.is_empty() {
            return;
        }

        let syntax_handle = syntaxes.find_handle_by_path(path).unwrap_or_default();

        if self.syntax_handle != syntax_handle {
            self.syntax_handle = syntax_handle;
            self.highlighted.clear();
            self.highlighted.insert_range(BufferRange::between(
                BufferPosition::zero(),
                BufferPosition::line_col((self.content.lines.len() - 1) as _, 0),
            ));
        }
    }

    pub fn breakpoints(&self) -> &[BufferBreakpoint] {
        &self.breakpoints.breakpoints
    }

    pub fn breakpoints_mut(&mut self) -> BufferBreakpointMutCollection {
        BufferBreakpointMutCollection {
            inner: &mut self.breakpoints,
            buffer_handle: self.handle,
        }
    }

    pub fn needs_save(&self) -> bool {
        self.properties.saving_enabled && self.needs_save
    }

    pub fn insert_text(
        &mut self,
        word_database: &mut WordDatabase,
        position: BufferPosition,
        text: &str,
        events: &mut BufferTextInsertsMutGuard,
    ) -> BufferRange {
        self.search_ranges.clear();
        let position = self.content.saturate_position(position);

        if text.is_empty() {
            return BufferRange::between(position, position);
        }
        self.needs_save = true;

        let range = Self::insert_text_no_history(
            &mut self.content,
            self.properties.word_database_enabled,
            word_database,
            position,
            text,
        );

        events.add(range, text);

        if self.properties.history_enabled {
            self.history.add_edit(Edit {
                kind: EditKind::Insert,
                range,
                text,
            });
        }

        range
    }

    fn insert_text_no_history(
        content: &mut BufferContent,
        uses_word_database: bool,
        word_database: &mut WordDatabase,
        position: BufferPosition,
        text: &str,
    ) -> BufferRange {
        if uses_word_database {
            for word in WordIter(content.lines()[position.line_index as usize].as_str())
                .of_kind(WordKind::Identifier)
            {
                word_database.remove(word);
            }
        }

        let range = content.insert_text(position, text);

        if uses_word_database {
            for line in
                &content.lines()[range.from.line_index as usize..=range.to.line_index as usize]
            {
                for word in WordIter(line.as_str()).of_kind(WordKind::Identifier) {
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
        events: &mut BufferRangeDeletesMutGuard,
    ) {
        self.search_ranges.clear();
        range.from = self.content.saturate_position(range.from);
        range.to = self.content.saturate_position(range.to);

        if range.from == range.to {
            return;
        }
        self.needs_save = true;

        events.add(range);

        let from = range.from;
        let to = range.to;

        if self.properties.history_enabled {
            fn add_history_delete_line(buffer: &mut Buffer, from: BufferPosition) {
                let line = buffer.content.lines()[from.line_index as usize].as_str();
                let range = BufferRange::between(
                    BufferPosition::line_col(from.line_index, line.len() as _),
                    BufferPosition::line_col(from.line_index + 1, 0),
                );
                buffer.history.add_edit(Edit {
                    kind: EditKind::Delete,
                    range,
                    text: "\n",
                });
                buffer.history.add_edit(Edit {
                    kind: EditKind::Delete,
                    range: BufferRange::between(from, range.from),
                    text: &line[from.column_byte_index as usize..],
                });
            }

            if from.line_index == to.line_index {
                let text = &self.content.lines()[from.line_index as usize].as_str()
                    [from.column_byte_index as usize..to.column_byte_index as usize];
                self.history.add_edit(Edit {
                    kind: EditKind::Delete,
                    range,
                    text,
                });
            } else {
                let text = &self.content.lines()[to.line_index as usize].as_str()
                    [..to.column_byte_index as usize];
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
            self.properties.word_database_enabled,
            word_database,
            range,
        );
    }

    fn delete_range_no_history(
        content: &mut BufferContent,
        uses_word_database: bool,
        word_database: &mut WordDatabase,
        range: BufferRange,
    ) {
        if uses_word_database {
            for line in
                &content.lines()[range.from.line_index as usize..=range.to.line_index as usize]
            {
                for word in WordIter(line.as_str()).of_kind(WordKind::Identifier) {
                    word_database.remove(word);
                }
            }

            content.delete_range(range);

            for word in WordIter(content.lines()[range.from.line_index as usize].as_str())
                .of_kind(WordKind::Identifier)
            {
                word_database.add(word);
            }
        } else {
            content.delete_range(range);
        }
    }

    pub fn commit_edits(&mut self) {
        self.history.commit_edits();
    }

    pub fn undo(
        &mut self,
        word_database: &mut WordDatabase,
        events: &mut EditorEventWriter,
    ) -> impl '_ + ExactSizeIterator<Item = Edit<'_>> + DoubleEndedIterator<Item = Edit<'_>> {
        self.apply_history_edits(word_database, events, BufferHistory::undo_edits)
    }

    pub fn redo(
        &mut self,
        word_database: &mut WordDatabase,
        events: &mut EditorEventWriter,
    ) -> impl '_ + ExactSizeIterator<Item = Edit<'_>> + DoubleEndedIterator<Item = Edit<'_>> {
        self.apply_history_edits(word_database, events, BufferHistory::redo_edits)
    }

    fn apply_history_edits<'a, F, I>(
        &'a mut self,
        word_database: &mut WordDatabase,
        events: &mut EditorEventWriter,
        selector: F,
    ) -> I
    where
        F: FnOnce(&'a mut BufferHistory) -> I,
        I: 'a + Clone + ExactSizeIterator<Item = Edit<'a>>,
    {
        self.search_ranges.clear();
        self.needs_save = true;

        let content = &mut self.content;
        let uses_word_database = self.properties.word_database_enabled;

        let edits = selector(&mut self.history);

        let mut edits_iter = edits.clone();
        let mut next_edit = edits_iter.next();
        loop {
            let mut edit = match next_edit.take() {
                Some(edit) => edit,
                None => break,
            };
            match edit.kind {
                EditKind::Insert => {
                    let mut events = events.buffer_text_inserts_mut_guard(self.handle);
                    loop {
                        Self::insert_text_no_history(
                            content,
                            uses_word_database,
                            word_database,
                            edit.range.from,
                            edit.text,
                        );
                        events.add(edit.range, edit.text);

                        edit = match edits_iter.next() {
                            Some(edit) => edit,
                            None => break,
                        };
                        if edit.kind != EditKind::Insert {
                            next_edit = Some(edit);
                            break;
                        }
                    }
                }
                EditKind::Delete => {
                    let mut events = events.buffer_range_deletes_mut_guard(self.handle);
                    loop {
                        Self::delete_range_no_history(
                            content,
                            uses_word_database,
                            word_database,
                            edit.range,
                        );
                        events.add(edit.range);

                        edit = match edits_iter.next() {
                            Some(edit) => edit,
                            None => break,
                        };
                        if edit.kind != EditKind::Delete {
                            next_edit = Some(edit);
                            break;
                        }
                    }
                }
            }
        }

        /*
        for edit in edits.clone() {
            match edit.kind {
                EditKind::Insert => {
                    Self::insert_text_no_history(
                        content,
                        uses_word_database,
                        word_database,
                        edit.range.from,
                        edit.text,
                    );
                    events
                        .buffer_text_inserts_mut_guard(self.handle)
                        .add(edit.range, edit.text);
                    //edit_events.add_insert(edit.range, edit.text);
                }
                EditKind::Delete => {
                    Self::delete_range_no_history(
                        content,
                        uses_word_database,
                        word_database,
                        edit.range,
                    );
                    events
                        .buffer_range_deletes_mut_guard(self.handle)
                        .add(edit.range);
                    //edit_events.add_delete(edit.range);
                }
            }
        }
        */

        edits
    }

    pub fn set_search(&mut self, pattern: &Pattern) {
        self.search_ranges.clear();
        self.content
            .find_search_ranges(pattern, &mut self.search_ranges);
    }

    pub fn search_ranges(&self) -> &[BufferRange] {
        &self.search_ranges
    }

    pub fn read_from_file(
        &mut self,
        word_database: &mut WordDatabase,
        events: &mut EditorEventWriter,
    ) -> Result<(), BufferReadError> {
        fn clear_buffer(buffer: &mut Buffer, word_database: &mut WordDatabase) {
            buffer.remove_all_words_from_database(word_database);
            buffer.content.clear();
            buffer.highlighted.clear();
        }

        self.needs_save = false;
        self.history.clear();
        self.search_ranges.clear();

        events.enqueue(EditorEvent::BufferRead {
            handle: self.handle,
        });

        if !self.path.starts_with(help::HELP_PREFIX) && !self.properties.is_file {
            return Ok(());
        }

        if self.path.as_os_str().is_empty() {
            return Err(BufferReadError::FileNotFound);
        } else if let Some(mut reader) = help::open(&self.path) {
            clear_buffer(self, word_database);
            self.content.read(&mut reader)?;
        } else {
            match File::open(&self.path) {
                Ok(file) => {
                    clear_buffer(self, word_database);
                    let mut reader = io::BufReader::new(file);
                    self.content.read(&mut reader)?;
                }
                Err(error) => {
                    if self.properties.saving_enabled {
                        return Err(error.into());
                    } else {
                        clear_buffer(self, word_database);
                    }
                }
            }
        }

        self.highlighted.insert_range(BufferRange::between(
            BufferPosition::zero(),
            BufferPosition::line_col((self.content.lines.len() - 1) as _, 0),
        ));

        if self.properties.word_database_enabled {
            for line in &self.content.lines {
                for word in WordIter(line.as_str()).of_kind(WordKind::Identifier) {
                    word_database.add(word);
                }
            }
        }

        Ok(())
    }

    pub fn write_to_file(
        &mut self,
        new_path: Option<&Path>,
        events: &mut EditorEventWriter,
    ) -> Result<(), BufferWriteError> {
        let new_path = match new_path {
            Some(path) => {
                self.properties.saving_enabled = true;
                self.properties.is_file = true;
                self.set_path(path);
                true
            }
            None => false,
        };

        if !self.properties.saving_enabled {
            return Err(BufferWriteError::SavingDisabled);
        }

        if self.properties.is_file {
            let file = File::create(&self.path)?;
            self.content.write(&mut io::BufWriter::new(file))?;
        }

        self.needs_save = false;

        events.enqueue(EditorEvent::BufferWrite {
            handle: self.handle,
            new_path,
        });
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BufferHandle(pub u32);

pub struct InsertProcess {
    pub alive: bool,
    pub handle: Option<PlatformProcessHandle>,
    pub buffer_handle: BufferHandle,
    pub position: BufferPosition,
    pub input: Option<PooledBuf>,
    pub output_residual_bytes: ResidualStrBytes,
}

#[derive(Default)]
pub struct BufferCollection {
    buffers: Vec<Buffer>,
    insert_processes: Vec<InsertProcess>,
}

impl BufferCollection {
    pub fn add_new(&mut self) -> &mut Buffer {
        let mut handle = None;
        for (i, buffer) in self.buffers.iter_mut().enumerate() {
            if !buffer.alive {
                handle = Some(BufferHandle(i as _));
                break;
            }
        }
        let handle = match handle {
            Some(handle) => handle,
            None => {
                let handle = BufferHandle(self.buffers.len() as _);
                self.buffers.push(Buffer::new(handle));
                handle
            }
        };

        let buffer = &mut self.buffers[handle.0 as usize];
        buffer.alive = true;
        buffer
    }

    pub fn try_get(&self, handle: BufferHandle) -> Option<&Buffer> {
        let index = handle.0 as usize;
        if self.buffers.len() <= index {
            return None;
        }
        let buffer = &self.buffers[index];
        if buffer.alive {
            Some(buffer)
        } else {
            None
        }
    }

    pub fn get(&self, handle: BufferHandle) -> &Buffer {
        &self.buffers[handle.0 as usize]
    }

    pub fn get_mut(&mut self, handle: BufferHandle) -> &mut Buffer {
        &mut self.buffers[handle.0 as usize]
    }

    pub fn find_with_path(&self, buffers_root: &Path, path: &Path) -> Option<BufferHandle> {
        let mut components = path.components();
        let path = match components.next()? {
            Component::CurDir => components.as_path(),
            Component::RootDir | Component::Prefix(_) => match path.strip_prefix(buffers_root) {
                Ok(path) => path,
                Err(_) => path,
            },
            _ => path,
        };

        for buffer in self.iter() {
            let buffer_path = buffer.path.as_path();
            let buffer_path = buffer_path
                .strip_prefix(buffers_root)
                .unwrap_or(buffer_path);

            if buffer_path == path {
                return Some(buffer.handle());
            }
        }

        None
    }

    pub fn iter(&self) -> impl Iterator<Item = &Buffer> {
        self.buffers.iter().filter(|b| b.alive)
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Buffer> {
        self.buffers.iter_mut().filter(|b| b.alive)
    }

    pub fn defer_remove(&self, handle: BufferHandle, events: &mut EditorEventWriter) {
        let buffer = &self.buffers[handle.0 as usize];
        if buffer.alive {
            events.enqueue(EditorEvent::BufferClose { handle });
        }
    }

    pub(crate) fn remove_now(
        &mut self,
        platform: &mut Platform,
        handle: BufferHandle,
        word_database: &mut WordDatabase,
    ) {
        let buffer = &mut self.buffers[handle.0 as usize];
        if buffer.alive {
            buffer.dispose(word_database);
        }

        for process in &mut self.insert_processes {
            if process.buffer_handle != handle {
                continue;
            }

            if let Some(handle) = process.handle.take() {
                platform
                    .requests
                    .enqueue(PlatformRequest::KillProcess { handle });
            }
        }
    }

    pub(crate) fn on_buffer_text_inserts(
        &mut self,
        buffer_handle: BufferHandle,
        inserts: &[EditorEventTextInsert],
        events: &mut EditorEventWriter,
    ) {
        let buffer = self.get_mut(buffer_handle);

        let mut breakpoints_changed = false;
        for insert in inserts {
            let range = insert.range;
            buffer.highlighted.insert_range(range);
            buffer.lints.insert_range(range);
            if buffer.breakpoints.insert_range(range) {
                breakpoints_changed = true;
            }
        }

        if breakpoints_changed {
            events.enqueue(EditorEvent::BufferBreakpointsChanged {
                handle: buffer_handle,
            });
        }

        for process in self.insert_processes.iter_mut() {
            if process.alive && process.buffer_handle == buffer_handle {
                let mut position = process.position;
                for insert in inserts {
                    position = position.insert(insert.range);
                }
                process.position = position;
            }
        }
    }

    pub(crate) fn on_buffer_range_deletes(
        &mut self,
        buffer_handle: BufferHandle,
        deletes: &[BufferRange],
        events: &mut EditorEventWriter,
    ) {
        let buffer = self.get_mut(buffer_handle);

        let mut breakpoints_changed = false;
        for &range in deletes {
            buffer.highlighted.delete_range(range);
            buffer.lints.delete_range(range);
            if buffer.breakpoints.delete_range(range) {
                breakpoints_changed = true;
            }
        }

        if breakpoints_changed {
            events.enqueue(EditorEvent::BufferBreakpointsChanged {
                handle: buffer_handle,
            });
        }

        for process in self.insert_processes.iter_mut() {
            if process.alive && process.buffer_handle == buffer_handle {
                let mut position = process.position;
                for &range in deletes {
                    position = position.delete(range);
                }
                process.position = position;
            }
        }
    }

    pub fn spawn_insert_process(
        &mut self,
        platform: &mut Platform,
        mut command: Command,
        buffer_handle: BufferHandle,
        position: BufferPosition,
        input: Option<PooledBuf>,
    ) {
        let mut index = None;
        for (i, process) in self.insert_processes.iter_mut().enumerate() {
            if !process.alive {
                index = Some(i);
                break;
            }
        }
        let index = match index {
            Some(index) => index,
            None => {
                let index = self.insert_processes.len();
                self.insert_processes.push(InsertProcess {
                    alive: false,
                    handle: None,
                    buffer_handle,
                    position,
                    input: None,
                    output_residual_bytes: ResidualStrBytes::default(),
                });
                index
            }
        };

        let process = &mut self.insert_processes[index];
        process.alive = true;
        process.handle = None;
        process.buffer_handle = buffer_handle;
        process.position = position;
        process.input = input;
        process.output_residual_bytes = ResidualStrBytes::default();

        let stdin = match &process.input {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        };

        command.stdin(stdin);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::null());

        platform.requests.enqueue(PlatformRequest::SpawnProcess {
            tag: ProcessTag::Buffer(index as _),
            command,
            buf_len: 4 * 1024,
        });
    }

    pub(crate) fn on_process_spawned(
        &mut self,
        platform: &mut Platform,
        index: u32,
        handle: PlatformProcessHandle,
    ) {
        let process = &mut self.insert_processes[index as usize];
        process.handle = Some(handle);

        if let Some(buf) = process.input.take() {
            platform
                .requests
                .enqueue(PlatformRequest::WriteToProcess { handle, buf });
            platform
                .requests
                .enqueue(PlatformRequest::CloseProcessInput { handle });
        }
    }

    pub(crate) fn on_process_output(
        &mut self,
        word_database: &mut WordDatabase,
        index: u32,
        bytes: &[u8],
        events: &mut EditorEventWriter,
    ) {
        let process = &mut self.insert_processes[index as usize];
        if process.handle.is_none() {
            return;
        }

        let mut buf = Default::default();
        let texts = process.output_residual_bytes.receive_bytes(&mut buf, bytes);

        let buffer = &mut self.buffers[process.buffer_handle.0 as usize];
        let mut events = events.buffer_text_inserts_mut_guard(buffer.handle());
        let mut position = process.position;
        for text in texts {
            let insert_range = buffer.insert_text(word_database, position, text, &mut events);
            position = position.insert(insert_range);
        }
    }

    pub(crate) fn on_process_exit(
        &mut self,
        word_database: &mut WordDatabase,
        index: u32,
        events: &mut EditorEventWriter,
    ) {
        self.on_process_output(word_database, index, &[], events);
        let process = &mut self.insert_processes[index as usize];
        process.alive = false;
        process.handle = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{buffer_position::BufferPosition, events::EditorEventQueue};

    #[test]
    fn display_distance() {
        fn display_len(text: &str) -> usize {
            CharDisplayDistances::new(text, 4)
                .last()
                .map(|d| d.distance as _)
                .unwrap_or(0)
        }

        assert_eq!(0, display_len(""));
        assert_eq!(1, display_len("a"));
        assert_eq!(1, display_len("é"));
        assert_eq!(4, display_len("    "));
        assert_eq!(4, display_len("\t"));
        assert_eq!(8, display_len("\t\t"));
        assert_eq!(8, display_len("    \t"));
        assert_eq!(5, display_len("x\t"));
        assert_eq!(6, display_len("xx\t"));
        assert_eq!(7, display_len("xxx\t"));
        assert_eq!(8, display_len("xxxx\t"));
    }

    fn buffer_from_str(text: &str) -> BufferContent {
        let mut buffer = BufferContent::new();
        buffer.insert_text(BufferPosition::zero(), text);
        buffer
    }

    #[test]
    fn buffer_utf8_support() {
        let mut buffer = buffer_from_str("abd");
        let range = buffer.insert_text(BufferPosition::line_col(0, 2), "ç");
        assert_eq!(
            BufferRange::between(
                BufferPosition::line_col(0, 2),
                BufferPosition::line_col(0, (2 + 'ç'.len_utf8()) as _)
            ),
            range
        );
    }

    #[test]
    fn buffer_content_insert_text() {
        let mut buffer = BufferContent::new();

        assert_eq!(1, buffer.lines().len());
        assert_eq!("", buffer.to_string());

        buffer.insert_text(BufferPosition::line_col(0, 0), "hold");
        buffer.insert_text(BufferPosition::line_col(0, 2), "r");
        buffer.insert_text(BufferPosition::line_col(0, 1), "ello w");
        assert_eq!(1, buffer.lines().len());
        assert_eq!("hello world", buffer.to_string());

        buffer.insert_text(BufferPosition::line_col(0, 5), "\n");
        buffer.insert_text(
            BufferPosition::line_col(1, 6),
            " appending more\nand more\nand even more\nlines",
        );
        assert_eq!(5, buffer.lines().len());
        assert_eq!(
            "hello\n world appending more\nand more\nand even more\nlines",
            buffer.to_string()
        );

        let mut buffer = buffer_from_str("this is content");
        buffer.insert_text(BufferPosition::line_col(0, 8), "some\nmultiline ");
        assert_eq!(2, buffer.lines().len());
        assert_eq!("this is some\nmultiline content", buffer.to_string());

        let mut buffer = buffer_from_str("this is content");
        buffer.insert_text(
            BufferPosition::line_col(0, 8),
            "some\nmore\nextensive\nmultiline ",
        );
        assert_eq!(4, buffer.lines().len());
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

        assert_eq!(2, buffer.lines().len());
        assert_eq!(
            "this is the initial\ncontent of the buffer",
            buffer.to_string()
        );

        buffer.delete_range(BufferRange::between(
            BufferPosition::zero(),
            BufferPosition::zero(),
        ));
        assert_eq!(2, buffer.lines().len());
        assert_eq!(
            "this is the initial\ncontent of the buffer",
            buffer.to_string()
        );

        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 11),
            BufferPosition::line_col(0, 19),
        ));
        assert_eq!(2, buffer.lines().len());
        assert_eq!("this is the\ncontent of the buffer", buffer.to_string());

        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(0, 8),
            BufferPosition::line_col(1, 15),
        ));
        assert_eq!(1, buffer.lines().len());
        assert_eq!("this is buffer", buffer.to_string());

        let mut buffer = buffer_from_str("this\nbuffer\ncontains\nmultiple\nlines\nyes");
        assert_eq!(6, buffer.lines().len());
        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 4),
            BufferPosition::line_col(4, 1),
        ));
        assert_eq!("this\nbuffines\nyes", buffer.to_string());
    }

    #[test]
    fn buffer_content_delete_lines() {
        let mut buffer = buffer_from_str("first line\nsecond line\nthird line");
        assert_eq!(3, buffer.lines().len());
        buffer.delete_range(BufferRange::between(
            BufferPosition::line_col(1, 0),
            BufferPosition::line_col(2, 0),
        ));
        assert_eq!("first line\nthird line", buffer.to_string());

        let mut buffer = buffer_from_str("first line\nsecond line\nthird line");
        assert_eq!(3, buffer.lines().len());
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
        buffer.properties = BufferProperties::text();
        buffer.insert_text(
            &mut word_database,
            BufferPosition::zero(),
            "single line content",
            &mut events
                .writer()
                .buffer_text_inserts_mut_guard(buffer.handle()),
        );
        let range = BufferRange::between(
            BufferPosition::line_col(0, 7),
            BufferPosition::line_col(0, 12),
        );
        buffer.delete_range(
            &mut word_database,
            range,
            &mut events
                .writer()
                .buffer_range_deletes_mut_guard(buffer.handle()),
        );

        assert_eq!("single content", buffer.content.to_string());
        {
            let mut ranges = buffer.undo(&mut word_database, &mut events.writer());
            assert_eq!(range, ranges.next().unwrap().range);
            ranges.next().unwrap();
            assert!(ranges.next().is_none());
        }
        assert!(buffer.content.to_string().is_empty());
        let mut redo_iter = buffer.redo(&mut word_database, &mut events.writer());
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
        buffer.properties = BufferProperties::text();
        let insert_range = buffer.insert_text(
            &mut word_database,
            BufferPosition::zero(),
            "multi\nline\ncontent",
            &mut events
                .writer()
                .buffer_text_inserts_mut_guard(buffer.handle()),
        );
        assert_eq!("multi\nline\ncontent", buffer.content.to_string());

        let delete_range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(1, 3),
        );
        buffer.delete_range(
            &mut word_database,
            delete_range,
            &mut events
                .writer()
                .buffer_range_deletes_mut_guard(buffer.handle()),
        );
        assert_eq!("me\ncontent", buffer.content.to_string());

        {
            let mut undo_edits = buffer.undo(&mut word_database, &mut events.writer());
            assert_eq!(delete_range, undo_edits.next().unwrap().range);
            assert_eq!(insert_range, undo_edits.next().unwrap().range);
            assert!(undo_edits.next().is_none());
        }
        assert_eq!("", buffer.content.to_string());

        {
            let mut redo_edits = buffer.redo(&mut word_database, &mut events.writer());
            redo_edits.next().unwrap();
            redo_edits.next().unwrap();
            assert!(redo_edits.next().is_none());
        }
        assert_eq!("me\ncontent", buffer.content.to_string());
    }

    #[test]
    fn buffer_insert_delete_forward_insert_undo() {
        let mut word_database = WordDatabase::new();
        let mut events = EditorEventQueue::default();

        let mut buffer = Buffer::new(BufferHandle(0));
        buffer.properties = BufferProperties::text();
        let insert_range = buffer.insert_text(
            &mut word_database,
            BufferPosition::zero(),
            "\n",
            &mut events
                .writer()
                .buffer_text_inserts_mut_guard(buffer.handle()),
        );
        let assert_range = BufferRange::between(
            BufferPosition::line_col(0, 0),
            BufferPosition::line_col(1, 0),
        );
        assert_eq!(assert_range, insert_range);

        buffer.commit_edits();
        assert_eq!("\n", buffer.content.to_string());

        let insert_range = buffer.insert_text(
            &mut word_database,
            BufferPosition::zero(),
            "a",
            &mut events
                .writer()
                .buffer_text_inserts_mut_guard(buffer.handle()),
        );
        let assert_range = BufferRange::between(
            BufferPosition::line_col(0, 0),
            BufferPosition::line_col(0, 1),
        );
        assert_eq!(assert_range, insert_range);

        buffer.delete_range(
            &mut word_database,
            BufferRange::between(
                BufferPosition::line_col(0, 1),
                BufferPosition::line_col(1, 0),
            ),
            &mut events
                .writer()
                .buffer_range_deletes_mut_guard(buffer.handle()),
        );

        let insert_range = buffer.insert_text(
            &mut word_database,
            BufferPosition::line_col(0, 1),
            "b",
            &mut events
                .writer()
                .buffer_text_inserts_mut_guard(buffer.handle()),
        );
        let assert_range = BufferRange::between(
            BufferPosition::line_col(0, 1),
            BufferPosition::line_col(0, 2),
        );
        assert_eq!(assert_range, insert_range);

        buffer.undo(&mut word_database, &mut events.writer());
    }

    #[test]
    fn buffer_content_text_range() {
        let buffer = buffer_from_str("abc\ndef\nghi");
        let range = BufferRange::between(
            BufferPosition::line_col(0, 2),
            BufferPosition::line_col(2, 1),
        );

        let mut text_range = buffer.text_range(range);
        assert_eq!(Some("c"), text_range.next());
        assert_eq!(Some("\n"), text_range.next());
        assert_eq!(Some("def"), text_range.next());
        assert_eq!(Some("\n"), text_range.next());
        assert_eq!(Some("g"), text_range.next());
        assert_eq!(None, text_range.next());
    }

    #[test]
    fn buffer_content_word_at() {
        fn col(column: usize) -> BufferPosition {
            BufferPosition::line_col(0, column as _)
        }

        fn assert_word(word: WordRefWithPosition, pos: BufferPosition, kind: WordKind, text: &str) {
            assert_eq!(pos, word.position);
            assert_eq!(kind, word.kind);
            assert_eq!(text, word.text);
        }

        let buffer = buffer_from_str("word");
        assert_word(buffer.word_at(col(0)), col(0), WordKind::Identifier, "word");
        assert_word(buffer.word_at(col(2)), col(0), WordKind::Identifier, "word");
        assert_word(buffer.word_at(col(4)), col(4), WordKind::Whitespace, "");

        let buffer = buffer_from_str("asd word+? asd");
        assert_word(buffer.word_at(col(3)), col(3), WordKind::Whitespace, " ");
        assert_word(buffer.word_at(col(4)), col(4), WordKind::Identifier, "word");
        assert_word(buffer.word_at(col(6)), col(4), WordKind::Identifier, "word");
        assert_word(buffer.word_at(col(8)), col(8), WordKind::Symbol, "+?");
        assert_word(buffer.word_at(col(9)), col(8), WordKind::Symbol, "+?");
        assert_word(buffer.word_at(col(10)), col(10), WordKind::Whitespace, " ");
    }

    #[test]
    fn buffer_content_words_from() {
        fn col(column: usize) -> BufferPosition {
            BufferPosition::line_col(0, column as _)
        }

        fn assert_word(word: WordRefWithPosition, pos: BufferPosition, kind: WordKind, text: &str) {
            assert_eq!(pos, word.position);
            assert_eq!(kind, word.kind);
            assert_eq!(text, word.text);
        }

        let buffer = buffer_from_str("word");
        let (w, mut lw, mut rw) = buffer.words_from(col(0));
        assert_word(w, col(0), WordKind::Identifier, "word");
        assert!(lw.next().is_none());
        assert!(rw.next().is_none());
        let (w, mut lw, mut rw) = buffer.words_from(col(2));
        assert_word(w, col(0), WordKind::Identifier, "word");
        assert!(lw.next().is_none());
        assert!(rw.next().is_none());
        let (w, mut lw, mut rw) = buffer.words_from(col(4));
        assert_word(w, col(4), WordKind::Whitespace, "");
        assert_word(lw.next().unwrap(), col(0), WordKind::Identifier, "word");
        assert!(lw.next().is_none());
        assert!(rw.next().is_none());

        let buffer = buffer_from_str("first second third");
        let (w, mut lw, mut rw) = buffer.words_from(col(8));
        assert_word(w, col(6), WordKind::Identifier, "second");
        assert_word(lw.next().unwrap(), col(5), WordKind::Whitespace, " ");
        assert_word(lw.next().unwrap(), col(0), WordKind::Identifier, "first");
        assert!(lw.next().is_none());
        assert_word(rw.next().unwrap(), col(12), WordKind::Whitespace, " ");
        assert_word(rw.next().unwrap(), col(13), WordKind::Identifier, "third");
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
    fn buffer_display_len() {
        fn len(buffer: &BufferContent, line: usize) -> usize {
            buffer.line_display_lens()[line].total_len(4)
        }

        let mut buffer = buffer_from_str("abc\tdef");

        assert_eq!(10, len(&buffer, 0));

        buffer.insert_text(BufferPosition::line_col(0, 3), "\n");

        assert_eq!(3, len(&buffer, 0));
        assert_eq!(7, len(&buffer, 1));

        buffer.insert_text(BufferPosition::line_col(1, 3), "\n");

        assert_eq!(3, len(&buffer, 0));
        assert_eq!(6, len(&buffer, 1));
        assert_eq!(1, len(&buffer, 2));

        buffer.insert_text(BufferPosition::line_col(2, 0), "xx");

        assert_eq!(3, len(&buffer, 0));
        assert_eq!(6, len(&buffer, 1));
        assert_eq!(3, len(&buffer, 2));

        buffer.delete_range(BufferRange::between(
            BufferPosition::zero(),
            BufferPosition::line_col(0, 3),
        ));

        assert_eq!(0, len(&buffer, 0));
        assert_eq!(6, len(&buffer, 1));
        assert_eq!(3, len(&buffer, 2));
    }
}
