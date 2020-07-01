#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8);

const fn color_hex(hex: u32) -> Color {
    Color(
        ((hex >> 16) & 0xff) as _,
        ((hex >> 8) & 0xff) as _,
        (hex & 0xff) as _,
    )
}

pub const PICO8_COLORS: [Color; 16] = [
    color_hex(0x291814), // cocoa
    color_hex(0x111d35), // midnight
    color_hex(0x422136), // port
    color_hex(0x125359), // sea
    color_hex(0x742f29), // leather
    color_hex(0x49333b), // charcoal
    color_hex(0xa28879), // olive
    color_hex(0xf3ef7d), // sand
    color_hex(0xbe1250), // crimson
    color_hex(0xff6c24), // amber
    color_hex(0xa8e72e), // tea
    color_hex(0x00b543), // jade
    color_hex(0x065ab5), // denim
    color_hex(0x754665), // aubergine
    color_hex(0xff6e59), // salmon
    color_hex(0xff9d81), // coral
];

pub struct Theme {
    pub cursor: Color,

    pub text_background: Color,
    pub text_foreground: Color,

    pub toolbar_active_background: Color,
    pub toolbar_active_foreground: Color,
    pub toolbar_inactive_background: Color,
    pub toolbar_inactive_foreground: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            cursor: PICO8_COLORS[15],

            text_background: PICO8_COLORS[0],
            text_foreground: PICO8_COLORS[6],

            toolbar_active_background: PICO8_COLORS[5],
            toolbar_active_foreground: PICO8_COLORS[6],
            toolbar_inactive_background: PICO8_COLORS[2],
            toolbar_inactive_foreground: PICO8_COLORS[13],
        }
    }
}
