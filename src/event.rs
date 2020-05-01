#[derive(Debug, Clone, Copy)]
pub enum Event {
    None,
    Key(Key),
    Resize(u16, u16),
}

#[derive(Debug, Clone, Copy)]
pub enum Key {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Insert,
    F(u8),
    Char(char),
    Ctrl(char),
    Alt(char),
    Null,
    Esc,
}

