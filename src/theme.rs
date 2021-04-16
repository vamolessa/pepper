#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8);

impl Color {
    pub const fn into_u32(self) -> u32 {
        let r = self.0 as u32;
        let g = self.1 as u32;
        let b = self.2 as u32;
        r << 16 | g << 8 | b
    }

    pub const fn from_u32(hex: u32) -> Color {
        Color(
            ((hex >> 16) & 0xff) as _,
            ((hex >> 8) & 0xff) as _,
            (hex & 0xff) as _,
        )
    }
}

macro_rules! theme_colors {
    ($($color:ident,)*) => {
        pub const THEME_COLOR_NAMES: &[&str] = &[$(stringify!($color),)*];

        pub struct Theme {
            $(pub $color: Color,)*
        }

        impl Theme {
            pub fn color_from_name(&mut self, name: &str) -> Option<&mut Color> {
                match name {
                    $(stringify!($color) => Some(&mut self.$color),)*
                    _ => None,
                }
            }
        }
    }
}

theme_colors! {
    background,
    highlight,
    statusbar_active_background,
    statusbar_inactive_background,
    normal_cursor,
    insert_cursor,
    select_cursor,

    token_whitespace,
    token_text,
    token_comment,
    token_keyword,
    token_type,
    token_symbol,
    token_string,
    token_literal,
}

impl Default for Theme {
    fn default() -> Self {
        gruvbox_theme()
    }
}

pub fn gruvbox_theme() -> Theme {
    Theme {
        background: Color::from_u32(0x1d2021),
        highlight: Color::from_u32(0xfabd2f),
        statusbar_active_background: Color::from_u32(0x504945),
        statusbar_inactive_background: Color::from_u32(0x282828),
        normal_cursor: Color::from_u32(0xcc241d),
        insert_cursor: Color::from_u32(0xfabd2f),
        select_cursor: Color::from_u32(0x458588),

        token_whitespace: Color::from_u32(0x504945),
        token_text: Color::from_u32(0xebdbb2),
        token_comment: Color::from_u32(0x7c6f64),
        token_keyword: Color::from_u32(0xfe8019),
        token_type: Color::from_u32(0x8ec07c),
        token_symbol: Color::from_u32(0xa89984),
        token_string: Color::from_u32(0xb8bb26),
        token_literal: Color::from_u32(0xd3869b),
    }
}
