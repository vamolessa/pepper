use std::{fmt, path::Path, str::FromStr};

use crate::{
    buffer::{BufferCapabilities, BufferCollection, BufferHandle},
    buffer_position::{BufferPosition, BufferRange},
    client::ClientHandle,
    cursor::{Cursor, CursorCollection},
    events::EditorEventQueue,
    history::{Edit, EditKind},
    word_database::{WordDatabase, WordIter, WordKind},
};

pub enum CursorMovement {
    ColumnsForward(usize),
    ColumnsBackward(usize),
    LinesForward(usize),
    LinesBackward(usize),
    WordsForward(usize),
    WordsBackward(usize),
    Home,
    HomeNonWhitespace,
    End,
    FirstLine,
    LastLine,
}

#[derive(Clone, Copy)]
pub enum CursorMovementKind {
    PositionAndAnchor,
    PositionOnly,
}

pub struct BufferView {
    pub client_handle: ClientHandle,
    pub buffer_handle: BufferHandle,
    pub cursors: CursorCollection,
}

impl BufferView {
    pub fn new(client_handle: ClientHandle, buffer_handle: BufferHandle) -> Self {
        Self {
            client_handle,
            buffer_handle,
            cursors: CursorCollection::new(),
        }
    }

    pub fn clone_with_client_handle(&self, client_handle: ClientHandle) -> Self {
        Self {
            client_handle,
            buffer_handle: self.buffer_handle,
            cursors: self.cursors.clone(),
        }
    }

    pub fn move_cursors(
        &mut self,
        buffers: &BufferCollection,
        movement: CursorMovement,
        movement_kind: CursorMovementKind,
    ) {
        fn try_nth<I, E>(iter: I, mut n: usize) -> Result<E, usize>
        where
            I: Iterator<Item = E>,
        {
            for e in iter {
                if n == 0 {
                    return Ok(e);
                }
                n -= 1;
            }
            Err(n)
        }

        let buffer = match buffers.get(self.buffer_handle) {
            Some(buffer) => buffer.content(),
            None => return,
        };

        let mut cursors = self.cursors.mut_guard();
        match movement {
            CursorMovement::ColumnsForward(n) => {
                let last_line_index = buffer.line_count() - 1;
                for c in &mut cursors[..] {
                    let line = buffer.line_at(c.position.line_index).as_str();
                    match try_nth(line[c.position.column_byte_index..].char_indices(), n) {
                        Ok((i, _)) => c.position.column_byte_index += i,
                        Err(0) => c.position.column_byte_index = line.len(),
                        Err(mut n) => {
                            n -= 1;
                            loop {
                                if c.position.line_index == last_line_index {
                                    c.position.column_byte_index =
                                        buffer.line_at(last_line_index).as_str().len();
                                    break;
                                }

                                c.position.line_index += 1;
                                let line = buffer.line_at(c.position.line_index).as_str();
                                match try_nth(line.char_indices(), n) {
                                    Ok((i, _)) => {
                                        c.position.column_byte_index = i;
                                        break;
                                    }
                                    Err(0) => {
                                        c.position.column_byte_index = line.len();
                                        break;
                                    }
                                    Err(rest) => n = rest - 1,
                                }
                            }
                        }
                    }
                }
            }
            CursorMovement::ColumnsBackward(n) => {
                if n == 0 {
                    return;
                }
                let n = n - 1;

                for c in &mut cursors[..] {
                    let line = buffer.line_at(c.position.line_index).as_str();
                    match try_nth(line[..c.position.column_byte_index].char_indices().rev(), n) {
                        Ok((i, _)) => c.position.column_byte_index = i,
                        Err(0) => {
                            if c.position.line_index == 0 {
                                c.position.column_byte_index = 0;
                            } else {
                                c.position.line_index -= 1;
                                c.position.column_byte_index =
                                    buffer.line_at(c.position.line_index).as_str().len();
                            }
                        }
                        Err(mut n) => {
                            n -= 1;
                            loop {
                                if c.position.line_index == 0 {
                                    c.position.column_byte_index = 0;
                                    break;
                                }

                                c.position.line_index -= 1;
                                let line = buffer.line_at(c.position.line_index).as_str();
                                match try_nth(line.char_indices().rev(), n) {
                                    Ok((i, _)) => {
                                        c.position.column_byte_index = i;
                                        break;
                                    }
                                    Err(0) => {
                                        if c.position.line_index == 0 {
                                            c.position.column_byte_index = 0;
                                        } else {
                                            c.position.line_index -= 1;
                                            c.position.column_byte_index = buffer
                                                .line_at(c.position.line_index)
                                                .as_str()
                                                .len();
                                        }
                                        break;
                                    }
                                    Err(rest) => n = rest - 1,
                                }
                            }
                        }
                    }
                }
            }
            CursorMovement::LinesForward(n) => {
                cursors.save_column_byte_indices();
                for i in 0..cursors[..].len() {
                    let saved_column_byte_index = cursors.get_saved_column_byte_index(i);
                    let c = &mut cursors[i];
                    c.position.line_index = buffer
                        .line_count()
                        .saturating_sub(1)
                        .min(c.position.line_index + n);
                    if let Some(index) = saved_column_byte_index {
                        c.position.column_byte_index = index;
                    }
                    c.position = buffer.saturate_position(c.position);
                }
            }
            CursorMovement::LinesBackward(n) => {
                cursors.save_column_byte_indices();
                for i in 0..cursors[..].len() {
                    let saved_column_byte_index = cursors.get_saved_column_byte_index(i);
                    let c = &mut cursors[i];
                    c.position.line_index = c.position.line_index.saturating_sub(n);
                    if let Some(index) = saved_column_byte_index {
                        c.position.column_byte_index = index;
                    }
                    c.position = buffer.saturate_position(c.position);
                }
            }
            CursorMovement::WordsForward(n) => {
                let last_line_index = buffer.line_count() - 1;
                for c in &mut cursors[..] {
                    let mut n = n;
                    let mut line = buffer.line_at(c.position.line_index).as_str();

                    while n > 0 {
                        if c.position.column_byte_index == line.len() {
                            if c.position.line_index == last_line_index {
                                break;
                            }

                            c.position.line_index += 1;
                            c.position.column_byte_index = 0;
                            line = buffer.line_at(c.position.line_index).as_str();
                            n -= 1;
                            continue;
                        }

                        let words = WordIter::new(&line[c.position.column_byte_index..])
                            .inspect(|w| c.position.column_byte_index += w.text.len())
                            .skip(1)
                            .filter(|w| w.kind != WordKind::Whitespace);

                        match try_nth(words, n - 1) {
                            Ok(word) => {
                                c.position.column_byte_index -= word.text.len();
                                break;
                            }
                            Err(rest) => {
                                n = rest;
                                c.position.column_byte_index = line.len();
                            }
                        }
                    }
                }
            }
            CursorMovement::WordsBackward(n) => {
                for c in &mut cursors[..] {
                    let mut n = n;
                    let mut line = &buffer.line_at(c.position.line_index).as_str()
                        [..c.position.column_byte_index];

                    while n > 0 {
                        let mut last_kind = WordKind::Identifier;
                        let words = WordIter::new(line)
                            .rev()
                            .inspect(|w| {
                                c.position.column_byte_index -= w.text.len();
                                last_kind = w.kind;
                            })
                            .filter(|w| w.kind != WordKind::Whitespace);

                        match try_nth(words, n - 1) {
                            Ok(_) => break,
                            Err(rest) => n = rest + 1,
                        }

                        if last_kind == WordKind::Whitespace {
                            n -= 1;
                            if n == 0 {
                                break;
                            }
                        }

                        if c.position.line_index == 0 {
                            break;
                        }

                        c.position.line_index -= 1;
                        line = buffer.line_at(c.position.line_index).as_str();
                        c.position.column_byte_index = line.len();
                        n -= 1;
                    }
                }
            }
            CursorMovement::Home => {
                for c in &mut cursors[..] {
                    c.position.column_byte_index = 0;
                }
            }
            CursorMovement::HomeNonWhitespace => {
                for c in &mut cursors[..] {
                    let first_word = buffer.line_at(c.position.line_index).word_at(0);
                    match first_word.kind {
                        WordKind::Whitespace => {
                            c.position.column_byte_index = first_word.text.len()
                        }
                        _ => c.position.column_byte_index = 0,
                    }
                }
            }
            CursorMovement::End => {
                for c in &mut cursors[..] {
                    c.position.column_byte_index =
                        buffer.line_at(c.position.line_index).as_str().len();
                }
            }
            CursorMovement::FirstLine => {
                for c in &mut cursors[..] {
                    c.position.line_index = 0;
                    c.position = buffer.saturate_position(c.position);
                }
            }
            CursorMovement::LastLine => {
                for c in &mut cursors[..] {
                    c.position.line_index = buffer.line_count() - 1;
                    c.position = buffer.saturate_position(c.position);
                }
            }
        }

        if let CursorMovementKind::PositionAndAnchor = movement_kind {
            for c in &mut cursors[..] {
                c.anchor = c.position;
            }
        }
    }

    pub fn get_selection_text(&self, buffers: &BufferCollection, text: &mut String) {
        text.clear();

        let buffer = match buffers.get(self.buffer_handle) {
            Some(buffer) => buffer.content(),
            None => return,
        };

        let mut iter = self.cursors[..].iter();
        if let Some(cursor) = iter.next() {
            let mut last_range = cursor.as_range();
            buffer.append_range_text_to_string(last_range, text);
            for cursor in iter {
                let range = cursor.as_range();
                if range.from.line_index > last_range.to.line_index {
                    text.push('\n');
                }
                buffer.append_range_text_to_string(range, text);
                last_range = range;
            }
        }
    }
}

pub enum BufferViewError {
    InvalidPath,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct BufferViewHandle(usize);
impl fmt::Display for BufferViewHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for BufferViewHandle {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse() {
            Ok(i) => Ok(Self(i)),
            Err(_) => Err(()),
        }
    }
}

#[derive(Default)]
pub struct BufferViewCollection {
    buffer_views: Vec<Option<BufferView>>,
    fix_cursor_ranges: Vec<BufferRange>,
}

impl BufferViewCollection {
    pub fn add(&mut self, buffer_view: BufferView) -> BufferViewHandle {
        for (i, slot) in self.buffer_views.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(buffer_view);
                return BufferViewHandle(i);
            }
        }

        let handle = BufferViewHandle(self.buffer_views.len());
        self.buffer_views.push(Some(buffer_view));
        handle
    }

    pub fn defer_remove_buffer_where<F>(
        &mut self,
        buffers: &mut BufferCollection,
        events: &mut EditorEventQueue,
        predicate: F,
    ) where
        F: Fn(&BufferView) -> bool,
    {
        for i in 0..self.buffer_views.len() {
            if let Some(view) = &self.buffer_views[i] {
                if predicate(&view) {
                    buffers.defer_remove(view.buffer_handle, events);
                    self.buffer_views[i] = None;
                }
            }
        }
    }

    pub fn get(&self, handle: BufferViewHandle) -> Option<&BufferView> {
        self.buffer_views[handle.0].as_ref()
    }

    pub fn get_mut(&mut self, handle: BufferViewHandle) -> Option<&mut BufferView> {
        self.buffer_views[handle.0].as_mut()
    }

    pub fn iter_with_handles(&self) -> impl Iterator<Item = (BufferViewHandle, &BufferView)> {
        self.buffer_views
            .iter()
            .enumerate()
            .filter_map(|(i, v)| Some(BufferViewHandle(i)).zip(v.as_ref()))
    }

    pub fn insert_text_at_position(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        handle: BufferViewHandle,
        position: BufferPosition,
        text: &str,
        events: &mut EditorEventQueue,
    ) {
        let current_view = match &mut self.buffer_views[handle.0] {
            Some(view) => view,
            None => return,
        };
        let buffer = match buffers.get_mut(current_view.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        self.fix_cursor_ranges.clear();
        let range = buffer.insert_text(word_database, position, text, events);
        self.fix_cursor_ranges.push(range);

        let current_buffer_handle = current_view.buffer_handle;
        self.fix_buffer_cursors(current_buffer_handle, |cursor, range| cursor.insert(range));
    }

    pub fn insert_text_at_cursor_positions(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        handle: BufferViewHandle,
        text: &str,
        events: &mut EditorEventQueue,
    ) {
        let current_view = match &mut self.buffer_views[handle.0] {
            Some(view) => view,
            None => return,
        };
        let buffer = match buffers.get_mut(current_view.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        self.fix_cursor_ranges.clear();
        for cursor in current_view.cursors[..].iter().rev() {
            let range = buffer.insert_text(word_database, cursor.position, text, events);
            self.fix_cursor_ranges.push(range);
        }

        let current_buffer_handle = current_view.buffer_handle;
        self.fix_buffer_cursors(current_buffer_handle, |cursor, range| cursor.insert(range));
    }

    pub fn delete_in_range(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        handle: BufferViewHandle,
        range: BufferRange,
        events: &mut EditorEventQueue,
    ) {
        let current_view = match &mut self.buffer_views[handle.0] {
            Some(view) => view,
            None => return,
        };
        let buffer = match buffers.get_mut(current_view.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        self.fix_cursor_ranges.clear();
        self.fix_cursor_ranges.push(range);
        buffer.delete_range(word_database, range, events);

        let current_buffer_handle = current_view.buffer_handle;
        self.fix_buffer_cursors(current_buffer_handle, |cursor, range| cursor.delete(range));
    }

    pub fn delete_in_cursor_ranges(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        handle: BufferViewHandle,
        events: &mut EditorEventQueue,
    ) {
        let current_view = match &mut self.buffer_views[handle.0] {
            Some(view) => view,
            None => return,
        };
        let buffer = match buffers.get_mut(current_view.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        self.fix_cursor_ranges.clear();
        for cursor in current_view.cursors[..].iter().rev() {
            let range = cursor.as_range();
            buffer.delete_range(word_database, range, events);
            self.fix_cursor_ranges.push(range);
        }

        let current_buffer_handle = current_view.buffer_handle;
        self.fix_buffer_cursors(current_buffer_handle, |cursor, range| cursor.delete(range));
    }

    pub fn apply_completion(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        handle: BufferViewHandle,
        completion: &str,
        events: &mut EditorEventQueue,
    ) {
        let current_view = match &mut self.buffer_views[handle.0] {
            Some(view) => view,
            None => return,
        };
        let buffer = match buffers.get_mut(current_view.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        self.fix_cursor_ranges.clear();
        for cursor in current_view.cursors[..].iter().rev() {
            let mut word_position = cursor.position;
            word_position.column_byte_index = word_position.column_byte_index.saturating_sub(1);
            let word = buffer.content().word_at(word_position);
            let word_kind = word.kind;
            let word_position = word.position;

            if let WordKind::Identifier = word_kind {
                let range = BufferRange::between(word_position, cursor.position);
                buffer.delete_range(word_database, range, events);
            }

            let insert_range = buffer.insert_text(word_database, word_position, completion, events);
            let mut range = BufferRange::between(cursor.position, insert_range.to);
            if cursor.position > insert_range.to {
                std::mem::swap(&mut range.from, &mut range.to);
            }
            self.fix_cursor_ranges.push(range);
        }

        let current_buffer_handle = current_view.buffer_handle;
        self.fix_buffer_cursors(current_buffer_handle, |cursor, mut range| {
            if range.from <= range.to {
                cursor.insert(range);
            } else {
                std::mem::swap(&mut range.from, &mut range.to);
                cursor.delete(range);
            }
        });
    }

    fn fix_buffer_cursors(
        &mut self,
        buffer_handle: BufferHandle,
        op: fn(&mut Cursor, BufferRange),
    ) {
        for view in self.buffer_views.iter_mut().flatten() {
            if view.buffer_handle != buffer_handle {
                continue;
            }

            let ranges = &self.fix_cursor_ranges;
            for c in &mut view.cursors.mut_guard()[..] {
                for range in ranges.iter() {
                    op(c, *range);
                }
            }
        }
    }

    pub fn undo(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
        handle: BufferViewHandle,
    ) {
        if let Some(buffer) = self.buffer_views[handle.0]
            .as_mut()
            .and_then(|view| buffers.get_mut(view.buffer_handle))
        {
            self.apply_edits(handle, true, buffer.undo(word_database, events));
        }
    }

    pub fn redo(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
        handle: BufferViewHandle,
    ) {
        if let Some(buffer) = self.buffer_views[handle.0]
            .as_mut()
            .and_then(|view| buffers.get_mut(view.buffer_handle))
        {
            self.apply_edits(handle, false, buffer.redo(word_database, events));
        }
    }

    fn apply_edits<'a>(
        &mut self,
        handle: BufferViewHandle,
        fix_edit_ranges: bool,
        edits: impl 'a + Iterator<Item = Edit<'a>>,
    ) {
        let buffer_handle = match self.get(handle) {
            Some(view) => view.buffer_handle,
            None => return,
        };

        self.fix_cursor_ranges.clear();
        for edit in edits {
            match edit.kind {
                EditKind::Insert => {
                    if fix_edit_ranges {
                        for fix_range in &mut self.fix_cursor_ranges {
                            fix_range.from = fix_range.from.insert(edit.range);
                            fix_range.to = fix_range.to.insert(edit.range);
                        }
                    }
                    self.fix_cursor_ranges.push(edit.range);
                    for (i, view) in self.buffer_views.iter_mut().flatten().enumerate() {
                        if i != handle.0 && view.buffer_handle == buffer_handle {
                            for c in &mut view.cursors.mut_guard()[..] {
                                c.insert(edit.range);
                            }
                        }
                    }
                }
                EditKind::Delete => {
                    if fix_edit_ranges {
                        for fix_range in &mut self.fix_cursor_ranges {
                            fix_range.from = fix_range.from.delete(edit.range);
                            fix_range.to = fix_range.to.delete(edit.range);
                        }
                    }
                    let fix_range = BufferRange::between(edit.range.from, edit.range.from);
                    self.fix_cursor_ranges.push(fix_range);
                    for (i, view) in self.buffer_views.iter_mut().flatten().enumerate() {
                        if i != handle.0 && view.buffer_handle == buffer_handle {
                            for c in &mut view.cursors.mut_guard()[..] {
                                c.delete(edit.range);
                            }
                        }
                    }
                }
            }
        }

        if self.fix_cursor_ranges.is_empty() {
            return;
        }

        if let Some(view) = self.buffer_views[handle.0].as_mut() {
            let mut cursors = view.cursors.mut_guard();
            cursors.clear();
            for range in &self.fix_cursor_ranges {
                cursors.add(Cursor {
                    anchor: range.from,
                    position: range.to,
                });
            }
        }
        self.fix_cursor_ranges.clear();
    }

    pub fn buffer_view_handle_from_buffer_handle(
        &mut self,
        client_handle: ClientHandle,
        buffer_handle: BufferHandle,
    ) -> BufferViewHandle {
        let current_buffer_view_handle = self
            .iter_with_handles()
            .filter(|(_, view)| {
                view.buffer_handle == buffer_handle && view.client_handle == client_handle
            })
            .map(|(h, _)| h)
            .next();

        match current_buffer_view_handle {
            Some(handle) => handle,
            None => self.add(BufferView::new(client_handle, buffer_handle)),
        }
    }

    pub fn buffer_view_handle_from_path(
        &mut self,
        client_handle: ClientHandle,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        root: &Path,
        path: &Path,
        line_index: Option<usize>,
        events: &mut EditorEventQueue,
    ) -> Result<BufferViewHandle, BufferViewError> {
        pub fn try_set_line_index(
            buffer_views: &mut BufferViewCollection,
            buffers: &mut BufferCollection,
            handle: BufferViewHandle,
            line_index: Option<usize>,
        ) {
            let line_index = match line_index {
                Some(line_index) => line_index,
                None => return,
            };
            let view = match buffer_views.get_mut(handle) {
                Some(view) => view,
                None => return,
            };

            let mut cursors = view.cursors.mut_guard();
            let mut position = BufferPosition::line_col(line_index, 0);

            if let Some(buffer) = buffers.get(view.buffer_handle) {
                position = buffer.content().saturate_position(position);
            }

            cursors.clear();
            cursors.add(Cursor {
                anchor: position,
                position,
            });
        }

        if let Some(buffer) = buffers.find_with_path(root, path) {
            let buffer_handle = buffer.handle();
            let handle = self.buffer_view_handle_from_buffer_handle(client_handle, buffer_handle);
            try_set_line_index(self, buffers, handle, line_index);
            Ok(handle)
        } else if path.to_str().map(|s| !s.is_empty()).unwrap_or(false) {
            let path = path.strip_prefix(root).unwrap_or(path);

            let buffer = buffers.new(BufferCapabilities::text());
            buffer.set_path(Some(path));
            let _ = buffer.discard_and_reload_from_file(word_database, events);

            let buffer_view = BufferView::new(client_handle, buffer.handle());
            let handle = self.add(buffer_view);

            try_set_line_index(self, buffers, handle, line_index);
            Ok(handle)
        } else {
            Err(BufferViewError::InvalidPath)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestContext {
        pub word_database: WordDatabase,
        pub events: EditorEventQueue,
        pub buffers: BufferCollection,
        pub buffer_views: BufferViewCollection,
        pub buffer_view_handle: BufferViewHandle,
    }

    impl TestContext {
        pub fn with_buffer(text: &str) -> Self {
            let mut events = EditorEventQueue::default();
            let mut word_database = WordDatabase::new();

            let mut buffers = BufferCollection::default();
            let buffer = buffers.new(BufferCapabilities::text());
            buffer.insert_text(
                &mut word_database,
                BufferPosition::line_col(0, 0),
                text,
                &mut events,
            );

            let buffer_view =
                BufferView::new(ClientHandle::from_index(0).unwrap(), buffer.handle());

            let mut buffer_views = BufferViewCollection::default();
            let buffer_view_handle = buffer_views.add(buffer_view);

            Self {
                word_database,
                events,
                buffers,
                buffer_views,
                buffer_view_handle,
            }
        }
    }

    #[test]
    fn buffer_view_utf8_support() {
        let mut ctx = TestContext::with_buffer("");

        let buffer_view = ctx.buffer_views.get(ctx.buffer_view_handle).unwrap();
        let main_cursor = buffer_view.cursors.main_cursor();
        assert_eq!(BufferPosition::line_col(0, 0), main_cursor.anchor);
        assert_eq!(BufferPosition::line_col(0, 0), main_cursor.position);

        ctx.buffer_views.insert_text_at_cursor_positions(
            &mut ctx.buffers,
            &mut ctx.word_database,
            ctx.buffer_view_handle,
            "ç",
            &mut ctx.events,
        );

        let buffer_view = ctx.buffer_views.get(ctx.buffer_view_handle).unwrap();
        let main_cursor = buffer_view.cursors.main_cursor();
        assert_eq!(BufferPosition::line_col(0, 2), main_cursor.anchor);
        assert_eq!(BufferPosition::line_col(0, 2), main_cursor.position);
    }

    #[test]
    fn buffer_view_cursor_movement() {
        fn set_cursor(ctx: &mut TestContext, position: BufferPosition) {
            let buffer_view = ctx.buffer_views.get_mut(ctx.buffer_view_handle).unwrap();
            let mut cursors = buffer_view.cursors.mut_guard();
            cursors.clear();
            cursors.add(Cursor {
                anchor: position,
                position,
            });
        }

        fn main_cursor_position(ctx: &TestContext) -> BufferPosition {
            ctx.buffer_views
                .get(ctx.buffer_view_handle)
                .unwrap()
                .cursors
                .main_cursor()
                .position
        }

        fn assert_movement(
            ctx: &mut TestContext,
            from: (usize, usize),
            to: (usize, usize),
            movement: CursorMovement,
        ) {
            set_cursor(ctx, BufferPosition::line_col(from.0, from.1));
            ctx.buffer_views
                .get_mut(ctx.buffer_view_handle)
                .unwrap()
                .move_cursors(
                    &ctx.buffers,
                    movement,
                    CursorMovementKind::PositionAndAnchor,
                );
            assert_eq!(
                BufferPosition::line_col(to.0, to.1),
                main_cursor_position(ctx)
            );
        }

        let mut ctx = TestContext::with_buffer("ab\nc e\nefgh\ni k\nlm");
        assert_movement(&mut ctx, (2, 2), (2, 2), CursorMovement::ColumnsForward(0));
        assert_movement(&mut ctx, (2, 2), (2, 3), CursorMovement::ColumnsForward(1));
        assert_movement(&mut ctx, (2, 2), (2, 4), CursorMovement::ColumnsForward(2));
        assert_movement(&mut ctx, (2, 2), (3, 0), CursorMovement::ColumnsForward(3));
        assert_movement(&mut ctx, (2, 2), (3, 3), CursorMovement::ColumnsForward(6));
        assert_movement(&mut ctx, (2, 2), (4, 0), CursorMovement::ColumnsForward(7));
        assert_movement(
            &mut ctx,
            (2, 2),
            (4, 2),
            CursorMovement::ColumnsForward(999),
        );

        assert_movement(&mut ctx, (2, 2), (2, 2), CursorMovement::ColumnsBackward(0));
        assert_movement(&mut ctx, (2, 2), (2, 1), CursorMovement::ColumnsBackward(1));
        assert_movement(&mut ctx, (2, 0), (1, 3), CursorMovement::ColumnsBackward(1));
        assert_movement(&mut ctx, (2, 2), (1, 3), CursorMovement::ColumnsBackward(3));
        assert_movement(&mut ctx, (2, 2), (0, 2), CursorMovement::ColumnsBackward(7));
        assert_movement(
            &mut ctx,
            (2, 2),
            (0, 0),
            CursorMovement::ColumnsBackward(999),
        );

        assert_movement(&mut ctx, (2, 2), (2, 2), CursorMovement::WordsForward(0));
        assert_movement(&mut ctx, (2, 0), (2, 4), CursorMovement::WordsForward(1));
        assert_movement(&mut ctx, (2, 0), (3, 0), CursorMovement::WordsForward(2));
        assert_movement(&mut ctx, (2, 2), (3, 2), CursorMovement::WordsForward(3));
        assert_movement(&mut ctx, (2, 2), (3, 3), CursorMovement::WordsForward(4));
        assert_movement(&mut ctx, (2, 2), (4, 0), CursorMovement::WordsForward(5));
        assert_movement(&mut ctx, (2, 2), (4, 2), CursorMovement::WordsForward(6));
        assert_movement(&mut ctx, (2, 2), (4, 2), CursorMovement::WordsForward(999));

        assert_movement(&mut ctx, (2, 2), (2, 2), CursorMovement::WordsBackward(0));
        assert_movement(&mut ctx, (2, 0), (1, 3), CursorMovement::WordsBackward(1));
        assert_movement(&mut ctx, (2, 0), (1, 2), CursorMovement::WordsBackward(2));
        assert_movement(&mut ctx, (2, 2), (2, 0), CursorMovement::WordsBackward(1));
        assert_movement(&mut ctx, (2, 2), (1, 3), CursorMovement::WordsBackward(2));
        assert_movement(&mut ctx, (2, 2), (1, 2), CursorMovement::WordsBackward(3));
        assert_movement(&mut ctx, (2, 2), (1, 0), CursorMovement::WordsBackward(4));
        assert_movement(&mut ctx, (2, 2), (0, 2), CursorMovement::WordsBackward(5));
        assert_movement(&mut ctx, (2, 2), (0, 0), CursorMovement::WordsBackward(6));
        assert_movement(&mut ctx, (2, 2), (0, 0), CursorMovement::WordsBackward(999));

        let mut ctx = TestContext::with_buffer("123\n  abc def\nghi");
        assert_movement(&mut ctx, (1, 0), (1, 2), CursorMovement::WordsForward(1));
        assert_movement(&mut ctx, (1, 9), (2, 0), CursorMovement::WordsForward(1));
        assert_movement(&mut ctx, (1, 2), (1, 0), CursorMovement::WordsBackward(1));
        assert_movement(&mut ctx, (2, 0), (1, 9), CursorMovement::WordsBackward(1));
    }
}
