#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8);

const fn color_hex(hex: u32) -> Color {
    Color(
        ((hex >> 16) & 0xff) as _,
        ((hex >> 8) & 0xff) as _,
        (hex & 0xff) as _,
    )
}

pub const PICO8_COLORS: &[Color] = &[
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

                         /*
                         color_hex(0x291814), //  0 cocoa
                         color_hex(0x111d35), //  1 midnight
                         color_hex(0x422136), //  2 port
                         color_hex(0x125359), //  3 sea
                         color_hex(0x742f29), //  4 leather
                         color_hex(0x49333b), //  5 charcoal
                         color_hex(0xa28879), //  6 olive
                         color_hex(0xf3ef7d), //  7 sand
                         color_hex(0xbe1250), //  8 crimson
                         color_hex(0xff6c24), //  9 amber
                         color_hex(0xa8e72e), // 10 tea
                         color_hex(0x00b543), // 11 jade
                         color_hex(0x065ab5), // 12 denim
                         color_hex(0x754665), // 13 aubergine
                         color_hex(0xff6e59), // 14 salmon
                         color_hex(0xff9d81), // 15 coral
                         */
];

pub struct Theme {
    pub background: Color,

    pub cursor_normal: Color,
    pub cursor_select: Color,
    pub cursor_insert: Color,

    pub text_normal: Color,

    pub toolbar_background: Color,
    pub toolbar_foreground: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: PICO8_COLORS[0],

            cursor_normal: PICO8_COLORS[8],
            cursor_select: PICO8_COLORS[12],
            cursor_insert: PICO8_COLORS[11],

            text_normal: PICO8_COLORS[15],

            toolbar_background: PICO8_COLORS[8],
            toolbar_foreground: PICO8_COLORS[6],
        }
    }
}
