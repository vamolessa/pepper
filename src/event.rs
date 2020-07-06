#[derive(Debug, Clone, Copy)]
pub enum Event {
    None,
    Key(Key),
    Resize(u16, u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    Delete,
    F(u8),
    Char(char),
    Ctrl(char),
    Alt(char),
    Esc,
}

#[derive(Debug)]
pub enum KeyParseError {
    UnexpectedEndOfText,
    InvalidCharacter(char),
}

impl Key {
    pub fn parse(text: &str, index: &mut usize) -> Result<Self, KeyParseError> {
        let text = &text[*index..];
        let mut chars = text.chars();

        macro_rules! next {
            () => {
                match chars.next() {
                    Some(element) => {
                        *index += 1;
                        element
                    }
                    None => return Err(KeyParseError::UnexpectedEndOfText),
                }
            };
        }

        macro_rules! consume {
            ($character:expr) => {
                let c = next!();
                if c != $character {
                    return Err(KeyParseError::InvalidCharacter(c));
                }
            };
        }

        macro_rules! consume_str {
            ($str:expr) => {
                for c in $str.chars() {
                    consume!(c);
                }
            };
        }

        let key = match next!() {
            '\\' => match next!() {
                '\\' => Key::Char('\\'),
                '<' => Key::Char('<'),
                c => return Err(KeyParseError::InvalidCharacter(c)),
            },
            '<' => match next!() {
                'b' => {
                    consume_str!("ackspace>");
                    Key::Backspace
                }
                's' => {
                    consume_str!("pace>");
                    Key::Char(' ')
                }
                'e' => match next!() {
                    'n' => match next!() {
                        't' => {
                            consume_str!("er>");
                            Key::Enter
                        }
                        'd' => {
                            consume!('>');
                            Key::End
                        }
                        c => return Err(KeyParseError::InvalidCharacter(c)),
                    },
                    's' => {
                        consume_str!("c>");
                        Key::Esc
                    }
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'l' => {
                    consume_str!("eft>");
                    Key::Left
                }
                'r' => {
                    consume_str!("ight>");
                    Key::Right
                }
                'u' => {
                    consume_str!("p>");
                    Key::Up
                }
                'd' => match next!() {
                    'o' => {
                        consume_str!("wn>");
                        Key::Down
                    }
                    'e' => {
                        consume_str!("lete>");
                        Key::Delete
                    }
                    c => return Err(KeyParseError::InvalidCharacter(c)),
                },
                'h' => {
                    consume_str!("ome>");
                    Key::Home
                }
                'p' => {
                    consume_str!("age");
                    match next!() {
                        'u' => {
                            consume_str!("p>");
                            Key::PageUp
                        }
                        'd' => {
                            consume_str!("own>");
                            Key::PageDown
                        }
                        c => return Err(KeyParseError::InvalidCharacter(c)),
                    }
                }
                't' => {
                    consume_str!("ab>");
                    Key::Tab
                }
                'f' => {
                    let n = match next!() {
                        '1' => match next!() {
                            '>' => 1,
                            '0' => {
                                consume!('>');
                                10
                            }
                            '1' => {
                                consume!('>');
                                11
                            }
                            '2' => {
                                consume!('>');
                                12
                            }
                            c => return Err(KeyParseError::InvalidCharacter(c)),
                        },
                        c => {
                            consume!('>');
                            match c.to_digit(10) {
                                Some(n) => n,
                                None => return Err(KeyParseError::InvalidCharacter(c)),
                            }
                        }
                    };
                    Key::F(n as _)
                }
                'c' => {
                    consume!('-');
                    let c = next!();
                    let key = if c.is_ascii_alphanumeric() {
                        Key::Ctrl(c)
                    } else {
                        return Err(KeyParseError::InvalidCharacter(c));
                    };
                    consume!('>');
                    key
                }
                'a' => {
                    consume!('-');
                    let c = next!();
                    let key = if c.is_ascii_alphanumeric() {
                        Key::Alt(c)
                    } else {
                        return Err(KeyParseError::InvalidCharacter(c));
                    };
                    consume!('>');
                    key
                }
                c => return Err(KeyParseError::InvalidCharacter(c)),
            },
            c => {
                if c.is_ascii() {
                    Key::Char(c)
                } else {
                    return Err(KeyParseError::InvalidCharacter(c));
                }
            }
        };

        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_key() {
        let mut index = 0;
        assert_eq!(
            Key::Backspace,
            Key::parse("<backspace>", &mut index).unwrap()
        );
        index = 0;
        assert_eq!(Key::Char(' '), Key::parse("<space>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Enter, Key::parse("<enter>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Left, Key::parse("<left>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Right, Key::parse("<right>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Up, Key::parse("<up>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Down, Key::parse("<down>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Home, Key::parse("<home>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::End, Key::parse("<end>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::PageUp, Key::parse("<pageup>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::PageDown, Key::parse("<pagedown>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Tab, Key::parse("<tab>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Delete, Key::parse("<delete>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Esc, Key::parse("<esc>", &mut index).unwrap());

        for n in 1..=12 {
            index = 0;
            assert_eq!(
                Key::F(n as _),
                Key::parse(&format!("<f{}>", n)[..], &mut index).unwrap()
            );
        }

        index = 0;
        assert_eq!(Key::Ctrl('a'), Key::parse("<c-a>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Ctrl('z'), Key::parse("<c-z>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Ctrl('0'), Key::parse("<c-0>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Ctrl('9'), Key::parse("<c-9>", &mut index).unwrap());

        index = 0;
        assert_eq!(Key::Alt('a'), Key::parse("<a-a>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Alt('z'), Key::parse("<a-z>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Alt('0'), Key::parse("<a-0>", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Alt('9'), Key::parse("<a-9>", &mut index).unwrap());

        index = 0;
        assert_eq!(Key::Char('a'), Key::parse("a", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Char('z'), Key::parse("z", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Char('0'), Key::parse("0", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Char('9'), Key::parse("9", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Char('_'), Key::parse("_", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Char('<'), Key::parse("\\<", &mut index).unwrap());
        index = 0;
        assert_eq!(Key::Char('\\'), Key::parse("\\\\", &mut index).unwrap());
    }
}
