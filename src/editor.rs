use crate::{
    buffer::BufferCollection,
    buffer_view::BufferViewCollection,
    command::CommandCollection,
    config::Config,
    event::{Event, Key},
    keymap::{KeyMapCollection, MatchResult},
    mode::{Mode, ModeContext, ModeOperation},
    theme::Theme,
    viewport::ViewportCollection,
};

pub enum EditorPollResult {
    Pending,
    Quit,
    Error(String),
}

pub struct KeysIterator<'a> {
    keys: &'a [Key],
    index: usize,
}

impl<'a> KeysIterator<'a> {
    fn new(keys: &'a [Key]) -> Self {
        Self { keys, index: 0 }
    }

    pub fn next(&mut self) -> Key {
        if self.index < self.keys.len() {
            let next = self.keys[self.index];
            self.index += 1;
            next
        } else {
            Key::None
        }
    }

    pub fn put_back(&mut self) {
        if self.index > 0 {
            self.index -= 1;
        }
    }
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
            Event::None => EditorPollResult::Pending,
            Event::Resize(w, h) => {
                self.viewports.set_view_size(w as _, h as _);
                EditorPollResult::Pending
            }
            Event::Key(key) => {
                self.buffered_keys.push(key);

                match self
                    .keymaps
                    .matches(self.mode.discriminant(), &self.buffered_keys[..])
                {
                    MatchResult::None => (),
                    MatchResult::Prefix => return EditorPollResult::Pending,
                    MatchResult::Replace(replaced_keys) => {
                        self.buffered_keys.clear();
                        self.buffered_keys.extend_from_slice(replaced_keys);
                    }
                }

                let mut keys = KeysIterator::new(&self.buffered_keys);
                let result = loop {
                    let mut mode_context = ModeContext {
                        commands: &self.commands,
                        buffers: &mut self.buffers,
                        buffer_views: &mut self.buffer_views,
                        viewports: &mut self.viewports,
                        input: &mut self.input,
                    };

                    match self.mode.on_event(&mut mode_context, &mut keys) {
                        ModeOperation::NoMatch => {
                            self.buffered_keys.clear();
                            break EditorPollResult::Pending;
                        }
                        ModeOperation::Pending => break EditorPollResult::Pending,
                        ModeOperation::None => (),
                        ModeOperation::Quit => {
                            self.buffered_keys.clear();
                            break EditorPollResult::Quit;
                        }
                        ModeOperation::EnterMode(next_mode) => {
                            self.mode = next_mode;
                            self.mode.on_enter(&mut mode_context);
                        }
                        ModeOperation::Error(error) => {
                            self.mode = Mode::Normal;
                            self.mode.on_enter(&mut mode_context);

                            self.buffered_keys.clear();
                            break EditorPollResult::Error(error);
                        }
                    }
                };

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

                result
            }
        }
    }
}
