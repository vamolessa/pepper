use std::{error::Error, fmt, str::Chars};

#[derive(Debug)]
pub struct InvalidGlobError;
impl fmt::Display for InvalidGlobError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(stringify!(InvalidGlobError))
    }
}
impl Error for InvalidGlobError {}

pub enum Op {
    Slice { from: u16, to: u16 },
    Separator,
    Skip { count: u16 },
    Many,
    ManyComponents,
    AnyWithinRanges { from: u16, to: u16 },
    ExceptWithinRanges { from: u16, to: u16 },
    SubPatternGroup { len: u16 },
    SubPattern { len: u16 },
}

#[derive(Default)]
pub struct Glob {
    pub texts: String,
    pub ops: Vec<Op>,
}

impl Glob {
    pub fn compile(&mut self, pattern: &str) -> Result<(), InvalidGlobError> {
        self.texts.clear();
        self.ops.clear();

        match self.compile_recursive(pattern.chars()) {
            Ok(rest) if rest.as_str().is_empty() => Ok(()),
            _ => {
                self.texts.clear();
                self.ops.clear();
                Err(InvalidGlobError)
            }
        }
    }

    fn compile_recursive<'a>(
        &mut self,
        mut pattern: Chars<'a>,
    ) -> Result<Chars<'a>, InvalidGlobError> {
        let mut start_ops_index = self.ops.len();
        loop {
            let previous_state = pattern.clone();
            match pattern.next() {
                None => break,
                Some('?') => match self.ops[start_ops_index..].last_mut() {
                    Some(Op::Skip { count }) => *count += 1,
                    _ => self.ops.push(Op::Skip { count: 1 }),
                },
                Some('*') => {
                    let previous_state = pattern.clone();
                    match pattern.next() {
                        Some('*') => {
                            match self.ops.last() {
                                None | Some(Op::Separator) => (),
                                _ => return Err(InvalidGlobError),
                            }

                            let previous_state = pattern.clone();
                            match pattern.next() {
                                None => {
                                    pattern = previous_state;
                                    self.ops.push(Op::ManyComponents);
                                }
                                Some('/') => self.ops.push(Op::ManyComponents),
                                _ => return Err(InvalidGlobError),
                            }
                        }
                        _ => {
                            pattern = previous_state;
                            self.ops.push(Op::Many);
                        }
                    }
                }
                Some('[') => {
                    let previous_state = pattern.clone();
                    let inverse = match pattern.next() {
                        Some('!') => true,
                        _ => {
                            pattern = previous_state;
                            false
                        }
                    };
                    let from = self.texts.len() as _;
                    loop {
                        let from = match pattern.next() {
                            None => return Err(InvalidGlobError),
                            Some(']') => break,
                            Some(c) => c,
                        };
                        let previous_state = pattern.clone();
                        let to = match pattern.next() {
                            Some('-') => {
                                let to = match pattern.next() {
                                    None | Some(']') => return Err(InvalidGlobError),
                                    Some(b) => b,
                                };
                                if to < from {
                                    return Err(InvalidGlobError);
                                }
                                to
                            }
                            _ => {
                                pattern = previous_state;
                                from
                            }
                        };

                        self.texts.push(from);
                        self.texts.push(to);
                    }
                    let to = self.texts.len() as _;
                    if inverse {
                        self.ops.push(Op::ExceptWithinRanges { from, to })
                    } else {
                        self.ops.push(Op::AnyWithinRanges { from, to })
                    }
                }
                Some(']') => return Err(InvalidGlobError),
                Some('{') => {
                    let fix_index = self.ops.len();
                    self.ops.push(Op::SubPatternGroup { len: 0 });

                    loop {
                        let fix_index = self.ops.len();
                        self.ops.push(Op::SubPattern { len: 0 });

                        pattern = self.compile_recursive(pattern)?;

                        let ops_count = self.ops.len();
                        match &mut self.ops[fix_index] {
                            Op::SubPattern { len } => *len = (ops_count - fix_index - 1) as _,
                            _ => unreachable!(),
                        }

                        match pattern.next() {
                            Some('}') => break,
                            Some(',') => continue,
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
                Some('}') | Some(',') => return Ok(previous_state),
                Some('/') => self.ops.push(Op::Separator),
                Some(c) => match self.ops[start_ops_index..].last_mut() {
                    Some(Op::Slice { to, .. }) if *to == self.texts.len() as _ => {
                        self.texts.push(c);
                        *to = self.texts.len() as _;
                    }
                    _ => {
                        let from = self.texts.len() as _;
                        self.texts.push(c);
                        let to = self.texts.len() as _;
                        self.ops.push(Op::Slice { from, to });
                    }
                },
            }
        }

        Ok(pattern)
    }

    pub fn matches(&self, path: &str) -> bool {
        matches_recursive(&self.ops, &self.texts, path.chars(), &Continuation::None)
    }
}

enum Continuation<'this, 'ops> {
    None,
    Next(&'ops [Op], &'this Continuation<'this, 'ops>),
}

fn matches_recursive<'data, 'cont>(
    mut ops: &'data [Op],
    texts: &str,
    mut path: Chars,
    continuation: &'cont Continuation<'cont, 'data>,
) -> bool {
    'op_loop: loop {
        let op = match ops.split_first() {
            Some((op, rest)) => {
                ops = rest;
                op
            }
            None => match continuation {
                Continuation::None => return path.next().is_none(),
                Continuation::Next(ops, continuation) => {
                    return matches_recursive(ops, texts, path, continuation)
                }
            },
        };

        match op {
            &Op::Slice { from, to } => {
                let prefix = &texts[(from as usize)..(to as usize)];
                match path.as_str().strip_prefix(prefix) {
                    Some(rest) => path = rest.chars(),
                    None => return false,
                }
            }
            Op::Separator => match path.next() {
                Some(c) if std::path::is_separator(c) => (),
                _ => return false,
            },
            &Op::Skip { count } => {
                for _ in 0..count {
                    match path.next() {
                        Some(c) if !std::path::is_separator(c) => (),
                        _ => return false,
                    }
                }
            }
            Op::Many => loop {
                if matches_recursive(ops, texts, path.clone(), continuation) {
                    return true;
                }
                match path.next() {
                    Some(c) if !std::path::is_separator(c) => (),
                    _ => return false,
                }
            },
            Op::ManyComponents => loop {
                if matches_recursive(ops, texts, path.clone(), continuation) {
                    return true;
                }
                if path.find(|&c| std::path::is_separator(c)).is_none() {
                    return false;
                }
            },
            &Op::AnyWithinRanges { from, to } => {
                let c = match path.next() {
                    Some(c) => c,
                    None => return false,
                };
                let mut ranges = texts[from as usize..to as usize].chars();
                while let Some(from) = ranges.next() {
                    let to = ranges.next().unwrap();
                    if from <= c && c <= to {
                        continue 'op_loop;
                    }
                }
                return false;
            }
            &Op::ExceptWithinRanges { from, to } => {
                let c = match path.next() {
                    Some(c) => c,
                    None => return false,
                };
                let mut ranges = texts[from as usize..to as usize].chars();
                while let Some(from) = ranges.next() {
                    let to = ranges.next().unwrap();
                    if c < from || to < c {
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
                    if matches_recursive(&ops[..len], texts, path.clone(), &continuation) {
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

