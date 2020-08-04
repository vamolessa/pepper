#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8);

pub struct Theme {
    pub background: Color,

    pub cursor_normal: Color,
    pub cursor_select: Color,
    pub cursor_insert: Color,

    pub token_whitespace: Color,
    pub token_text: Color,
    pub token_comment: Color,
    pub token_keyword: Color,
    pub token_type: Color,
    pub token_symbol: Color,
    pub token_string: Color,
    pub token_literal: Color,

    pub highlight: Color,
}

pub fn pico8_theme() -> Theme {
    const fn color_hex(hex: u32) -> Color {
        Color(
            ((hex >> 16) & 0xff) as _,
            ((hex >> 8) & 0xff) as _,
            (hex & 0xff) as _,
        )
    }

    const COLORS: &[Color] = &[
        color_hex(0x000000), //  0 black
        color_hex(0x1d2b53), //  1 storm
        color_hex(0x7e2553), //  2 wine
        color_hex(0x008751), //  3 moss
        color_hex(0xab5236), //  4 tan
        color_hex(0x5f574f), //  5 slate
        color_hex(0xc2c3c7), //  6 silver
        color_hex(0xfff1e8), //  7 white
        color_hex(0xff004d), //  8 ember
        color_hex(0xffa300), //  9 orange
        color_hex(0xffec27), // 10 lemon
        color_hex(0x00e436), // 11 lime
        color_hex(0x29adff), // 12 sky
        color_hex(0x83769c), // 13 dusk
        color_hex(0xff77a8), // 14 pink
        color_hex(0xffccaa), // 15 peach
    ];

    Theme {
        background: COLORS[0],

        cursor_normal: COLORS[8],
        cursor_select: COLORS[12],
        cursor_insert: COLORS[11],

        token_whitespace: COLORS[2],
        token_text: COLORS[15],
        token_comment: COLORS[13],
        token_keyword: COLORS[9],
        token_type: COLORS[7],
        token_symbol: COLORS[6],
        token_string: COLORS[10],
        token_literal: COLORS[14],

        highlight: COLORS[10],
    }
}
