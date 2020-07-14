use std::path::Path;

use crate::{
    buffer::{TextRef,BufferCollection},
    cursor::Cursor,
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{BufferViewCollection, BufferViewHandle},
    command::CommandCollection,
    config::Config,
    event::Key,
    keymap::{KeyMapCollection, MatchResult},
    mode::{Mode, ModeContext, ModeOperation},
    theme::Theme,
};

pub enum EditorOperation<'a> {
    Content(&'a str),
    Path(Option<&'a Path>),
    Mode(Mode),
    Insert(BufferPosition, TextRef<'a>),
    Delete(BufferRange),
    ClearCursors,
    Cursor(Cursor),
    Search(&'a str),
}

pub struct EditorOperationSink<'a> {
    operations: Vec<EditorOperation<'a>>,
}

impl<'a> EditorOperationSink<'a> {
    pub fn send(&mut self, operation: EditorOperation<'a>) {
        self.operations.push(operation);
    }

    pub fn drain(&mut self) -> impl 'a + Iterator<Item = EditorOperation<'a>> {
        self.operations.drain(..)
    }
}

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

    mode: Mode,
    pub keymaps: KeyMapCollection,
    buffered_keys: Vec<Key>,
    input: String,
    pub commands: CommandCollection,

    pub buffers: BufferCollection,
    pub buffer_views: BufferViewCollection,
    local_client_current_buffer_view_handle: Option<BufferViewHandle>,
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
            local_client_current_buffer_view_handle: None,
        }
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn input(&self) -> &str {
        &self.input[..]
    }

    pub fn on_key(&mut self, key: Key) -> EditorPollResult {
        self.buffered_keys.push(key);

        match self
            .keymaps
            .matches(self.mode.discriminant(), &self.buffered_keys[..])
        {
            MatchResult::None => (),
            MatchResult::Prefix => return EditorPollResult::Pending,
            MatchResult::ReplaceWith(replaced_keys) => {
                self.buffered_keys.clear();
                self.buffered_keys.extend_from_slice(replaced_keys);
            }
        }

        let mut keys = KeysIterator::new(&self.buffered_keys);
        let result = loop {
            if keys.index >= self.buffered_keys.len() {
                break EditorPollResult::Pending;
            }

            let mut mode_context = ModeContext {
                commands: &self.commands,
                buffers: &mut self.buffers,
                buffer_views: &mut self.buffer_views,
                current_buffer_view_handle: &mut self.local_client_current_buffer_view_handle,
                input: &mut self.input,
            };

            match self.mode.on_event(&mut mode_context, &mut keys) {
                ModeOperation::Pending => return EditorPollResult::Pending,
                ModeOperation::None => (),
                ModeOperation::Quit => return EditorPollResult::Quit,
                ModeOperation::EnterMode(next_mode) => {
                    self.mode = next_mode;
                    self.mode.on_enter(&mut mode_context);
                }
                ModeOperation::Error(error) => {
                    self.mode = Mode::default();
                    self.mode.on_enter(&mut mode_context);

                    break EditorPollResult::Error(error);
                }
            }
        };

        self.buffered_keys.clear();
        result
    }
}
