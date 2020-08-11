use std::{fmt, str::FromStr};

use serde_derive::{Deserialize, Serialize};

pub enum ParseThemeError {
    ColorNotFound,
    BadColorFormat,
    ColorHexTooBig,
    ParseColorError(Box<dyn fmt::Display>),
}

impl fmt::Display for ParseThemeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ColorNotFound => write!(f, "could not find color"),
            Self::BadColorFormat => write!(f, "colors should start with '#'"),
            Self::ColorHexTooBig => write!(f, "color hex is too big"),
            Self::ParseColorError(e) => write!(f, "{}", e),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color(pub u8, pub u8, pub u8);

impl Color {
    pub const fn from_hex(hex: u32) -> Color {
        Color(
            ((hex >> 16) & 0xff) as _,
            ((hex >> 8) & 0xff) as _,
            (hex & 0xff) as _,
        )
    }
}

impl FromStr for Color {
    type Err = ParseThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let color = s.trim_start_matches('#');
        if color.len() == s.len() {
            return Err(ParseThemeError::BadColorFormat);
        }

        match u32::from_str_radix(color, 16) {
            Ok(hex) => {
                if hex <= 0xFFFFFF {
                    Ok(Color::from_hex(hex))
                } else {
                    Err(ParseThemeError::ColorHexTooBig)
                }
            }
            Err(e) => Err(ParseThemeError::ParseColorError(Box::new(e))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub background: Color,
    pub highlight: Color,

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
}

impl Theme {
    pub fn parse_and_set(&mut self, name: &str, color: &str) -> Result<(), ParseThemeError> {
        macro_rules! match_and_parse {
            ($($name:ident,)*) => {
                match name {
                    $(stringify!($name) => self.$name = color.parse()?,)*
                    _ => return Err(ParseThemeError::ColorNotFound),
                }
            }
        }

        match_and_parse! {
            background, highlight,
            cursor_normal, cursor_select, cursor_insert,
            token_whitespace, token_text, token_comment, token_keyword,
            token_type, token_symbol, token_string, token_literal,
        }

        Ok(())
    }
}

pub fn pico8_theme() -> Theme {
    const COLORS: &[Color] = &[
        Color::from_hex(0x000000), //  0 black
        Color::from_hex(0x1d2b53), //  1 storm
        Color::from_hex(0x7e2553), //  2 wine
        Color::from_hex(0x008751), //  3 moss
        Color::from_hex(0xab5236), //  4 tan
        Color::from_hex(0x5f574f), //  5 slate
        Color::from_hex(0xc2c3c7), //  6 silver
        Color::from_hex(0xfff1e8), //  7 white
        Color::from_hex(0xff004d), //  8 ember
        Color::from_hex(0xffa300), //  9 orange
        Color::from_hex(0xffec27), // 10 lemon
        Color::from_hex(0x00e436), // 11 lime
        Color::from_hex(0x29adff), // 12 sky
        Color::from_hex(0x83769c), // 13 dusk
        Color::from_hex(0xff77a8), // 14 pink
        Color::from_hex(0xffccaa), // 15 peach
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
