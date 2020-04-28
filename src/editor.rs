use crate::{
    buffer::BufferCollection,
    buffer_view::BufferView,
    config::Config,
    theme::{Color, Theme},
};

pub struct Editor {
    pub config: Config,
    pub theme: Theme,

    pub buffers: BufferCollection,
    pub buffer_views: Vec<BufferView>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            config: Default::default(),
            buffers: Default::default(),
            buffer_views: Default::default(),
            theme: Theme {
                foreground: Color(255, 255, 255),
                background: Color(0, 0, 0),
            },
        }
    }
}

impl Editor {
    pub fn set_view_size(&mut self, size: (u16, u16)) {
        for view in &mut self.buffer_views {
            view.size = size;
        }
    }
}
