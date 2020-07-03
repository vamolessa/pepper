use crate::{
    buffer::{Buffer, BufferCollection, BufferContent},
    buffer_view::{BufferView, BufferViewCollection},
    config::Config,
    event::{Event, Key},
    mode::{Mode, ModeContext, Operation},
    theme::Theme,
    viewport::ViewportCollection,
};

pub struct Editor {
    pub config: Config,
    pub theme: Theme,

    pub mode: Mode,
    pub buffered_keys: Vec<Key>,
    pub input: String,

    pub buffers: BufferCollection,
    pub buffer_views: BufferViewCollection,
    pub viewports: ViewportCollection,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            config: Default::default(),
            theme: Theme::default(),
            mode: Mode::default(),
            buffered_keys: Vec::new(),
            input: String::new(),
            buffers: Default::default(),
            buffer_views: BufferViewCollection::default(),
            viewports: ViewportCollection::new(),
        }
    }

    pub fn new_buffer_from_content(&mut self, content: BufferContent) {
        let buffer_handle = self.buffers.add(Buffer::with_content(content));
        let buffer_view_index = self
            .buffer_views
            .add(BufferView::with_handle(buffer_handle));
        self.viewports
            .current_viewport_mut()
            .set_current_buffer_view_handle(buffer_view_index);
    }

    pub fn on_event(&mut self, event: Event) -> bool {
        match event {
            Event::None => (),
            Event::Resize(w, h) => {
                self.viewports.set_view_size(w as _, h as _);
            }
            Event::Key(key) => {
                self.buffered_keys.push(key);

                match self.mode.on_event(ModeContext {
                    buffers: &mut self.buffers,
                    buffer_views: &mut self.buffer_views,
                    current_buffer_view_handle: self
                        .viewports
                        .current_viewport()
                        .current_buffer_view_handle(),
                    keys: &self.buffered_keys[..],
                    input: &mut self.input,
                }) {
                    Operation::None => (),
                    Operation::Waiting => return true,
                    Operation::Exit => return false,
                    Operation::EnterMode(mode) => {
                        self.mode.on_leave(ModeContext {
                            buffers: &mut self.buffers,
                            buffer_views: &mut self.buffer_views,
                            current_buffer_view_handle: self
                                .viewports
                                .current_viewport()
                                .current_buffer_view_handle(),
                            keys: &self.buffered_keys[..],
                            input: &mut self.input,
                        });
                        self.mode = mode;
                        self.mode.on_enter(ModeContext {
                            buffers: &mut self.buffers,
                            buffer_views: &mut self.buffer_views,
                            current_buffer_view_handle: self
                                .viewports
                                .current_viewport()
                                .current_buffer_view_handle(),
                            keys: &self.buffered_keys[..],
                            input: &mut self.input,
                        });
                    }
                    Operation::NextViewport => {
                        self.viewports.focus_next_viewport(&mut self.buffer_views)
                    }
                }

                if let Some(handle) = self
                    .viewports
                    .current_viewport()
                    .current_buffer_view_handle()
                {
                    let buffer_view = &self.buffer_views.get(handle);
                    self.viewports
                        .current_viewport_mut()
                        .scroll_to_cursor(buffer_view.cursors.main_cursor().position);
                }

                self.buffered_keys.clear();
            }
        }

        true
    }
}
