pub enum Event {
    None,
    Key(KeyEvent),
    Resize(u16, u16),
}

pub enum KeyEvent {
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

