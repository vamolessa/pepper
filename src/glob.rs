use std::path;

pub struct InvalidGlobError;

pub struct Glob {
    bytes: Vec<u8>,
    patterns: Vec<Pattern>,
}

enum Pattern {
    Any,
    Many,
    ManyComponents,
    Slice(u16, u16),
    AnyWithin(u16, u16),
    ExceptWithin(u16, u16),
    SubPattern(u16, u16),
}

/////////

fn match_glob(pattern: &[u8], path: &[u8]) -> bool {
    match_glob_recursive(pattern, path, false).unwrap_or(false)
}

fn match_glob_recursive(
    pattern: &[u8],
    path: &[u8],
    mut inside_group: bool,
) -> Result<bool, InvalidGlobError> {
    let mut pattern_index = 0;
    let mut path_index = 0;

    macro_rules! next_pattern_byte {
        () => {{
            let i = pattern_index;
            pattern_index += 1;
            if i < pattern.len() {
                Some(pattern[i])
            } else {
                None
            }
        }};
    }

    macro_rules! peek_pattern_byte {
        () => {
            if pattern_index < pattern.len() {
                Some(pattern[pattern_index])
            } else {
                None
            }
        };
    }

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
        match next_pattern_byte!() {
            None => return Ok(path_index == path.len()),
            Some(b'/') => {
                if !path::is_separator(next_path_byte!() as _) {
                    return Ok(false);
                }
            }
            Some(b'?') => {
                next_path_byte!();
            }
            Some(b'*') => {
                let pattern = &pattern[pattern_index..];
                let path = &path[path_index..];
                match peek_pattern_byte!() {
                    Some(b'*') => {
                        pattern_index += 1;
                        if !matches!(peek_pattern_byte!(), None | Some(b'/')) {
                            return Err(InvalidGlobError);
                        }
                        if match_glob_recursive(pattern, path, inside_group)? {
                            return Ok(true);
                        }
                        for (i, _) in path.iter().enumerate().filter(|(_, &b)| b == b'/') {
                            if match_glob_recursive(pattern, &path[i..], inside_group)? {
                                return Ok(true);
                            }
                        }
                        return Ok(false);
                    }
                    _ => {
                        let next_separator_index =
                            path.iter().position(|&b| b == b'/').unwrap_or(path.len());
                        for i in 0..=next_separator_index {
                            if match_glob_recursive(pattern, &path[i..], inside_group)? {
                                return Ok(true);
                            }
                        }
                        return Ok(false);
                    }
                }
            }
            Some(b'[') => {
                let inverse = match peek_pattern_byte!() {
                    Some(b'!') => {
                        pattern_index += 1;
                        true
                    }
                    _ => false,
                };
                loop {
                    let start = match next_pattern_byte!() {
                        None => return Err(InvalidGlobError),
                        Some(b']') => return Ok(false),
                        Some(b) => b,
                    };
                    match peek_pattern_byte!() {
                        Some(b'-') => {
                            pattern_index += 1;
                            let end = match next_pattern_byte!() {
                                None | Some(b']') => return Err(InvalidGlobError),
                                Some(b) => b,
                            };
                            if end < start {
                                return Err(InvalidGlobError);
                            }
                            let b = next_path_byte!();
                            let inside = start <= b && b <= end;
                            if inside != inverse {
                                break;
                            }
                        }
                        Some(b']') => break,
                        _ => {
                            let equal = next_path_byte!() == start;
                            if equal != inverse {
                                break;
                            }
                        }
                    }
                }
                match pattern[pattern_index..].iter().position(|&b| b == b']') {
                    Some(i) => pattern_index += i + 1,
                    None => return Err(InvalidGlobError),
                }
            }
            Some(b']') => return Err(InvalidGlobError),
            Some(b'{') => {
                let mut pattern = &pattern[pattern_index..];
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
            Some(b'}') => {
                if !inside_group {
                    return Err(InvalidGlobError);
                }
                inside_group = false;
            }
            Some(b',') => {
                if !inside_group {
                    return Err(InvalidGlobError);
                }
                match pattern[pattern_index..].iter().position(|&b| b == b'}') {
                    Some(i) => pattern_index += i + 1,
                    None => return Err(InvalidGlobError),
                }
                inside_group = false;
            }
            Some(b) => {
                if next_path_byte!() != b {
                    return Ok(false);
                }
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
        assert_eq!(true, match_glob(b"a*{b,c}d", b"aaabd"));
        assert_eq!(true, match_glob(b"a*{b,c}d", b"abbd"));
        assert_eq!(true, match_glob(b"a*{b*,c}d", b"abbzzzzd"));
    }
}
