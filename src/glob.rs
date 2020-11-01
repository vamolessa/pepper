use std::path;

pub struct InvalidGlobError;

fn match_glob_recursive(
    pattern: &[u8],
    path: &[u8],
    inside_group: bool,
) -> Result<bool, InvalidGlobError> {
    if pattern.is_empty() && path.is_empty() {
        return Ok(true);
    }

    let mut state = State {
        pattern,
        next_index: 0,
        inside_group,
    };

    let mut next_path_index = 0;
    macro_rules! next_path_byte {
        () => {{
            let i = next_path_index;
            next_path_index += 1;
            if i < path.len() {
                let b = path[i];
                if path::is_separator(b as _) {
                    b'/'
                } else {
                    b
                }
            } else {
                return Ok(false);
            }
        }};
    }

    loop {
        match state.next_subpattern()? {
            SubPattern::None => return Ok(next_path_index == path.len()),
            SubPattern::Byte(b) => {
                if next_path_byte!() != b {
                    return Ok(false);
                }
            }
            SubPattern::AnyByte => {
                next_path_byte!();
            }
            SubPattern::AnySegment => {
                unimplemented!();
            }
            SubPattern::AnyMultiSegment=> {
                unimplemented!();
                }
            SubPattern::Range(start, end) => {
                let b = next_path_byte!();
                if b < start || end > b {
                    return Ok(false);
                }
            }
            SubPattern::ExceptRange(start, end) => {
                let b = next_path_byte!();
                if start <= b && b <= end {
                    return Ok(false);
                }
            }
            SubPattern::BeginGroup => {
                unimplemented!();
            }
        }
    }
}

enum SubPattern {
    None,
    Byte(u8),
    AnyByte,
    AnySegment,
    AnyMultiSegment,
    Range(u8, u8),
    ExceptRange(u8, u8),
    BeginGroup,
}

struct State<'a> {
    pattern: &'a [u8],
    next_index: usize,
    inside_group: bool,
}

impl<'a> State<'a> {
    pub fn next_subpattern(&mut self) -> Result<SubPattern, InvalidGlobError> {
        macro_rules! next_byte {
            () => {{
                let i = self.next_index;
                self.next_index += 1;
                if i < self.pattern.len() {
                    Some(self.pattern[i])
                } else {
                    None
                }
            }};
        }

        macro_rules! peek_byte {
            () => {
                if self.next_index < self.pattern.len() {
                    Some(self.pattern[self.next_index])
                } else {
                    None
                }
            };
        }

        loop {
            match next_byte!() {
                Some(b'?') => return Ok(SubPattern::AnyByte),
                Some(b'*') => match peek_byte!() {
                    Some(b'*') => {
                        self.next_index += 1;
                        match peek_byte!() {
                            None | Some(b'/') => return Ok(SubPattern::AnyMultiSegment),
                            _ => return Err(InvalidGlobError),
                        }
                    }
                    _ => return Ok(SubPattern::AnySegment),
                },
                Some(b'[') => {
                    let inverse = match peek_byte!() {
                        Some(b'!') => {
                            self.next_index += 1;
                            true
                        }
                        _ => false,
                    };
                    let start = match next_byte!() {
                        Some(c) => c,
                        None => return Err(InvalidGlobError),
                    };
                    if next_byte!() != Some(b'-') {
                        return Err(InvalidGlobError);
                    }
                    let end = match next_byte!() {
                        Some(c) => c,
                        None => return Err(InvalidGlobError),
                    };
                    if start > end {
                        return Err(InvalidGlobError);
                    }
                    if next_byte!() != Some(b']') {
                        return Err(InvalidGlobError);
                    }

                    if inverse {
                        return Ok(SubPattern::ExceptRange(start, end));
                    } else {
                        return Ok(SubPattern::Range(start, end));
                    }
                }
                Some(b']') => return Err(InvalidGlobError),
                Some(b'{') => {
                    self.inside_group = true;
                    return Ok(SubPattern::BeginGroup);
                }
                Some(b'}') => {
                    if !self.inside_group {
                        return Err(InvalidGlobError);
                    }
                    self.inside_group = false;
                }
                Some(b',') => {
                    if !self.inside_group {
                        return Err(InvalidGlobError);
                    }
                    match self.pattern[self.next_index..]
                        .iter()
                        .position(|b| *b == b'}')
                    {
                        Some(i) => self.next_index += i,
                        None => return Err(InvalidGlobError),
                    }
                    self.inside_group = false;
                }
                Some(b) => return Ok(SubPattern::Byte(b)),
                None => return Ok(SubPattern::None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_class() {
        macro_rules! assert_subpattern {
            ($expect:pat, $pattern:expr) => {
                let mut state = State {
                    pattern: $pattern.as_bytes(),
                    next_index: 0,
                    inside_group: false,
                };
                assert!(matches!(state.next_subpattern(), Ok($expect)))
            };
        }
        macro_rules! assert_subpattern_fail {
            ($pattern:expr) => {
                let mut state = State {
                    pattern: $pattern.as_bytes(),
                    next_index: 0,
                    inside_group: false,
                };
                assert!(matches!(state.next_subpattern(), Err(InvalidGlobError)))
            };
        }

        assert_subpattern!(SubPattern::None, "");

        assert_subpattern!(SubPattern::Byte(b'a'), "a");
        assert_subpattern!(SubPattern::Byte(b'z'), "z");
        assert_subpattern!(SubPattern::Byte(b'A'), "A");
        assert_subpattern!(SubPattern::Byte(b'Z'), "Z");
        assert_subpattern!(SubPattern::Byte(b'0'), "0");
        assert_subpattern!(SubPattern::Byte(b'9'), "9");

        assert_subpattern!(SubPattern::AnyByte, "?");
        assert_subpattern!(SubPattern::AnySegment, "*");
        assert_subpattern!(SubPattern::AnyMultiSegment, "**");

        assert_subpattern!(SubPattern::Range(b'a', b'z'), "[a-z]");
        assert_subpattern!(SubPattern::Range(b'A', b'Z'), "[A-Z]");
        assert_subpattern!(SubPattern::Range(b'0', b'9'), "[0-9]");
        assert_subpattern_fail!("[a-z");
        assert_subpattern_fail!("]");
        assert_subpattern_fail!("[z-a]");

        assert_subpattern!(SubPattern::ExceptRange(b'a', b'z'), "[!a-z]");
        assert_subpattern!(SubPattern::ExceptRange(b'A', b'Z'), "[!A-Z]");
        assert_subpattern!(SubPattern::ExceptRange(b'0', b'9'), "[!0-9]");
        assert_subpattern_fail!("[!a-z");
        assert_subpattern_fail!("[!]");
        assert_subpattern_fail!("[!z-a]");

        assert_subpattern!(SubPattern::BeginGroup, "{");
        assert_subpattern_fail!("}");
        assert_subpattern_fail!(",");
    }
}
