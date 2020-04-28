#[derive(Debug, Default, Clone, Copy)]
pub struct Color(pub u8, pub u8, pub u8);

pub struct Theme {
    pub foreground: Color,
    pub background: Color,
}
