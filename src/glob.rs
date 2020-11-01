pub struct InvalidGlobError;

fn match_glob_recursive(
    mut pattern: &str,
    path: &str,
    mut inside_group: bool,
) -> Result<bool, InvalidGlobError> {
    if pattern.is_empty() && path.is_empty() {
        return Ok(true);
    }

    let mut chars = path.chars();

    macro_rules! next_char {
        () => {
            match chars.next() {
                Some(c) => c,
                None => return Ok(false),
            }
        };
    }

    loop {
        let (subpattern, rest, ingroup) = next_subpattern(pattern, inside_group)?;
        pattern = rest;
        inside_group = ingroup;

        match subpattern {
            SubPattern::None => match chars.next() {
                None => break Ok(true),
                _ => break Ok(false),
            },
            SubPattern::Char(c) => {
                if next_char!() != c {
                    break Ok(false);
                }
            }
            SubPattern::AnyChar => {
                next_char!();
            }
            SubPattern::AnySegment => {
                unimplemented!();
            }
            SubPattern::AnyMultiSegment => {
                unimplemented!();
            }
            SubPattern::Range(start, end) => {
                let c = next_char!();
                if c < start || end > c {
                    break Ok(false);
                }
            }
            SubPattern::ExceptRange(start, end) => {
                let c = next_char!();
                if start <= c && c <= end {
                    break Ok(false);
                }
            }
            SubPattern::BeginGroup => {}
            _ => (),
        }
    }
}

enum SubPattern {
    None,
    Char(char),
    Separator,
    AnyChar,
    AnySegment,
    AnyMultiSegment,
    Range(char, char),
    ExceptRange(char, char),
    BeginGroup,
}

fn next_subpattern(
    pattern: &str,
    mut inside_group: bool,
) -> Result<(SubPattern, &str, bool), InvalidGlobError> {
    let mut chars = pattern.chars();
    loop {
        match chars.next() {
            Some('?') => break Ok((SubPattern::AnyChar, chars.as_str(), inside_group)),
            Some('*') => match chars.as_str().chars().next() {
                Some('*') => {
                    chars.next();
                    let rest = chars.as_str();
                    match rest.chars().next() {
                        None | Some('/') => {
                            break Ok((SubPattern::AnyMultiSegment, rest, inside_group))
                        }
                        _ => break Err(InvalidGlobError),
                    }
                }
                _ => break Ok((SubPattern::AnySegment, chars.as_str(), inside_group)),
            },
            Some('[') => {
                let inverse = match chars.as_str().chars().next() {
                    Some('!') => {
                        chars.next();
                        true
                    }
                    _ => false,
                };

                let start = match chars.next() {
                    Some(c) => c,
                    None => break Err(InvalidGlobError),
                };
                if chars.next() != Some('-') {
                    break Err(InvalidGlobError);
                }
                let end = match chars.next() {
                    Some(c) => c,
                    _ => break Err(InvalidGlobError),
                };
                if start > end {
                    break Err(InvalidGlobError);
                }
                if chars.next() != Some(']') {
                    break Err(InvalidGlobError);
                }

                let rest = chars.as_str();
                if inverse {
                    break Ok((SubPattern::ExceptRange(start, end), rest, inside_group));
                } else {
                    break Ok((SubPattern::Range(start, end), rest, inside_group));
                }
            }
            Some(']') => break Err(InvalidGlobError),
            Some('{') => break Ok((SubPattern::BeginGroup, chars.as_str(), true)),
            Some('}') => {
                if !inside_group {
                    break Err(InvalidGlobError);
                }
                inside_group = false;
            }
            Some(',') => {
                if !inside_group {
                    break Err(InvalidGlobError);
                }

                let rest = chars.as_str();
                match rest.find('}') {
                    Some(i) => chars = rest[i..].chars(),
                    None => break Err(InvalidGlobError),
                }
                inside_group = false;
            }
            Some('/') => break Ok((SubPattern::Separator, chars.as_str(), inside_group)),
            Some(c) => break Ok((SubPattern::Char(c), chars.as_str(), inside_group)),
            None => break Ok((SubPattern::None, "", false)),
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
                assert!(matches!(
                    next_subpattern($pattern, false),
                    Ok(($expect, "", false))
                ))
            };
        }
        macro_rules! assert_subpattern_fail {
            ($pattern:expr) => {
                assert!(matches!(
                    next_subpattern($pattern, false),
                    Err(InvalidGlobError)
                ))
            };
        }

        assert_subpattern!(SubPattern::None, "");

        assert_subpattern!(SubPattern::Char('a'), "a");
        assert_subpattern!(SubPattern::Char('z'), "z");
        assert_subpattern!(SubPattern::Char('A'), "A");
        assert_subpattern!(SubPattern::Char('Z'), "Z");
        assert_subpattern!(SubPattern::Char('0'), "0");
        assert_subpattern!(SubPattern::Char('9'), "9");

        assert_subpattern!(SubPattern::Separator, "/");
        assert_subpattern!(SubPattern::AnyChar, "?");
        assert_subpattern!(SubPattern::AnySegment, "*");
        assert_subpattern!(SubPattern::AnyMultiSegment, "**");

        assert_subpattern!(SubPattern::Range('a', 'z'), "[a-z]");
        assert_subpattern!(SubPattern::Range('A', 'Z'), "[A-Z]");
        assert_subpattern!(SubPattern::Range('0', '9'), "[0-9]");
        assert_subpattern_fail!("[a-z");
        assert_subpattern_fail!("]");
        assert_subpattern_fail!("[z-a]");

        assert_subpattern!(SubPattern::ExceptRange('a', 'z'), "[!a-z]");
        assert_subpattern!(SubPattern::ExceptRange('A', 'Z'), "[!A-Z]");
        assert_subpattern!(SubPattern::ExceptRange('0', '9'), "[!0-9]");
        assert_subpattern_fail!("[!a-z");
        assert_subpattern_fail!("[!]");
        assert_subpattern_fail!("[!z-a]");

        assert!(matches!(
            next_subpattern("{", true),
            Ok((SubPattern::BeginGroup, "", true))
        ));
        assert_subpattern_fail!("}");
        assert_subpattern_fail!(",");
    }
}
