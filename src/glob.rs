use std::path;

pub struct InvalidGlobError;

fn match_glob(pattern: &[u8], path: &[u8]) -> bool {
    match_glob_recursive(pattern, path, false).unwrap_or(false)
}

fn match_glob_recursive(
    pattern: &[u8],
    path: &[u8],
    inside_group: bool,
) -> Result<bool, InvalidGlobError> {
    let mut state = State {
        pattern,
        index: 0,
        inside_group,
    };

    let mut path_index = 0;
    let mut try_path_index = 0;
    let mut try_pattern_index = 0;

    macro_rules! check_next_path_byte {
        ($byte:ident => $check:expr) => {{
            let i = path_index;
            path_index += 1;
            if i < path.len() {
                let $byte = path[i];
                if $check {
                    continue;
                }
            }
        }};
    }

    loop {
        match state.next_subpattern()? {
            SubPattern::None => {
                if path_index == path.len() {
                    return Ok(true);
                }
            }
            SubPattern::Byte(b'/') => check_next_path_byte!(b => path::is_separator(b as _)),
            SubPattern::Byte(pb) => check_next_path_byte!(b => b == pb),
            SubPattern::AnyByte => {
                if path_index < path.len() {
                    path_index += 1;
                    continue;
                }
            }
            SubPattern::AnySegment => {
                try_pattern_index = state.index - 1;
                try_path_index = path_index + 1;
                continue;

                /*
                let rest = &path[path_index..];
                let next_separator_pos = match rest.iter().position(|&b| path::is_separator(b as _))
                {
                    Some(i) => i,
                    None => rest.len(),
                };

                match state.next_subpattern()? {
                    SubPattern::None | SubPattern::Byte(b'/') => {
                        path_index += next_separator_pos;
                    }
                    SubPattern::Byte(pb) => {
                        match rest[..next_separator_pos].iter().position(|&b| b == pb) {
                            Some(i) => path_index += i + 1,
                            None => return Ok(false),
                        }
                    }
                    SubPattern::AnyByte => {
                        if next_separator_pos > 0 {
                            path_index += 2;
                        } else {
                            return Ok(false);
                        }
                    }
                    SubPattern::AnySegment => unreachable!(),
                    SubPattern::AnyMultiSegment => unreachable!(),
                    SubPattern::Range(start, end) => {
                        match rest[..next_separator_pos]
                            .iter()
                            .position(|&b| start <= b && b <= end)
                        {
                            Some(i) => path_index += i + 1,
                            None => return Ok(false),
                        }
                    }
                    SubPattern::ExceptRange(start, end) => {
                        match rest[..next_separator_pos]
                            .iter()
                            .position(|&b| b < start || end < start)
                        {
                            Some(i) => path_index += i + 1,
                            None => return Ok(false),
                        }
                    }
                    SubPattern::BeginGroup => {
                        unimplemented!();
                    }
                }
                */
            }
            SubPattern::AnyMultiSegment => {
                unimplemented!();
            }
            SubPattern::Range(start, end) => check_next_path_byte!(b => start <= b && b <= end),
            SubPattern::ExceptRange(start, end) => check_next_path_byte!(b => b < start || end < b),
            SubPattern::BeginGroup => {
                unimplemented!();
            }
        }

        if 0 < try_path_index && try_path_index <= path.len() {
            state.index = try_pattern_index;
            path_index = try_path_index;
            continue;
        }

        return Ok(false);
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
    index: usize,
    inside_group: bool,
}

impl<'a> State<'a> {
    pub fn next_subpattern(&mut self) -> Result<SubPattern, InvalidGlobError> {
        macro_rules! next_byte {
            () => {{
                let i = self.index;
                self.index += 1;
                if i < self.pattern.len() {
                    Some(self.pattern[i])
                } else {
                    None
                }
            }};
        }

        macro_rules! peek_byte {
            () => {
                if self.index < self.pattern.len() {
                    Some(self.pattern[self.index])
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
                        self.index += 1;
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
                            self.index += 1;
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
                    match self.pattern[self.index..].iter().position(|&b| b == b'}') {
                        Some(i) => self.index += i,
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
    fn test_match() {
        assert!(match_glob(b"", b""));
        assert!(match_glob(b"abc", b"abc"));
        assert!(match_glob(b"a?c", b"abc"));
        assert!(match_glob(b"a[A-Z]c", b"aBc"));
        assert!(!match_glob(b"a[A-Z]c", b"abc"));
        assert!(match_glob(b"a[!0-9]c", b"abc"));
    }

    #[test]
    fn test_subpattern() {
        macro_rules! assert_subpattern {
            ($expect:pat, $pattern:expr) => {
                let mut state = State {
                    pattern: $pattern.as_bytes(),
                    index: 0,
                    inside_group: false,
                };
                assert!(matches!(state.next_subpattern(), Ok($expect)))
            };
        }
        macro_rules! assert_subpattern_fail {
            ($pattern:expr) => {
                let mut state = State {
                    pattern: $pattern.as_bytes(),
                    index: 0,
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
