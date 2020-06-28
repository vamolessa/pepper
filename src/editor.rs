use crate::{
    buffer::{Buffer, BufferCollection, BufferContent},
    buffer_view::{BufferView, BufferViewCollection},
    config::Config,
    event::{Event, Key},
    mode::{initial_mode, Mode, Operation},
    theme::Theme,
    viewport::ViewportCollection,
};

pub struct Editor {
    pub config: Config,
    pub theme: Theme,

    pub mode: Box<dyn Mode>,
    pub buffered_keys: Vec<Key>,

    pub buffers: BufferCollection,
    pub buffer_views: BufferViewCollection,
    pub viewports: ViewportCollection,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            config: Default::default(),
            theme: Theme::default(),
            mode: initial_mode(),
            buffered_keys: Vec::new(),
            buffers: Default::default(),
            buffer_views: BufferViewCollection::default(),
            viewports: ViewportCollection::new(),
        }
    }
}

impl Editor {
    pub fn new_buffer_from_content(&mut self, content: BufferContent) {
        let buffer_handle = self.buffers.add(Buffer::with_content(content));
        self.viewports
            .current_viewport_mut()
            .set_buffer_view(Some(self.buffer_views.len()));
        self.buffer_views
            .push(BufferView::with_handle(buffer_handle));
    }

    pub fn on_event(&mut self, event: Event) -> bool {
        match event {
            Event::None => (),
            Event::Resize(_w, _h) => (),
            Event::Key(key) => {
                self.buffered_keys.push(key);

                match self.mode.on_event(
                    &mut self.buffers,
                    &mut self.buffer_views,
                    self.viewports.current_viewport().buffer_view_index(),
                    &self.buffered_keys[..],
                ) {
                    Operation::None => self.buffered_keys.clear(),
                    Operation::Waiting => (),
                    Operation::Exit => return false,
                    Operation::EnterMode(mode) => {
                        self.buffered_keys.clear();
                        self.mode = mode;
                    }
                    Operation::ViewportOperation(op) => self.viewports.handle_operation(op),
                }
                if let Some(index) = self.viewports.current_viewport().buffer_view_index() {
                    let buffer_view = &self.buffer_views[index];
                    self.viewports
                        .current_viewport_mut()
                        .scroll_to_cursor(buffer_view.cursors.main_cursor().position);
                }
            }
        }

        true
    }
}
