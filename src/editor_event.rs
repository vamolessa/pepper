// TODO: merge with client_event.rs??

use std::ops::Range;

use crate::{buffer::BufferHandle, buffer_position::BufferRange};

pub struct EditorEventText {
    texts_range: Range<usize>,
}
impl EditorEventText {
    pub fn as_str<'a>(&self, events: &'a EditorEventQueue) -> &'a str {
        &events.read.texts[self.texts_range.clone()]
    }
}

pub enum EditorEvent {
    Idle,
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
struct EventQueue {
    events: Vec<EditorEvent>,
    texts: String,
}

#[derive(Default)]
pub struct EditorEventQueue {
    read: EventQueue,
    write: EventQueue,
}

impl EditorEventQueue {
    pub fn flip(&mut self) {
        self.read.events.clear();
        self.read.texts.clear();
        std::mem::swap(&mut self.read, &mut self.write);
    }

    pub fn enqueue(&mut self, event: EditorEvent) {
        self.write.events.push(event);
    }

    pub fn enqueue_buffer_insert(&mut self, handle: BufferHandle, range: BufferRange, text: &str) {
        let start = self.write.texts.len();
        self.write.texts.push_str(text);
        let text = EditorEventText {
            texts_range: start..self.write.texts.len(),
        };
        self.write.events.push(EditorEvent::BufferInsertText {
            handle,
            range,
            text,
        });
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &'a EditorEvent> {
        self.read.events.iter()
    }
}
