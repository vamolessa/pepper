use std::path::PathBuf;

use serde_derive::{Deserialize, Serialize};

use crate::{
    buffer::{BufferCollection, BufferContent, Text},
    buffer_position::{BufferPosition, BufferRange},
    buffer_view::{BufferViewCollection, BufferViewHandle},
    command::CommandCollection,
    config::Config,
    connection::TargetClient,
    cursor::{Cursor, CursorCollection},
    event::Key,
    keymap::{KeyMapCollection, MatchResult},
    mode::{Mode, ModeContext, ModeOperation},
    theme::Theme,
};

pub enum EditorLoop {
    Quit,
    Continue,
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EditorOperation {
    Focused(bool),
    Content,
    Path(Option<PathBuf>),
    Mode(Mode),
    Insert(BufferPosition, Text),
    Delete(BufferRange),
    ClearCursors(Cursor),
    Cursor(Cursor),
    InputAppend(char),
    InputKeep(usize),
    Search,
}

pub struct EditorOperationSender {
    operations: Vec<(TargetClient, EditorOperation)>,
    write_content_buf: Vec<u8>,
}

impl EditorOperationSender {
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            write_content_buf: Vec::with_capacity(1024 * 8),
        }
    }

    pub fn send(&mut self, target_client: TargetClient, operation: EditorOperation) {
        self.operations.push((target_client, operation));
    }

    pub fn send_cursors(&mut self, target_client: TargetClient, cursors: &CursorCollection) {
        self.send(
            target_client,
            EditorOperation::ClearCursors(*cursors.main_cursor()),
        );
        for cursor in &cursors[..] {
            self.send(target_client, EditorOperation::Cursor(*cursor));
        }
    }

    pub fn send_empty_content(&mut self, target_client: TargetClient) {
        self.send(target_client, EditorOperation::Content);
        self.write_content_buf.clear();
    }

    pub fn send_content(&mut self, target_client: TargetClient, content: &BufferContent) {
        self.send_empty_content(target_client);
        if content.write(&mut self.write_content_buf).is_err() {
            self.write_content_buf.clear();
        }
    }

    pub fn drain(&mut self) -> impl '_ + Iterator<Item = (TargetClient, EditorOperation, &str)> {
        let content = std::str::from_utf8(&self.write_content_buf[..]).unwrap_or("");
        self.operations.drain(..).map(move |(t, o)| (t, o, content))
    }
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

    focused_client: TargetClient,
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

            focused_client: TargetClient::All,
        }
    }

    pub fn on_key(
        &mut self,
        key: Key,
        target_client: TargetClient,
        operations: &mut EditorOperationSender,
    ) -> EditorLoop {
        if target_client != self.focused_client {
            operations.send(self.focused_client, EditorOperation::Focused(false));
            operations.send(target_client, EditorOperation::Focused(true));

            self.focused_client = target_client;
            self.buffered_keys.clear();
        }

        self.buffered_keys.push(key);

        match self
            .keymaps
            .matches(self.mode.discriminant(), &self.buffered_keys[..])
        {
            MatchResult::None => (),
            MatchResult::Prefix => return EditorLoop::Continue,
            MatchResult::ReplaceWith(replaced_keys) => {
                self.buffered_keys.clear();
                self.buffered_keys.extend_from_slice(replaced_keys);
            }
        }

        let mut keys = KeysIterator::new(&self.buffered_keys);
        let result = loop {
            if keys.index >= self.buffered_keys.len() {
                break EditorLoop::Continue;
            }

            let mut mode_context = ModeContext {
                target_client,
                operations,

                commands: &self.commands,
                buffers: &mut self.buffers,
                buffer_views: &mut self.buffer_views,
                current_buffer_view_handle: &mut self.local_client_current_buffer_view_handle,
                input: &mut self.input,
            };

            match self.mode.on_event(&mut mode_context, &mut keys) {
                ModeOperation::Pending => return EditorLoop::Continue,
                ModeOperation::None => (),
                ModeOperation::Quit => return EditorLoop::Quit,
                ModeOperation::EnterMode(next_mode) => {
                    self.mode = next_mode.clone();
                    self.mode.on_enter(&mut mode_context);
                    operations.send(TargetClient::All, EditorOperation::Mode(next_mode));
                }
                ModeOperation::Error(error) => {
                    self.mode = Mode::default();
                    self.mode.on_enter(&mut mode_context);
                    operations.send(TargetClient::All, EditorOperation::Mode(Mode::default()));

                    break EditorLoop::Error(error);
                }
            }
        };

        self.buffered_keys.clear();
        result
    }
}
