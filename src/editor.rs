use crate::{
    buffer::BufferCollection,
    buffer_view::BufferView,
    config::Config,
    event::Event,
    mode::{Mode, Normal, Transition},
    theme::{Color, Theme},
};

pub struct Editor {
    pub config: Config,
    pub theme: Theme,
    pub mode: Box<dyn Mode>,

    pub buffers: BufferCollection,
    pub buffer_views: Vec<BufferView>,
    pub current_buffer_view: usize,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            config: Default::default(),
            theme: Theme {
                foreground: Color(255, 255, 255),
                background: Color(0, 0, 0),
            },
            mode: Box::new(Normal {}),
            buffers: Default::default(),
            buffer_views: Default::default(),
            current_buffer_view: 0,
        }
    }
}

impl Editor {
    pub fn set_view_size(&mut self, size: (u16, u16)) {
        for view in &mut self.buffer_views {
            view.size = size;
        }
    }

    pub fn on_event(&mut self, event: &Event) -> bool {
        let buffer_view = &mut self.buffer_views[self.current_buffer_view];
        let buffers = &mut self.buffers;
        match self.mode.on_event(buffer_view, buffers, event) {
            Transition::None => (),
            Transition::MoveToMode(mode) => self.mode = mode,
            Transition::Exit => return true,
        }

        false
    }
}
