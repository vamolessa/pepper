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
    macro_rules! next_path_byte {
        () => {{
            let i = path_index;
            path_index += 1;
            if i < path.len() {
                path[i]
            } else {
                return Ok(false);
            }
        }};
    }

    loop {
        match state.next_subpattern()? {
            SubPattern::None => return Ok(path_index == path.len()),
            SubPattern::Byte(b'/') => {
                if !path::is_separator(next_path_byte!() as _) {
                    return Ok(false);
                }
            }
            SubPattern::Byte(b) => {
                if next_path_byte!() != b {
                    return Ok(false);
                }
            }
            SubPattern::AnyByte => {
                next_path_byte!();
            }
            SubPattern::AnySegment => {
                let pattern = &pattern[state.index..];
                let path = &path[path_index..];
                let next_separator_index =
                    path.iter().position(|&b| b == b'/').unwrap_or(path.len());
                for i in 0..=next_separator_index {
                    if match_glob_recursive(pattern, &path[i..], state.inside_group)? {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
            SubPattern::AnyMultiSegment => {
                let pattern = &pattern[state.index..];
                let path = &path[path_index..];
                if match_glob_recursive(pattern, path, state.inside_group)? {
                    return Ok(true);
                }
                for (i, _) in path.iter().enumerate().filter(|(_, &b)| b == b'/') {
                    if match_glob_recursive(pattern, &path[i..], state.inside_group)? {
                        return Ok(true);
                    }
                }
                return Ok(false);
            }
            SubPattern::Range(start, end) => {
                let b = next_path_byte!();
                if b < start || end < b {
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
                let mut pattern = &pattern[state.index..];
                let path = &path[path_index..];

                while !match_glob_recursive(pattern, path, true)? {
                    match pattern
                        .iter()
                        .enumerate()
                        .find(|(_, &b)| b == b',' || b == b'}')
                    {
                        Some((i, b',')) => pattern = &pattern[(i + 1)..],
                        _ => return Ok(false),
                    }
                }

                return Ok(true);
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
                        Some(i) => self.index += i + 1,
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
        assert_eq!(true, match_glob(b"", b""));
        assert_eq!(true, match_glob(b"abc", b"abc"));
        assert_eq!(false, match_glob(b"ab", b"abc"));
        assert_eq!(true, match_glob(b"a?c", b"abc"));
        assert_eq!(true, match_glob(b"a[A-Z]c", b"aBc"));
        assert_eq!(false, match_glob(b"a[A-Z]c", b"abc"));
        assert_eq!(true, match_glob(b"a[!0-9]c", b"abc"));

        assert_eq!(true, match_glob(b"a*c", b"ac"));
        assert_eq!(true, match_glob(b"a*c", b"abc"));
        assert_eq!(true, match_glob(b"a*c", b"abbbc"));
        assert_eq!(true, match_glob(b"a*/", b"abc/"));
        assert_eq!(true, match_glob(b"a*/c", b"a/c"));
        assert_eq!(true, match_glob(b"a*/c", b"abbb/c"));
        assert_eq!(true, match_glob(b"a*[0-9]/c", b"abbb5/c"));
        assert_eq!(false, match_glob(b"a*c", b"a/c"));
        assert_eq!(true, match_glob(b"a*bx*cy*d", b"a00bx000cy0000d"));

        assert_eq!(false, match_glob(b"a**c", b"ac"));
        assert_eq!(false, match_glob(b"a**c", b"a/c"));
        assert_eq!(true, match_glob(b"a**/c", b"a/c"));
        assert_eq!(true, match_glob(b"a**/c", b"a/b/c"));
        assert_eq!(true, match_glob(b"a**/c", b"a/bbb/c"));
        assert_eq!(true, match_glob(b"a**/c", b"aaa/b/c"));

        assert_eq!(true, match_glob(b"a{b,c}d", b"abd"));
        assert_eq!(true, match_glob(b"a{b,c}d", b"acd"));
        //assert_eq!(true, match_glob(b"*a{b,c}d", b"aaabd"));
        //assert_eq!(true, match_glob(b"*a{b,c}d", b"abbd"));
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
