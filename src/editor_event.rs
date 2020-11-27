use crate::buffer::BufferHandle;

pub enum EditorEvent {
    BufferLoad {
        handle: BufferHandle,
    },
    BufferOpen {
        handle: BufferHandle,
    },
    BufferEdit {
        handle: BufferHandle,
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
}

impl EditorEventQueue {
    pub fn enqueue(&mut self, event: EditorEvent) {
        self.events.push(event);
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
