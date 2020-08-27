use crate::{
    buffer::{BufferCollection, BufferHandle, TextRef},
    buffer_position::{BufferOffset, BufferRange},
    connection::TargetClient,
    cursor::CursorCollection,
    editor_operation::{EditorOperation, EditorOperationSerializer},
    history::{EditKind, EditRef},
};

pub enum MovementKind {
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
        operations: &mut EditorOperationSerializer,
        offset: BufferOffset,
        movement_kind: MovementKind,
    ) {
        let buffer = match buffers.get(self.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        for c in &mut self.cursors.as_mut()[..] {
            let mut target = BufferOffset::from(c.position) + offset;

            target.line_offset = target
                .line_offset
                .min(buffer.content.line_count() as isize - 1)
                .max(0);
            let target_line_len = buffer.content.line(target.line_offset as _).char_count();
            target.column_offset = target.column_offset.min(target_line_len as _);

            c.position = target.into();
            if let MovementKind::PositionWithAnchor = movement_kind {
                c.anchor = c.position;
            }
        }

        operations.serialize_cursors(self.target_client, &self.cursors);
    }

    pub fn collapse_cursors_anchors(&mut self, operations: &mut EditorOperationSerializer) {
        self.cursors.collapse_anchors();
        operations.serialize_cursors(self.target_client, &self.cursors);
    }

    pub fn swap_cursors_positions_and_anchors(
        &mut self,
        operations: &mut EditorOperationSerializer,
    ) {
        self.cursors.swap_positions_and_anchors();
        operations.serialize_cursors(self.target_client, &self.cursors);
    }

    pub fn move_to_next_search_match(
        &mut self,
        buffers: &BufferCollection,
        operations: &mut EditorOperationSerializer,
        movement_kind: MovementKind,
    ) {
        self.move_to_search_match(buffers, operations, movement_kind, |result, len| {
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
        operations: &mut EditorOperationSerializer,
        movement_kind: MovementKind,
    ) {
        self.move_to_search_match(buffers, operations, movement_kind, |result, len| {
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
        operations: &mut EditorOperationSerializer,
        movement_kind: MovementKind,
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
            let mut cursors = self.cursors.as_mut();
            for c in &mut cursors[..] {
                c.position = search_ranges[next_index].from;
            }

            if let MovementKind::PositionWithAnchor = movement_kind {
                for c in &mut cursors[..] {
                    c.anchor = c.position;
                }
            }
        }

        operations.serialize_cursors(self.target_client, &self.cursors);
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

#[derive(Eq, PartialEq)]
pub struct BufferViewHandle(usize);

#[derive(Default)]
pub struct BufferViewCollection {
    buffer_views: Vec<Option<BufferView>>,
    temp_ranges: Vec<BufferRange>,
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

    pub fn remove_where<F>(&mut self, predicate: F)
    where
        F: Fn(&BufferView) -> bool,
    {
        for i in 0..self.buffer_views.len() {
            if let Some(view) = &self.buffer_views[i] {
                if predicate(&view) {
                    self.buffer_views[i] = None;
                }
            }
        }
    }

    pub fn get(&self, handle: &BufferViewHandle) -> &BufferView {
        self.buffer_views[handle.0].as_ref().unwrap()
    }

    pub fn get_mut(&mut self, handle: &BufferViewHandle) -> &mut BufferView {
        self.buffer_views[handle.0].as_mut().unwrap()
    }

    pub fn iter(&self) -> impl Iterator<Item = &BufferView> {
        self.buffer_views.iter().flatten()
    }

    pub fn iter_with_handles(&self) -> impl Iterator<Item = (BufferViewHandle, &BufferView)> {
        self.buffer_views
            .iter()
            .enumerate()
            .filter_map(|(i, v)| Some(BufferViewHandle(i)).zip(v.as_ref()))
    }

    pub fn insert_text(
        &mut self,
        buffers: &mut BufferCollection,
        operations: &mut EditorOperationSerializer,
        handle: &BufferViewHandle,
        text: TextRef,
    ) {
        let current_view = match &mut self.buffer_views[handle.0] {
            Some(view) => view,
            None => return,
        };
        let buffer = match buffers.get_mut(current_view.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        self.temp_ranges.clear();
        for cursor in current_view.cursors[..].iter().rev() {
            let range = buffer.insert_text(cursor.position, text);
            self.temp_ranges.push(range);
        }

        let current_buffer_handle = current_view.buffer_handle;
        for view in self.buffer_views.iter_mut().flatten() {
            if view.buffer_handle != current_buffer_handle {
                continue;
            }

            let ranges = &self.temp_ranges;
            for c in &mut view.cursors.as_mut()[..] {
                for range in ranges {
                    c.insert(*range);
                }
            }

            for range in ranges {
                operations.serialize_insert(view.target_client, range.from, text);
            }
            operations.serialize_cursors(view.target_client, &view.cursors);
        }
    }

    pub fn delete_in_selection(
        &mut self,
        buffers: &mut BufferCollection,
        operations: &mut EditorOperationSerializer,
        handle: &BufferViewHandle,
    ) {
        let current_view = match &mut self.buffer_views[handle.0] {
            Some(view) => view,
            None => return,
        };
        let buffer = match buffers.get_mut(current_view.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        self.temp_ranges.clear();
        for cursor in current_view.cursors[..].iter().rev() {
            let range = cursor.range();
            buffer.delete_range(range);
            self.temp_ranges.push(range);
        }

        let current_buffer_handle = current_view.buffer_handle;
        for view in self.buffer_views.iter_mut().flatten() {
            if view.buffer_handle != current_buffer_handle {
                continue;
            }

            let ranges = &self.temp_ranges;
            for c in &mut view.cursors.as_mut()[..] {
                for range in ranges.iter() {
                    c.delete(*range);
                }
            }

            for range in ranges {
                operations.serialize(view.target_client, &EditorOperation::Delete(*range));
            }
            operations.serialize_cursors(view.target_client, &view.cursors);
        }
    }

    pub fn undo(
        &mut self,
        buffers: &mut BufferCollection,
        operations: &mut EditorOperationSerializer,
        handle: &BufferViewHandle,
    ) {
        if let Some(buffer) = self.buffer_views[handle.0]
            .as_mut()
            .and_then(|view| buffers.get_mut(view.buffer_handle))
        {
            self.apply_edits(handle, operations, buffer.undo());
        }
    }

    pub fn redo(
        &mut self,
        buffers: &mut BufferCollection,
        operations: &mut EditorOperationSerializer,
        handle: &BufferViewHandle,
    ) {
        if let Some(buffer) = self.buffer_views[handle.0]
            .as_mut()
            .and_then(|view| buffers.get_mut(view.buffer_handle))
        {
            self.apply_edits(handle, operations, buffer.redo());
        }
    }

    fn apply_edits<'a>(
        &mut self,
        handle: &BufferViewHandle,
        operations: &mut EditorOperationSerializer,
        edits: impl 'a + Iterator<Item = EditRef<'a>>,
    ) {
        let buffer_handle = self.get(handle).buffer_handle;

        for edit in edits {
            match edit.kind {
                EditKind::Insert => {
                    for c in &mut self.get_mut(handle).cursors.as_mut()[..] {
                        c.anchor = edit.range.to;
                        c.position = edit.range.to;
                    }
                    for (i, view) in self.buffer_views.iter_mut().flatten().enumerate() {
                        if i != handle.0 && view.buffer_handle == buffer_handle {
                            for c in &mut view.cursors.as_mut()[..] {
                                c.insert(edit.range);
                            }
                        }
                        operations.serialize_insert(view.target_client, edit.range.from, edit.text);
                    }
                }
                EditKind::Delete => {
                    for c in &mut self.get_mut(handle).cursors.as_mut()[..] {
                        c.anchor = edit.range.from;
                        c.position = edit.range.from;
                    }
                    for (i, view) in self.buffer_views.iter_mut().flatten().enumerate() {
                        if i != handle.0 && view.buffer_handle == buffer_handle {
                            for c in &mut view.cursors.as_mut()[..] {
                                c.delete(edit.range);
                            }
                        }
                        operations
                            .serialize(view.target_client, &EditorOperation::Delete(edit.range));
                    }
                }
            }
        }

        for view in self.buffer_views.iter().flatten() {
            operations.serialize_cursors(view.target_client, &view.cursors);
        }
    }
}
