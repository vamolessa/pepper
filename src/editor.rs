use crate::{
    buffer::BufferCollection,
    config::Config,
    event::{Event, Key},
    mode::{initial_mode, Mode, Transition},
    theme::Theme,
    viewport::ViewportCollection,
};

pub struct Editor {
    pub config: Config,
    pub theme: Theme,

    pub mode: Box<dyn Mode>,
    pub buffered_keys: Vec<Key>,

    pub buffers: BufferCollection,
    pub viewports: ViewportCollection,
    pub current_viewport: usize,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            config: Default::default(),
            theme: Theme::default(),
            mode: initial_mode(),
            buffered_keys: Vec::new(),
            buffers: Default::default(),
            viewports: ViewportCollection::new(),
            current_viewport: 0,
        }
    }
}

impl Editor {
    pub fn on_event(&mut self, event: Event) -> bool {
        match event {
            Event::None => (),
            Event::Resize(_w, _h) => (),
            Event::Key(key) => {
                self.buffered_keys.push(key);

                let buffers = &mut self.buffers;
                let viewport = self.viewports.get_singleton_viewport_mut();
                let buffer_view = &mut viewport.current_buffer_view_mut();
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
