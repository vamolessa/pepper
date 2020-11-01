pub struct InvalidGlobError;

pub fn match_glob(pattern: &str, path: &str) -> Result<bool, InvalidGlobError> {
    Ok(false)
}

enum SubPattern {
    Char(char),
    AnyChar,
    AnySegment,
    AnyMultiSegment,
    Range(char, char),
    ExceptRange(char, char),
    BeginGroup,
}

fn next_subpattern(pattern: &str) -> Result<(SubPattern, &str), InvalidGlobError> {
    let mut chars = pattern.chars();
    loop {
        match chars.next() {
            Some('?') => break Ok((SubPattern::AnyChar, chars.as_str())),
            Some('*') => match chars.as_str().chars().next() {
                Some('*') => {
                    chars.next();
                    break Ok((SubPattern::AnyMultiSegment, chars.as_str()));
                }
                _ => break Ok((SubPattern::AnySegment, chars.as_str())),
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

                if inverse {
                    break Ok((SubPattern::ExceptRange(start, end), chars.as_str()));
                } else {
                    break Ok((SubPattern::Range(start, end), chars.as_str()));
                }
            }
            Some(']') => break Err(InvalidGlobError),
            Some('{') => break Ok((SubPattern::BeginGroup, chars.as_str())),
            Some('}') => (),
            Some(',') => {
                let pattern = chars.as_str();
                match pattern.find('}') {
                    Some(i) => chars = pattern[i..].chars(),
                    None => break Err(InvalidGlobError),
                }
            }
            Some(c) => break Ok((SubPattern::Char(c), chars.as_str())),
            None => break Err(InvalidGlobError),
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
                assert!(matches!(next_subpattern($pattern), Ok(($expect,""))))
            }
        }

        assert_subpattern!(SubPattern::Char('a'), "a");
        assert_subpattern!(SubPattern::Char('z'), "z");
        assert_subpattern!(SubPattern::Char('A'), "A");
        assert_subpattern!(SubPattern::Char('Z'), "Z");
        assert_subpattern!(SubPattern::Char('0'), "0");
        assert_subpattern!(SubPattern::Char('9'), "9");

        assert_subpattern!(SubPattern::AnyChar, "?");
        assert_subpattern!(SubPattern::AnySegment, "*");
        assert_subpattern!(SubPattern::AnyMultiSegment, "**");

        assert_subpattern!(SubPattern::Range('a', 'z'), "[a-z]");
        assert_subpattern!(SubPattern::Range('A', 'Z'), "[A-Z]");
        assert_subpattern!(SubPattern::Range('0', '9'), "[0-9]");
        assert!(matches!(next_subpattern("[z-a"), Err(InvalidGlobError)));
        assert!(matches!(next_subpattern("]"), Err(InvalidGlobError)));
        assert!(matches!(next_subpattern("[z-a]"), Err(InvalidGlobError)));

        assert_subpattern!(SubPattern::ExceptRange('a', 'z'), "[!a-z]");
        assert_subpattern!(SubPattern::ExceptRange('A', 'Z'), "[!A-Z]");
        assert_subpattern!(SubPattern::ExceptRange('0', '9'), "[!0-9]");
        assert!(matches!(next_subpattern("[!z-a"), Err(InvalidGlobError)));
        assert!(matches!(next_subpattern("[!]"), Err(InvalidGlobError)));
        assert!(matches!(next_subpattern("[!z-a]"), Err(InvalidGlobError)));
    }
}
