use std::{error::Error, fmt};

#[derive(Debug)]
pub struct InvalidGlobError;
impl fmt::Display for InvalidGlobError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(stringify!(InvalidGlobError))
    }
}
impl Error for InvalidGlobError {}

#[derive(Debug)]
pub enum Op {
    Slice { from: u16, to: u16 },
    Separator,
    Skip { len: u16 },
    Many,
    ManyComponents,
    AnyWithinRanges { start: u16, count: u16 },
    ExceptWithinRanges { start: u16, count: u16 },
    SubPatternGroup { len: u16 },
    SubPattern { len: u16 },
}

#[derive(Default)]
pub struct Glob {
    pub bytes: Vec<u8>,
    pub ops: Vec<Op>,
}

impl Glob {
    pub fn compile(&mut self, pattern: &str) -> Result<(), InvalidGlobError> {
        self.bytes.clear();
        self.ops.clear();

        match self.compile_recursive(pattern.as_bytes()) {
            Ok(len) if len == pattern.len() => Ok(()),
            _ => {
                self.bytes.clear();
                self.ops.clear();
                Err(InvalidGlobError)
            }
        }
    }

    fn compile_recursive(&mut self, pattern: &[u8]) -> Result<usize, InvalidGlobError> {
        let mut start_ops_index = self.ops.len();
        let mut index = 0;

        #[inline]
        fn next(pattern: &[u8], index: &mut usize) -> Option<u8> {
            let i = *index;
            if i < pattern.len() {
                *index += 1;
                Some(pattern[i])
            } else {
                None
            }
        }

        #[inline]
        fn peek(pattern: &[u8], index: usize) -> Option<u8> {
            if index < pattern.len() {
                Some(pattern[index])
            } else {
                None
            }
        }

        loop {
            match next(pattern, &mut index) {
                None => break,
                Some(b'?') => match self.ops[start_ops_index..].last_mut() {
                    Some(Op::Skip { len }) => *len += 1,
                    _ => self.ops.push(Op::Skip { len: 1 }),
                },
                Some(b'*') => match peek(pattern, index) {
                    Some(b'*') => {
                        match self.ops.last() {
                            None | Some(Op::Separator) => (),
                            _ => return Err(InvalidGlobError),
                        }

                        index += 1;
                        match peek(pattern, index) {
                            None => self.ops.push(Op::ManyComponents),
                            Some(b'/') => {
                                index += 1;
                                self.ops.push(Op::ManyComponents);
                            }
                            _ => return Err(InvalidGlobError),
                        }
                    }
                    _ => self.ops.push(Op::Many),
                },
                Some(b'[') => {
                    let inverse = match peek(pattern, index) {
                        Some(b'!') => {
                            index += 1;
                            true
                        }
                        _ => false,
                    };
                    let start = self.bytes.len();
                    loop {
                        let start = match next(pattern, &mut index) {
                            None => return Err(InvalidGlobError),
                            Some(b']') => break,
                            Some(b) => b,
                        };
                        let end = match peek(pattern, index) {
                            Some(b'-') => {
                                index += 1;
                                let end = match next(pattern, &mut index) {
                                    None | Some(b']') => return Err(InvalidGlobError),
                                    Some(b) => b,
                                };
                                if end < start {
                                    return Err(InvalidGlobError);
                                }
                                end
                            }
                            _ => start,
                        };

                        self.bytes.push(start);
                        self.bytes.push(end);
                    }
                    let count = ((self.bytes.len() - start) / 2) as _;
                    let start = start as _;
                    if inverse {
                        self.ops.push(Op::ExceptWithinRanges { start, count })
                    } else {
                        self.ops.push(Op::AnyWithinRanges { start, count })
                    }
                }
                Some(b']') => return Err(InvalidGlobError),
                Some(b'{') => {
                    let fix_index = self.ops.len();
                    self.ops.push(Op::SubPatternGroup { len: 0 });

                    loop {
                        let fix_index = self.ops.len();
                        self.ops.push(Op::SubPattern { len: 0 });

                        index += self.compile_recursive(&pattern[index..])?;

                        let ops_count = self.ops.len();
                        match &mut self.ops[fix_index] {
                            Op::SubPattern { len } => *len = (ops_count - fix_index - 1) as _,
                            _ => unreachable!(),
                        }

                        match next(pattern, &mut index) {
                            Some(b'}') => break,
                            Some(b',') => continue,
                            _ => return Err(InvalidGlobError),
                        }
                    }

                    let ops_count = self.ops.len();
                    match &mut self.ops[fix_index] {
                        Op::SubPatternGroup { len } => *len = (ops_count - fix_index - 1) as _,
                        _ => unreachable!(),
                    }

                    start_ops_index = self.ops.len();
                }
                Some(b'}') | Some(b',') => {
                    index -= 1;
                    break;
                }
                Some(b'/') => self.ops.push(Op::Separator),
                Some(b) => match self.ops[start_ops_index..].last_mut() {
                    Some(Op::Slice { to, .. }) if *to == self.bytes.len() as u16 => {
                        self.bytes.push(b);
                        *to += 1;
                    }
                    _ => {
                        let from = self.bytes.len() as _;
                        let to = from + 1;
                        self.bytes.push(b);
                        self.ops.push(Op::Slice { from, to });
                    }
                },
            }
        }

        Ok(index)
    }

    pub fn matches(&self, path: &str) -> bool {
        matches_recursive(&self.ops, &self.bytes, path.as_bytes(), &Continuation::None)
    }
}

enum Continuation<'this, 'ops> {
    None,
    Next(&'ops [Op], &'this Continuation<'this, 'ops>),
}

fn matches_recursive<'data, 'cont>(
    mut ops: &'data [Op],
    bytes: &'data [u8],
    mut path: &'data [u8],
    continuation: &'cont Continuation<'cont, 'data>,
) -> bool {
    #[inline]
    fn is_path_separator(b: &u8) -> bool {
        std::path::is_separator(*b as _)
    }

    'op_loop: loop {
        let op = match ops.split_first() {
            Some((op, rest)) => {
                ops = rest;
                op
            }
            None => match continuation {
                Continuation::None => return path.is_empty(),
                Continuation::Next(ops, continuation) => {
                    return matches_recursive(ops, bytes, path, continuation)
                }
            },
        };

        match op {
            &Op::Slice { from, to } => {
                let prefix = &bytes[(from as usize)..(to as usize)];
                if !path.starts_with(prefix) {
                    return false;
                }
                path = &path[prefix.len()..];
            }
            Op::Separator => {
                if path.is_empty() || !is_path_separator(&path[0]) {
                    return false;
                }
                path = &path[1..];
            }
            &Op::Skip { len } => {
                let len = len as usize;
                if path.len() < len || path[..len].iter().any(is_path_separator) {
                    return false;
                }
                path = &path[len..];
            }
            Op::Many => loop {
                if matches_recursive(ops, bytes, path, continuation) {
                    return true;
                }
                if path.is_empty() || is_path_separator(&path[0]) {
                    return false;
                }
                path = &path[1..];
            },
            Op::ManyComponents => loop {
                if matches_recursive(ops, bytes, path, continuation) {
                    return true;
                }
                if path.is_empty() {
                    return false;
                }
                match path.iter().position(is_path_separator) {
                    Some(i) => path = &path[(i + 1)..],
                    None => return false,
                }
            },
            &Op::AnyWithinRanges { start, count } => {
                if path.is_empty() {
                    return false;
                }
                let b = path[0];
                path = &path[1..];
                for range in bytes[(start as usize)..].chunks(2).take(count as _) {
                    let start = range[0];
                    let end = range[1];
                    if start <= b && b <= end {
                        continue 'op_loop;
                    }
                }
                return false;
            }
            &Op::ExceptWithinRanges { start, count } => {
                if path.is_empty() {
                    return false;
                }
                let b = path[0];
                path = &path[1..];
                for range in bytes[(start as usize)..].chunks(2).take(count as _) {
                    let start = range[0];
                    let end = range[1];
                    if b < start || end < b {
                        continue 'op_loop;
                    }
                }
                return false;
            }
            &Op::SubPatternGroup { len } => {
                let (mut ops, jump) = ops.split_at(len as _);
                while !ops.is_empty() {
                    let len = match ops[0] {
                        Op::SubPattern { len } => len as usize,
                        _ => unreachable!(),
                    };
                    ops = &ops[1..];
                    let continuation = Continuation::Next(jump, continuation);
                    if matches_recursive(&ops[..len], bytes, path, &continuation) {
                        return true;
                    }
                    ops = &ops[len..];
                }
                return false;
            }
            Op::SubPattern { .. } => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile() {
        let mut glob = Glob::default();

        assert!(glob.compile("").is_ok());
        assert!(glob.compile("abc").is_ok());
        assert!(glob.compile("a?c").is_ok());
        assert!(glob.compile("a[A-Z]c").is_ok());
        assert!(glob.compile("a[!0-9]c").is_ok());

        assert!(glob.compile("a*c").is_ok());
        assert!(glob.compile("a*/").is_ok());
        assert!(glob.compile("a*/c").is_ok());
        assert!(glob.compile("a*[0-9]/c").is_ok());
        assert!(glob.compile("a*bx*cy*d").is_ok());

        assert!(glob.compile("**").is_ok());
        assert!(glob.compile("/**").is_ok());
        assert!(glob.compile("**/").is_ok());
        assert!(glob.compile("a/**/").is_ok());
        assert!(glob.compile("a/**/c").is_ok());
        assert!(glob.compile("a/**c").is_err());
        assert!(glob.compile("a**/c").is_err());

        assert!(glob.compile("a{b,c}d").is_ok());
        assert!(glob.compile("a*{b,c}d").is_ok());
        assert!(glob.compile("a*{b*,c}d").is_ok());
        assert!(glob.compile("}").is_err());
        assert!(glob.compile(",").is_err());
    }

    #[test]
    fn matches() {
        fn assert_glob(glob: &mut Glob, expected: bool, pattern: &str, path: &str) {
            assert!(
                glob.compile(pattern).is_ok(),
                "invalid glob pattern '{}'",
                pattern,
            );
            assert_eq!(
                expected,
                glob.matches(path),
                "'{}' did {} match pattern '{}'",
                path,
                if expected { " not" } else { "" },
                pattern,
            );
        }

        let mut glob = Glob::default();

        assert_glob(&mut glob, true, "", "");
        assert_glob(&mut glob, true, "abc", "abc");
        assert_glob(&mut glob, false, "ab", "abc");
        assert_glob(&mut glob, true, "a?c", "abc");
        assert_glob(&mut glob, false, "a??", "a/c");
        assert_glob(&mut glob, true, "a[A-Z]c", "aBc");
        assert_glob(&mut glob, false, "a[A-Z]c", "abc");
        assert_glob(&mut glob, true, "a[!0-9A-CD-FGH]c", "abc");

        assert_glob(&mut glob, true, "*", "");
        assert_glob(&mut glob, true, "*", "a");
        assert_glob(&mut glob, true, "*", "abc");
        assert_glob(&mut glob, true, "a*c", "ac");
        assert_glob(&mut glob, true, "a*c", "abc");
        assert_glob(&mut glob, true, "a*c", "abbbc");
        assert_glob(&mut glob, true, "a*/", "abc/");
        assert_glob(&mut glob, true, "a*/c", "a/c");
        assert_glob(&mut glob, true, "a*/c", "abbb/c");
        assert_glob(&mut glob, true, "a*[0-9]/c", "abbb5/c");
        assert_glob(&mut glob, false, "a*c", "a/c");
        assert_glob(&mut glob, true, "a*bx*cy*d", "a00bx000cy0000d");

        assert_glob(&mut glob, false, "a/**/c", "");
        assert_glob(&mut glob, true, "a/**/c", "a/c");
        assert_glob(&mut glob, true, "a/**/c", "a/b/c");
        assert_glob(&mut glob, true, "a/**/c", "a/bb/bbb/c");
        assert_glob(&mut glob, true, "a/**/c", "a/a/bb/bbb/c");
        assert_glob(&mut glob, true, "**/c", "c");
        assert_glob(&mut glob, true, "**/c", "a/c");
        assert_glob(&mut glob, false, "**/c", "ac");
        assert_glob(&mut glob, false, "**/c", "a/bc");
        assert_glob(&mut glob, true, "**/c", "ab/c");
        assert_glob(&mut glob, true, "**/c", "a/b/c");

        assert_glob(&mut glob, true, "a{b,c}d", "abd");
        assert_glob(&mut glob, true, "a{b,c}d", "acd");
        assert_glob(&mut glob, true, "a*{b,c}d", "aaabd");
        assert_glob(&mut glob, true, "a*{b,c}d", "abbbd");
        assert_glob(&mut glob, true, "a*{b*,c}d", "acdbbczzcd");
        assert_glob(&mut glob, true, "a{b,c*}d", "aczd");
        assert_glob(&mut glob, true, "a*{b,c*}d", "acdbczzzd");

        assert_glob(&mut glob, false, "**/*.{a,b,cd}", "");
        assert_glob(&mut glob, true, "**/*.{a,b,cd}", "n.a");
        assert_glob(&mut glob, true, "**/*.{a,b,cd}", "n.b");
        assert_glob(&mut glob, true, "**/*.{a,b,cd}", "n.cd");
        assert_glob(&mut glob, true, "**/*.{a,b,cd}", "m/n.a");
        assert_glob(&mut glob, true, "**/*.{a,b,cd}", "m/n.b");
        assert_glob(&mut glob, true, "**/*.{a,b,cd}", "m/n.cd");
        assert_glob(&mut glob, true, "**/*.{a,b,cd}", "m/n/p.a");
        assert_glob(&mut glob, true, "**/*.{a,b,cd}", "m/n/p.b");
        assert_glob(&mut glob, true, "**/*.{a,b,cd}", "m/n/p.cd");
        assert_glob(&mut glob, false, "**/*.{a,b,cd}", "n.x");
        assert_glob(&mut glob, false, "**/*.{a,b,cd}", "m/n.x");
        assert_glob(&mut glob, false, "**/*.{a,b,cd}", "m/n/p.x");
    }
}

