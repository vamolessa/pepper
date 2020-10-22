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

#[derive(Debug, Clone)]
pub struct Theme {
    pub background: Color,
    pub highlight: Color,
    pub cursor: Color,

    pub token_whitespace: Color,
    pub token_text: Color,
    pub token_comment: Color,
    pub token_keyword: Color,
    pub token_type: Color,
    pub token_symbol: Color,
    pub token_string: Color,
    pub token_literal: Color,
}

impl Theme {
    pub fn color_from_name(&mut self, name: &str) -> Option<&mut Color> {
        macro_rules! match_and_get_property {
            ($($prop:ident,)*) => {
                match name {
                    $(stringify!($prop) => Some(&mut self.$prop),)*
                    _ => None
                }
            }
        }

        match_and_get_property! {
            background,
            highlight,
            cursor,

            token_whitespace,
            token_text,
            token_comment,
            token_keyword,
            token_type,
            token_symbol,
            token_string,
            token_literal,
        }
    }
}

pub fn pico8_theme() -> Theme {
    const COLORS: &[Color] = &[
        Color::from_u32(0x000000), //  0 black
        Color::from_u32(0x1d2b53), //  1 storm
        Color::from_u32(0x7e2553), //  2 wine
        Color::from_u32(0x008751), //  3 moss
        Color::from_u32(0xab5236), //  4 tan
        Color::from_u32(0x5f574f), //  5 slate
        Color::from_u32(0xc2c3c7), //  6 silver
        Color::from_u32(0xfff1e8), //  7 white
        Color::from_u32(0xff004d), //  8 ember
        Color::from_u32(0xffa300), //  9 orange
        Color::from_u32(0xffec27), // 10 lemon
        Color::from_u32(0x00e436), // 11 lime
        Color::from_u32(0x29adff), // 12 sky
        Color::from_u32(0x83769c), // 13 dusk
        Color::from_u32(0xff77a8), // 14 pink
        Color::from_u32(0xffccaa), // 15 peach
    ];

    Theme {
        background: COLORS[0],
        highlight: COLORS[10],
        cursor: COLORS[8],

        token_whitespace: COLORS[2],
        token_text: COLORS[15],
        token_comment: COLORS[13],
        token_keyword: COLORS[9],
        token_type: COLORS[7],
        token_symbol: COLORS[6],
        token_string: COLORS[11],
        token_literal: COLORS[14],
    }
}
