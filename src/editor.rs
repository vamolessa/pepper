use crate::{
    buffer::BufferCollection,
    buffer_view::BufferView,
    config::Config,
    event::{Event, Key},
    mode::{initial_mode, Mode, Transition},
    theme::Theme,
};

pub struct Editor {
    pub config: Config,
    pub theme: Theme,

    pub mode: Box<dyn Mode>,
    pub buffered_keys: Vec<Key>,

    pub buffers: BufferCollection,
    pub buffer_views: Vec<BufferView>,
    pub current_buffer_view: usize,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            config: Default::default(),
            theme: Theme::default(),
            mode: initial_mode(),
            buffered_keys: Vec::new(),
            buffers: Default::default(),
            buffer_views: Default::default(),
            current_buffer_view: 0,
        }
    }
}

impl Editor {
    pub fn set_view_size(&mut self, size: (usize, usize)) {
        for view in &mut self.buffer_views {
            view.size = size;
        }
    }

    pub fn on_event(&mut self, event: Event) -> bool {
        let buffer_view = &mut self.buffer_views[self.current_buffer_view];
        let buffers = &mut self.buffers;
        match event {
            Event::None => (),
            Event::Resize(_w, _h) => (),
            Event::Key(key) => {
                self.buffered_keys.push(key);
                match self
                    .mode
                    .on_event(buffer_view, buffers, &self.buffered_keys[..])
                {
                    Transition::None => self.buffered_keys.clear(),
                    Transition::Waiting => (),
                    Transition::Exit => return false,
                    Transition::EnterMode(mode) => {
                        self.buffered_keys.clear();
                        self.mode = mode;
                    }
                }
            }
        }

        true
    }
}
