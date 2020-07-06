use crate::{
    keymap::KeyMapCollection,
    buffer::BufferCollection,
    buffer_view::BufferViewCollection,
    command::CommandCollection,
    config::Config,
    event::{Event, Key},
    mode::{Mode, ModeContext, ModeOperation},
    theme::Theme,
    viewport::ViewportCollection,
};

pub enum EditorPollResult {
    Pending,
    Quit,
    Error(String),
}

pub struct Editor {
    pub config: Config,
    pub theme: Theme,

    pub mode: Mode,
    pub keymaps: KeyMapCollection,
    pub buffered_keys: Vec<Key>,
    pub input: String,
    pub commands: CommandCollection,

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
            keymaps: KeyMapCollection::default(),
            buffered_keys: Vec::new(),
            input: String::new(),
            commands: CommandCollection::default(),

            buffers: Default::default(),
            buffer_views: BufferViewCollection::default(),
            viewports: ViewportCollection::new(),
        }
    }

    pub fn on_event(&mut self, event: Event) -> EditorPollResult {
        match event {
            Event::None => (),
            Event::Resize(w, h) => {
                self.viewports.set_view_size(w as _, h as _);
            }
            Event::Key(key) => {
                self.buffered_keys.push(key);

                let (mode, mode_context) = self.get_mode_and_context();
                match mode.on_event(mode_context) {
                    ModeOperation::None => (),
                    ModeOperation::Pending => return EditorPollResult::Pending,
                    ModeOperation::Quit => return EditorPollResult::Quit,
                    ModeOperation::EnterMode(next_mode) => {
                        self.mode = next_mode;
                        let (mode, mode_context) = self.get_mode_and_context();
                        mode.on_enter(mode_context);
                    }
                    ModeOperation::Error(error) => {
                        self.buffered_keys.clear();

                        self.mode = Mode::Normal;
                        let (mode, mode_context) = self.get_mode_and_context();
                        mode.on_enter(mode_context);

                        return EditorPollResult::Error(error);
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

        EditorPollResult::Pending
    }

    fn get_mode_and_context<'a>(&'a mut self) -> (&'a mut Mode, ModeContext<'a>) {
        (
            &mut self.mode,
            ModeContext {
                commands: &self.commands,
                buffers: &mut self.buffers,
                buffer_views: &mut self.buffer_views,
                viewports: &mut self.viewports,
                keys: &self.buffered_keys[..],
                input: &mut self.input,
            },
        )
    }
}
