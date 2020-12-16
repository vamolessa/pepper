use std::ops::Range;

use crate::{buffer::BufferHandle, buffer_position::BufferRange};

pub struct EditorEventText {
    texts_range: Range<usize>,
}
impl EditorEventText {
    pub fn as_str<'a>(&self, iter: EditorEventsIter<'a>) -> &'a str {
        &iter.0.texts[self.texts_range.clone()]
    }
}

pub enum EditorEvent {
    BufferLoad {
        handle: BufferHandle,
    },
    BufferOpen {
        handle: BufferHandle,
    },
    BufferInsertText {
        handle: BufferHandle,
        range: BufferRange,
        text: EditorEventText,
    },
    BufferDeleteText {
        handle: BufferHandle,
        range: BufferRange,
    },
    BufferSave {
        handle: BufferHandle,
        new_path: bool,
    },
    BufferClose {
        handle: BufferHandle,
    },
}

#[derive(Default)]
pub struct EditorEventQueue {
    events: Vec<EditorEvent>,
    texts: String,
}

impl EditorEventQueue {
    pub fn enqueue(&mut self, event: EditorEvent) {
        self.events.push(event);
    }

    pub fn enqueue_buffer_insert(&mut self, handle: BufferHandle, range: BufferRange, text: &str) {
        let start = self.texts.len();
        self.texts.push_str(text);
        let text = EditorEventText {
            texts_range: start..self.texts.len(),
        };
        self.events.push(EditorEvent::BufferInsertText {
            handle,
            range,
            text,
        });
    }
}

#[derive(Clone, Copy)]
pub struct EditorEventsIter<'a>(&'a EditorEventQueue);
impl<'a> IntoIterator for EditorEventsIter<'a> {
    type Item = &'a EditorEvent;
    type IntoIter = std::slice::Iter<'a, EditorEvent>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.events.iter()
    }
}

#[derive(Default)]
pub struct EditorEventDoubleQueue {
    read: EditorEventQueue,
    write: EditorEventQueue,
}

impl EditorEventDoubleQueue {
    pub fn flip(&mut self) {
        self.read.events.clear();
        std::mem::swap(&mut self.read, &mut self.write);
    }

    pub fn get_stream_and_sink(&mut self) -> (EditorEventsIter, &mut EditorEventQueue) {
        (EditorEventsIter(&self.read), &mut self.write)
    }
}
