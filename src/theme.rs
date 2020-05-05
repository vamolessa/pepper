#[derive(Debug, Default, Clone, Copy)]
pub struct Color(pub u8, pub u8, pub u8);

pub struct Theme {
    pub background: Color,
    pub foreground: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: Color(0, 0, 0),
            foreground: Color(255, 255, 255),
        }
    }
}
