use std::{fs::File, io::Read, path::Path};

use crate::{
    buffer::{Buffer, BufferCollection, BufferContent, BufferHandle},
    buffer_position::{BufferOffset, BufferRange},
    client::TargetClient,
    cursor::{Cursor, CursorCollection},
    history::{Edit, EditKind},
    syntax::SyntaxCollection,
    word_database::WordDatabase,
};

pub enum CursorMovement {
    Column(isize),
    Line(isize),
    Word(isize),
}

pub enum CursorMovementKind {
    PositionWithAnchor,
    PositionOnly,
}

pub struct BufferView {
    pub target_client: TargetClient,
    pub buffer_handle: BufferHandle,
    pub cursors: CursorCollection,
}

impl BufferView {
    pub fn new(target_client: TargetClient, buffer_handle: BufferHandle) -> Self {
        Self {
            target_client,
            buffer_handle,
            cursors: CursorCollection::new(),
        }
    }

    pub fn clone_with_target_client(&self, target_client: TargetClient) -> Self {
        Self {
            target_client,
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
        let buffer = match buffers.get(self.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        let mut cursors = self.cursors.mut_guard();
        for c in &mut cursors[..] {
            let offset = match movement {
                CursorMovement::Column(n) => BufferOffset::line_col(0, n),
                CursorMovement::Line(n) => BufferOffset::line_col(n, 0),
                CursorMovement::Word(n) => BufferOffset::line_col(0, 0),
            };
            Self::move_cursor(c, buffer, offset);
        }

        if let CursorMovementKind::PositionWithAnchor = movement_kind {
            for c in &mut cursors[..] {
                c.anchor = c.position;
            }
        }
    }

    fn move_cursor(cursor: &mut Cursor, buffer: &Buffer, offset: BufferOffset) {
        let last_line_index = buffer.content.line_count() as isize - 1;
        let mut target = BufferOffset::from(cursor.position) + offset;
        macro_rules! line_count {
            ($index:expr) => {
                buffer.content.line_at($index as _).char_count() as isize
            };
        }

        loop {
            if target.line_offset < 0 {
                target.line_offset = 0;
                target.column_offset = target.column_offset.max(line_count!(target.line_offset));
                break;
            } else if target.line_offset > last_line_index {
                target.line_offset = last_line_index;
                target.column_offset = target.column_offset.max(line_count!(target.line_offset));
                break;
            }

            if target.column_offset < 0 {
                target.line_offset = 0.max(target.line_offset - 1);
                target.column_offset += line_count!(target.line_offset);
            } else {
                let current_line_count = line_count!(target.line_offset);
                if target.column_offset > current_line_count {
                    target.column_offset -= current_line_count;
                    target.line_offset = last_line_index.min(target.line_offset + 1);
                } else {
                    break;
                }
            }
        }

        cursor.position = target.into();
    }

    pub fn move_to_next_search_match(
        &mut self,
        buffers: &BufferCollection,
        movement_kind: CursorMovementKind,
    ) {
        self.move_to_search_match(buffers, movement_kind, |result, len| {
            let next_index = match result {
                Ok(index) => index + 1,
                Err(index) => index,
            };
            next_index % len
        });
    }

    pub fn move_to_previous_search_match(
        &mut self,
        buffers: &BufferCollection,
        movement_kind: CursorMovementKind,
    ) {
        self.move_to_search_match(buffers, movement_kind, |result, len| {
            let next_index = match result {
                Ok(index) => index,
                Err(index) => index,
            };
            (next_index + len - 1) % len
        });
    }

    fn move_to_search_match<F>(
        &mut self,
        buffers: &BufferCollection,
        movement_kind: CursorMovementKind,
        index_selector: F,
    ) where
        F: FnOnce(Result<usize, usize>, usize) -> usize,
    {
        let buffer = match buffers.get(self.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        let main_position = self.cursors.main_cursor().position;
        let search_ranges = buffer.search_ranges();
        if search_ranges.is_empty() {
            return;
        }

        let search_result = search_ranges.binary_search_by_key(&main_position, |r| r.from);
        let next_index = index_selector(search_result, search_ranges.len());

        {
            let mut cursors = self.cursors.mut_guard();
            for c in &mut cursors[..] {
                c.position = search_ranges[next_index].from;
            }

            if let CursorMovementKind::PositionWithAnchor = movement_kind {
                for c in &mut cursors[..] {
                    c.anchor = c.position;
                }
            }
        }
    }

    pub fn commit_edits(&self, buffers: &mut BufferCollection) {
        if let Some(buffer) = buffers.get_mut(self.buffer_handle) {
            buffer.history.commit_edits();
        }
    }

    pub fn get_selection_text(&self, buffers: &BufferCollection, text: &mut String) {
        text.clear();

        let buffer = match buffers.get(self.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        let mut iter = self.cursors[..].iter();
        if let Some(cursor) = iter.next() {
            let mut last_range = cursor.range();
            buffer.content.append_range_text_to_string(last_range, text);
            for cursor in iter {
                let range = cursor.range();
                if range.from.line_index > last_range.to.line_index {
                    text.push('\n');
                }
                buffer.content.append_range_text_to_string(range, text);
                last_range = range;
            }
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct BufferViewHandle(usize);

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

    pub fn remove_where<F>(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        predicate: F,
    ) where
        F: Fn(&BufferView) -> bool,
    {
        for i in 0..self.buffer_views.len() {
            if let Some(view) = &self.buffer_views[i] {
                if predicate(&view) {
                    self.buffer_views[i] = None;
                }
            }
        }

        buffers.remove_where(word_database, |h, _b| {
            !self.iter().any(|v| v.buffer_handle == h)
        });
    }

    pub fn get(&self, handle: BufferViewHandle) -> Option<&BufferView> {
        self.buffer_views[handle.0].as_ref()
    }

    pub fn get_mut(&mut self, handle: BufferViewHandle) -> Option<&mut BufferView> {
        self.buffer_views[handle.0].as_mut()
    }

    pub fn iter(&self) -> impl Iterator<Item = &BufferView> {
        self.buffer_views.iter().flatten()
    }

    fn iter_with_handles(&self) -> impl Iterator<Item = (BufferViewHandle, &BufferView)> {
        self.buffer_views
            .iter()
            .enumerate()
            .filter_map(|(i, v)| Some(BufferViewHandle(i)).zip(v.as_ref()))
    }

    pub fn insert_text(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        handle: BufferViewHandle,
        text: &str,
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
        for (i, cursor) in current_view.cursors[..].iter().enumerate().rev() {
            let range = buffer.insert_text(word_database, syntaxes, cursor.position, text, i);
            self.fix_cursor_ranges.push(range);
        }

        let current_buffer_handle = current_view.buffer_handle;
        self.fix_buffer_cursors(current_buffer_handle, |cursor, range| cursor.insert(range));
    }

    pub fn delete_in_selection(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        handle: BufferViewHandle,
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
        for (i, cursor) in current_view.cursors[..].iter().enumerate().rev() {
            let range = cursor.range();
            buffer.delete_range(word_database, syntaxes, range, i);
            self.fix_cursor_ranges.push(range);
        }

        let current_buffer_handle = current_view.buffer_handle;
        self.fix_buffer_cursors(current_buffer_handle, |cursor, range| cursor.delete(range));
    }

    pub fn apply_completion(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        handle: BufferViewHandle,
        completion: &str,
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
        for (i, cursor) in current_view.cursors[..].iter().enumerate().rev() {
            let (word_range, word) = buffer.content.find_word_at(cursor.position);

            if !word.is_empty() {
                let range = BufferRange::between(word_range.from, cursor.position);
                buffer.delete_range(word_database, syntaxes, range, i);
            }

            let insert_range =
                buffer.insert_text(word_database, syntaxes, word_range.from, completion, i);
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
        syntaxes: &SyntaxCollection,
        handle: BufferViewHandle,
    ) {
        if let Some(buffer) = self.buffer_views[handle.0]
            .as_mut()
            .and_then(|view| buffers.get_mut(view.buffer_handle))
        {
            self.apply_edits(handle, buffer.undo(syntaxes));
        }
    }

    pub fn redo(
        &mut self,
        buffers: &mut BufferCollection,
        syntaxes: &SyntaxCollection,
        handle: BufferViewHandle,
    ) {
        if let Some(buffer) = self.buffer_views[handle.0]
            .as_mut()
            .and_then(|view| buffers.get_mut(view.buffer_handle))
        {
            self.apply_edits(handle, buffer.redo(syntaxes));
        }
    }

    fn apply_edits<'a>(
        &mut self,
        handle: BufferViewHandle,
        edits: impl 'a + Iterator<Item = Edit<'a>>,
    ) {
        let buffer_handle = match self.get(handle) {
            Some(view) => view.buffer_handle,
            None => return,
        };

        self.fix_cursor_ranges.clear();
        for edit in edits {
            let cursor_index = edit.cursor_index as usize;
            if cursor_index >= self.fix_cursor_ranges.len() {
                self.fix_cursor_ranges
                    .resize(cursor_index + 1, BufferRange::default());
            }

            match edit.kind {
                EditKind::Insert => {
                    self.fix_cursor_ranges[cursor_index].from = edit.range.to;
                    for (i, view) in self.buffer_views.iter_mut().flatten().enumerate() {
                        if i != handle.0 && view.buffer_handle == buffer_handle {
                            for c in &mut view.cursors.mut_guard()[..] {
                                c.insert(edit.range);
                            }
                        }
                    }
                }
                EditKind::Delete => {
                    self.fix_cursor_ranges[cursor_index].from = edit.range.from;
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

        if let Some(view) = self.buffer_views[handle.0].as_mut() {
            if !self.fix_cursor_ranges.is_empty() {
                let mut cursors = view.cursors.mut_guard();
                cursors.clear();

                for range in &self.fix_cursor_ranges {
                    cursors.add_cursor(Cursor {
                        anchor: range.from,
                        position: range.from,
                    });
                }
            }
        }
    }

    pub fn new_buffer_from_file(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        syntaxes: &SyntaxCollection,
        target_client: TargetClient,
        path: &Path,
    ) -> Result<BufferViewHandle, String> {
        if let Some(buffer_handle) = buffers.find_with_path(path) {
            let mut iter = self.iter_with_handles().filter(|(_handle, view)| {
                view.buffer_handle == buffer_handle && view.target_client == target_client
            });

            let buffer_view_handle = match iter.next() {
                Some((handle, _view)) => handle,
                None => {
                    drop(iter);
                    let view = BufferView::new(target_client, buffer_handle);
                    self.add(view)
                }
            };
            Ok(buffer_view_handle)
        } else if path.to_str().map(|s| s.trim().len()).unwrap_or(0) > 0 {
            let content = match File::open(&path) {
                Ok(mut file) => {
                    let mut content = String::new();
                    match file.read_to_string(&mut content) {
                        Ok(_) => (),
                        Err(error) => {
                            return Err(format!(
                                "could not read contents from file {:?}: {:?}",
                                path, error
                            ))
                        }
                    }
                    BufferContent::from_str(&content)
                }
                Err(_) => BufferContent::from_str(""),
            };

            let buffer_handle =
                buffers.add(Buffer::new(word_database, syntaxes, path.into(), content));
            let buffer_view = BufferView::new(target_client, buffer_handle);
            let buffer_view_handle = self.add(buffer_view);
            Ok(buffer_view_handle)
        } else {
            Err(format!("invalid path {:?}", path))
        }
    }
}
