use std::{fmt, num::NonZeroU8, str::FromStr};

use crate::{
    buffer::{Buffer, BufferCollection, BufferHandle, CharDisplayDistances},
    buffer_position::{BufferPosition, BufferPositionIndex, BufferRange},
    client::ClientHandle,
    cursor::{Cursor, CursorCollection},
    events::EditorEventQueue,
    history::EditKind,
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
    alive: bool,
    handle: BufferViewHandle,
    pub client_handle: ClientHandle,
    pub buffer_handle: BufferHandle,
    pub cursors: CursorCollection,
}

impl BufferView {
    fn reset(&mut self, client_handle: ClientHandle, buffer_handle: BufferHandle) {
        self.alive = true;
        self.client_handle = client_handle;
        self.buffer_handle = buffer_handle;
        self.cursors.mut_guard().clear();
    }

    pub fn move_cursors(
        &mut self,
        buffers: &BufferCollection,
        movement: CursorMovement,
        movement_kind: CursorMovementKind,
        tab_size: NonZeroU8,
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
                    let line = buffer.line_at(c.position.line_index as _).as_str();
                    match try_nth(
                        line[c.position.column_byte_index as usize..].char_indices(),
                        n,
                    ) {
                        Ok((i, _)) => c.position.column_byte_index += i as BufferPositionIndex,
                        Err(0) => c.position.column_byte_index = line.len() as _,
                        Err(mut n) => {
                            n -= 1;
                            loop {
                                if c.position.line_index == last_line_index as _ {
                                    c.position.column_byte_index =
                                        buffer.line_at(last_line_index).as_str().len() as _;
                                    break;
                                }

                                c.position.line_index += 1;
                                let line = buffer.line_at(c.position.line_index as _).as_str();
                                match try_nth(line.char_indices(), n) {
                                    Ok((i, _)) => {
                                        c.position.column_byte_index = i as _;
                                        break;
                                    }
                                    Err(0) => {
                                        c.position.column_byte_index = line.len() as _;
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
                    let line = buffer.line_at(c.position.line_index as _).as_str();
                    match try_nth(
                        line[..c.position.column_byte_index as usize]
                            .char_indices()
                            .rev(),
                        n,
                    ) {
                        Ok((i, _)) => c.position.column_byte_index = i as _,
                        Err(0) => {
                            if c.position.line_index == 0 {
                                c.position.column_byte_index = 0;
                            } else {
                                c.position.line_index -= 1;
                                c.position.column_byte_index =
                                    buffer.line_at(c.position.line_index as _).as_str().len() as _;
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
                                let line = buffer.line_at(c.position.line_index as _).as_str();
                                match try_nth(line.char_indices().rev(), n) {
                                    Ok((i, _)) => {
                                        c.position.column_byte_index = i as _;
                                        break;
                                    }
                                    Err(0) => {
                                        if c.position.line_index == 0 {
                                            c.position.column_byte_index = 0;
                                        } else {
                                            c.position.line_index -= 1;
                                            c.position.column_byte_index = buffer
                                                .line_at(c.position.line_index as _)
                                                .as_str()
                                                .len()
                                                as _;
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
                cursors.save_display_distances(buffer, tab_size);
                for i in 0..cursors[..].len() {
                    let saved_display_distance = cursors.get_saved_display_distance(i);
                    let c = &mut cursors[i];
                    c.position.line_index = buffer
                        .line_count()
                        .saturating_sub(1)
                        .min(c.position.line_index as usize + n)
                        as _;
                    if let Some(distance) = saved_display_distance {
                        let line = buffer.line_at(c.position.line_index as _).as_str();
                        c.position.column_byte_index = CharDisplayDistances::new(line, tab_size)
                            .skip_while(|d| d.distance <= distance as _)
                            .next()
                            .map(|d| d.char_index)
                            .unwrap_or(line.len())
                            as _;
                    }
                    c.position = buffer.saturate_position(c.position);
                }
            }
            CursorMovement::LinesBackward(n) => {
                cursors.save_display_distances(buffer, tab_size);
                for i in 0..cursors[..].len() {
                    let saved_display_distance = cursors.get_saved_display_distance(i);
                    let c = &mut cursors[i];
                    c.position.line_index = c.position.line_index.saturating_sub(n as _);
                    if let Some(distance) = saved_display_distance {
                        let line = buffer.line_at(c.position.line_index as _).as_str();
                        c.position.column_byte_index = CharDisplayDistances::new(line, tab_size)
                            .skip_while(|d| d.distance <= distance as _)
                            .next()
                            .map(|d| d.char_index)
                            .unwrap_or(line.len())
                            as _;
                    }
                    c.position = buffer.saturate_position(c.position);
                }
            }
            CursorMovement::WordsForward(n) => {
                let last_line_index = buffer.line_count() - 1;
                for c in &mut cursors[..] {
                    let mut n = n;
                    let mut line = buffer.line_at(c.position.line_index as _).as_str();

                    while n > 0 {
                        if c.position.column_byte_index == line.len() as _ {
                            if c.position.line_index == last_line_index as _ {
                                break;
                            }

                            c.position.line_index += 1;
                            c.position.column_byte_index = 0;
                            line = buffer.line_at(c.position.line_index as _).as_str();
                            n -= 1;
                            continue;
                        }

                        let words = WordIter(&line[c.position.column_byte_index as usize..])
                            .inspect(|w| {
                                c.position.column_byte_index += w.text.len() as BufferPositionIndex
                            })
                            .skip(1)
                            .filter(|w| w.kind != WordKind::Whitespace);

                        match try_nth(words, n - 1) {
                            Ok(word) => {
                                c.position.column_byte_index -=
                                    word.text.len() as BufferPositionIndex;
                                break;
                            }
                            Err(rest) => {
                                n = rest;
                                c.position.column_byte_index = line.len() as _;
                            }
                        }
                    }
                }
            }
            CursorMovement::WordsBackward(n) => {
                for c in &mut cursors[..] {
                    let mut n = n;
                    let mut line = &buffer.line_at(c.position.line_index as _).as_str()
                        [..c.position.column_byte_index as usize];

                    while n > 0 {
                        let mut last_kind = WordKind::Identifier;
                        let words = WordIter(line)
                            .rev()
                            .inspect(|w| {
                                c.position.column_byte_index -= w.text.len() as BufferPositionIndex;
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
                        line = buffer.line_at(c.position.line_index as _).as_str();
                        c.position.column_byte_index = line.len() as _;
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
                    let first_word = buffer.line_at(c.position.line_index as _).word_at(0);
                    match first_word.kind {
                        WordKind::Whitespace => {
                            c.position.column_byte_index = first_word.text.len() as _
                        }
                        _ => c.position.column_byte_index = 0,
                    }
                }
            }
            CursorMovement::End => {
                for c in &mut cursors[..] {
                    c.position.column_byte_index =
                        buffer.line_at(c.position.line_index as _).as_str().len() as _;
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
                    c.position.line_index = (buffer.line_count() - 1) as _;
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

    pub fn append_selection_text(
        &self,
        buffers: &BufferCollection,
        text: &mut String,
        ranges: &mut Vec<(u32, u32)>,
    ) {
        ranges.clear();

        let buffer = match buffers.get(self.buffer_handle) {
            Some(buffer) => buffer.content(),
            None => return,
        };

        let mut iter = self.cursors[..].iter();
        if let Some(cursor) = iter.next() {
            let mut last_range = cursor.to_range();
            let from = text.len() as _;
            buffer.append_range_text_to_string(last_range, text);
            ranges.push((from, text.len() as _));
            for cursor in iter {
                let range = cursor.to_range();
                if range.from.line_index > last_range.to.line_index {
                    text.push('\n');
                }
                let from = text.len() as _;
                buffer.append_range_text_to_string(range, text);
                ranges.push((from, text.len() as _));
                last_range = range;
            }
        }
    }

    pub fn insert_text_at_cursor_positions(
        &self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        text: &str,
        events: &mut EditorEventQueue,
    ) {
        if let Some(buffer) = buffers.get_mut(self.buffer_handle) {
            for cursor in self.cursors[..].iter().rev() {
                buffer.insert_text(word_database, cursor.position, text, events);
            }
        }
    }

    pub fn delete_text_in_cursor_ranges(
        &self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
    ) {
        if let Some(buffer) = buffers.get_mut(self.buffer_handle) {
            for cursor in self.cursors[..].iter().rev() {
                buffer.delete_range(word_database, cursor.to_range(), events);
            }
        }
    }

    pub fn find_completion_positions(
        &self,
        buffers: &mut BufferCollection,
        positions: &mut Vec<BufferPosition>,
    ) {
        positions.clear();
        let buffer = match buffers.get_mut(self.buffer_handle) {
            Some(buffer) => buffer.content(),
            None => return,
        };

        for cursor in self.cursors[..].iter() {
            let position = buffer.position_before(cursor.position);
            let word = buffer.word_at(position);
            match word.kind {
                WordKind::Identifier => positions.push(word.position),
                _ => positions.push(cursor.position),
            }
        }
    }

    pub fn apply_completion(
        &self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        completion: &str,
        positions: &[BufferPosition],
        events: &mut EditorEventQueue,
    ) {
        let buffer = match buffers.get_mut(self.buffer_handle) {
            Some(buffer) => buffer,
            None => return,
        };

        for (cursor, &position) in self.cursors[..].iter().zip(positions.iter()).rev() {
            let range = BufferRange::between(position, cursor.position);
            buffer.delete_range(word_database, range, events);
            buffer.insert_text(word_database, position, completion, events);
        }
    }

    pub fn undo(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
    ) {
        let edits = match buffers.get_mut(self.buffer_handle) {
            Some(buffer) => buffer.undo(word_database, events),
            None => return,
        };

        let mut cursors = self.cursors.mut_guard();
        let mut last_edit_kind = None;
        for edit in edits {
            if last_edit_kind != Some(edit.kind) {
                cursors.clear();
            }
            let position = match edit.kind {
                EditKind::Insert => edit.range.to,
                EditKind::Delete => edit.range.from,
            };
            cursors.add(Cursor {
                anchor: edit.range.from,
                position,
            });
            last_edit_kind = Some(edit.kind);
        }

        events.enqueue_fix_cursors(self.handle, &cursors[..]);
        cursors.clear();
    }

    pub fn redo(
        &mut self,
        buffers: &mut BufferCollection,
        word_database: &mut WordDatabase,
        events: &mut EditorEventQueue,
    ) {
        let edits = match buffers.get_mut(self.buffer_handle) {
            Some(buffer) => buffer.redo(word_database, events),
            None => return,
        };

        let mut cursors = self.cursors.mut_guard();
        let mut last_edit_kind = None;
        for edit in edits {
            if last_edit_kind != Some(edit.kind) {
                cursors.clear();
            }
            let position = match edit.kind {
                EditKind::Insert => {
                    for cursor in &mut cursors[..] {
                        cursor.insert(edit.range);
                    }
                    edit.range.to
                }
                EditKind::Delete => {
                    for cursor in &mut cursors[..] {
                        cursor.delete(edit.range);
                    }
                    edit.range.from
                }
            };
            cursors.add(Cursor {
                anchor: edit.range.from,
                position,
            });
            last_edit_kind = Some(edit.kind);
        }

        events.enqueue_fix_cursors(self.handle, &cursors[..]);
        cursors.clear();
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct BufferViewHandle(u32);
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
    buffer_views: Vec<BufferView>,
}

impl BufferViewCollection {
    pub fn add_new(
        &mut self,
        client_handle: ClientHandle,
        buffer_handle: BufferHandle,
    ) -> BufferViewHandle {
        for (i, view) in self.buffer_views.iter_mut().enumerate() {
            if !view.alive {
                view.reset(client_handle, buffer_handle);
                return BufferViewHandle(i as _);
            }
        }
        let handle = BufferViewHandle(self.buffer_views.len() as _);
        self.buffer_views.push(BufferView {
            alive: true,
            handle,
            client_handle,
            buffer_handle,
            cursors: CursorCollection::new(),
        });
        handle
    }

    pub fn remove_buffer_views(&mut self, buffer_handle: BufferHandle) {
        for view in &mut self.buffer_views {
            if view.alive && view.buffer_handle == buffer_handle {
                view.alive = false;
            }
        }
    }

    pub fn get(&self, handle: BufferViewHandle) -> Option<&BufferView> {
        let view = &self.buffer_views[handle.0 as usize];
        if view.alive {
            Some(view)
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, handle: BufferViewHandle) -> Option<&mut BufferView> {
        let view = &mut self.buffer_views[handle.0 as usize];
        if view.alive {
            Some(view)
        } else {
            None
        }
    }

    pub fn on_buffer_load(&mut self, buffer: &Buffer) {
        let buffer_handle = buffer.handle();
        let buffer = buffer.content();

        for view in self.buffer_views.iter_mut().filter(|v| v.alive) {
            if view.buffer_handle == buffer_handle {
                for c in &mut view.cursors.mut_guard()[..] {
                    c.anchor = buffer.saturate_position(c.anchor);
                    c.position = buffer.saturate_position(c.position);
                }
            }
        }
    }

    pub fn on_buffer_insert_text(&mut self, buffer_handle: BufferHandle, range: BufferRange) {
        for view in self.buffer_views.iter_mut().filter(|v| v.alive) {
            if view.buffer_handle == buffer_handle {
                for c in &mut view.cursors.mut_guard()[..] {
                    c.insert(range);
                }
            }
        }
    }

    pub fn on_buffer_delete_text(&mut self, buffer_handle: BufferHandle, range: BufferRange) {
        for view in self.buffer_views.iter_mut().filter(|v| v.alive) {
            if view.buffer_handle == buffer_handle {
                for c in &mut view.cursors.mut_guard()[..] {
                    c.delete(range);
                }
            }
        }
    }

    pub fn buffer_view_handle_from_buffer_handle(
        &mut self,
        client_handle: ClientHandle,
        buffer_handle: BufferHandle,
    ) -> BufferViewHandle {
        let current_buffer_view_handle = self
            .buffer_views
            .iter()
            .position(|v| {
                v.alive && v.buffer_handle == buffer_handle && v.client_handle == client_handle
            })
            .map(|i| BufferViewHandle(i as _));

        match current_buffer_view_handle {
            Some(handle) => handle,
            None => self.add_new(client_handle, buffer_handle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ops::Range;

    use crate::{buffer::BufferCapabilities, buffer_position::BufferPosition};

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
            let buffer = buffers.add_new();
            buffer.capabilities = BufferCapabilities::text();
            buffer.insert_text(
                &mut word_database,
                BufferPosition::zero(),
                text,
                &mut events,
            );

            let mut buffer_views = BufferViewCollection::default();
            let buffer_view_handle =
                buffer_views.add_new(ClientHandle::from_index(0).unwrap(), buffer.handle());

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
            from: Range<usize>,
            to: Range<usize>,
            movement: CursorMovement,
        ) {
            set_cursor(
                ctx,
                BufferPosition::line_col(from.start as _, from.end as _),
            );
            ctx.buffer_views
                .get_mut(ctx.buffer_view_handle)
                .unwrap()
                .move_cursors(
                    &ctx.buffers,
                    movement,
                    CursorMovementKind::PositionAndAnchor,
                    NonZeroU8::new(4).unwrap(),
                );
            assert_eq!(
                BufferPosition::line_col(to.start as _, to.end as _),
                main_cursor_position(ctx)
            );
        }

        let mut ctx = TestContext::with_buffer("ab\nc e\nefgh\ni k\nlm");
        assert_movement(&mut ctx, 2..2, 2..2, CursorMovement::ColumnsForward(0));
        assert_movement(&mut ctx, 2..2, 2..3, CursorMovement::ColumnsForward(1));
        assert_movement(&mut ctx, 2..2, 2..4, CursorMovement::ColumnsForward(2));
        assert_movement(&mut ctx, 2..2, 3..0, CursorMovement::ColumnsForward(3));
        assert_movement(&mut ctx, 2..2, 3..3, CursorMovement::ColumnsForward(6));
        assert_movement(&mut ctx, 2..2, 4..0, CursorMovement::ColumnsForward(7));
        assert_movement(&mut ctx, 2..2, 4..2, CursorMovement::ColumnsForward(999));

        assert_movement(&mut ctx, 2..2, 2..2, CursorMovement::ColumnsBackward(0));
        assert_movement(&mut ctx, 2..2, 2..1, CursorMovement::ColumnsBackward(1));
        assert_movement(&mut ctx, 2..0, 1..3, CursorMovement::ColumnsBackward(1));
        assert_movement(&mut ctx, 2..2, 1..3, CursorMovement::ColumnsBackward(3));
        assert_movement(&mut ctx, 2..2, 0..2, CursorMovement::ColumnsBackward(7));
        assert_movement(&mut ctx, 2..2, 0..0, CursorMovement::ColumnsBackward(999));

        assert_movement(&mut ctx, 2..2, 2..2, CursorMovement::WordsForward(0));
        assert_movement(&mut ctx, 2..0, 2..4, CursorMovement::WordsForward(1));
        assert_movement(&mut ctx, 2..0, 3..0, CursorMovement::WordsForward(2));
        assert_movement(&mut ctx, 2..2, 3..2, CursorMovement::WordsForward(3));
        assert_movement(&mut ctx, 2..2, 3..3, CursorMovement::WordsForward(4));
        assert_movement(&mut ctx, 2..2, 4..0, CursorMovement::WordsForward(5));
        assert_movement(&mut ctx, 2..2, 4..2, CursorMovement::WordsForward(6));
        assert_movement(&mut ctx, 2..2, 4..2, CursorMovement::WordsForward(999));

        assert_movement(&mut ctx, 2..2, 2..2, CursorMovement::WordsBackward(0));
        assert_movement(&mut ctx, 2..0, 1..3, CursorMovement::WordsBackward(1));
        assert_movement(&mut ctx, 2..0, 1..2, CursorMovement::WordsBackward(2));
        assert_movement(&mut ctx, 2..2, 2..0, CursorMovement::WordsBackward(1));
        assert_movement(&mut ctx, 2..2, 1..3, CursorMovement::WordsBackward(2));
        assert_movement(&mut ctx, 2..2, 1..2, CursorMovement::WordsBackward(3));
        assert_movement(&mut ctx, 2..2, 1..0, CursorMovement::WordsBackward(4));
        assert_movement(&mut ctx, 2..2, 0..2, CursorMovement::WordsBackward(5));
        assert_movement(&mut ctx, 2..2, 0..0, CursorMovement::WordsBackward(6));
        assert_movement(&mut ctx, 2..2, 0..0, CursorMovement::WordsBackward(999));

        let mut ctx = TestContext::with_buffer("123\n  abc def\nghi");
        assert_movement(&mut ctx, 1..0, 1..2, CursorMovement::WordsForward(1));
        assert_movement(&mut ctx, 1..9, 2..0, CursorMovement::WordsForward(1));
        assert_movement(&mut ctx, 1..2, 1..0, CursorMovement::WordsBackward(1));
        assert_movement(&mut ctx, 2..0, 1..9, CursorMovement::WordsBackward(1));
    }
}

