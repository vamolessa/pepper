use std::path;

pub struct InvalidGlobError(());
impl InvalidGlobError {
    pub const fn new() -> Self {
        //panic!();
        Self(())
    }
}

macro_rules! dbg_bytes {
    ($bytes:expr) => {
        eprintln!(
            "{} = {}",
            stringify!($bytes),
            std::str::from_utf8($bytes).unwrap()
        )
    };
}

enum Op {
    Slice { from: u16, to: u16 },
    Skip { len: u16 },
    Many,
    ManyComponents,
    AnyWithinRanges { start: u16, count: u16 },
    ExceptWithinRanges { start: u16, count: u16 },
    SubPattern { len: u16 },
    End,
}

struct NoMatch;

#[derive(Default)]
pub struct Glob {
    bytes: Vec<u8>,
    ops: Vec<Op>,
}

impl Glob {
    pub fn compile(&mut self, pattern: &[u8]) -> Result<(), InvalidGlobError> {
        self.bytes.clear();
        self.ops.clear();

        match self.compile_recursive(pattern, 0) {
            Ok(len) if len == pattern.len() => {
                self.ops.push(Op::End);
                Ok(())
            }
            _ => Err(InvalidGlobError::new()),
        }
    }

    fn compile_recursive(
        &mut self,
        pattern: &[u8],
        depth: usize,
    ) -> Result<usize, InvalidGlobError> {
        let mut index = 0;

        macro_rules! next {
            () => {{
                let i = index;
                if i < pattern.len() {
                    index += 1;
                    Some(pattern[i])
                } else {
                    None
                }
            }};
        }

        macro_rules! peek {
            () => {
                if index < pattern.len() {
                    Some(pattern[index])
                } else {
                    None
                }
            };
        }

        loop {
            match next!() {
                None => break,
                Some(b'?') => match self.ops.last_mut() {
                    Some(Op::Skip { len }) => *len += 1,
                    _ => self.ops.push(Op::Skip { len: 1 }),
                },
                Some(b'*') => match peek!() {
                    Some(b'*') => {
                        index += 1;
                        match peek!() {
                            None | Some(b'/') => self.ops.push(Op::ManyComponents),
                            _ => return Err(InvalidGlobError::new()),
                        }
                    }
                    _ => self.ops.push(Op::Many),
                },
                Some(b'[') => {
                    let inverse = match peek!() {
                        Some(b'!') => {
                            index += 1;
                            true
                        }
                        _ => false,
                    };
                    let start = self.bytes.len();
                    loop {
                        let start = match next!() {
                            None => return Err(InvalidGlobError::new()),
                            Some(b']') => break,
                            Some(b) => b,
                        };
                        let end = match peek!() {
                            Some(b'-') => {
                                index += 1;
                                let end = match next!() {
                                    None | Some(b']') => return Err(InvalidGlobError::new()),
                                    Some(b) => b,
                                };
                                if end < start {
                                    return Err(InvalidGlobError::new());
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
                Some(b']') => return Err(InvalidGlobError::new()),
                Some(b'{') => {
                    let fix_index = self.ops.len();
                    self.ops.push(Op::SubPattern { len: 0 });

                    let next_depth = depth + 1;
                    loop {
                        index += self.compile_recursive(&pattern[index..], next_depth)?;
                        self.ops.push(Op::End);

                        match next!() {
                            Some(b'}') => break,
                            Some(b',') => continue,
                            _ => return Err(InvalidGlobError::new()),
                        }
                    }

                    let op_count = self.ops.len();
                    match &mut self.ops[fix_index] {
                        Op::SubPattern { len } => *len = (op_count - fix_index - 1) as _,
                        _ => unreachable!(),
                    }
                }
                Some(b'}') | Some(b',') => {
                    index -= 1;
                    break;
                }
                Some(b) => match self.ops.last_mut() {
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

    pub fn matches(&self, path: &[u8]) -> bool {
        match self.matches_recursive(path, 0) {
            Ok((rest, op_index)) => rest.len() == 0 && op_index + 1 == self.ops.len(),
            Err(_) => false,
        }
    }

    fn matches_recursive<'a>(
        &self,
        mut path: &'a [u8],
        mut op_index: usize,
    ) -> Result<(&'a [u8], usize), NoMatch> {
        'op_loop: loop {
            let op = {
                let i = op_index;
                op_index += 1;
                &self.ops[i]
            };

            match op {
                Op::Slice { from, to } => {
                    let prefix = &self.bytes[(*from as usize)..(*to as usize)];
                    if !path.starts_with(prefix) {
                        return Err(NoMatch);
                    }
                    path = &path[prefix.len()..];
                }
                Op::Skip { len } => {
                    let len = *len as usize;
                    if path.len() < len {
                        return Err(NoMatch);
                    }
                    path = &path[len..];
                }
                Op::Many => {
                    unimplemented!();
                }
                Op::ManyComponents => {
                    unimplemented!();
                }
                Op::AnyWithinRanges { start, count } => {
                    if path.is_empty() {
                        return Err(NoMatch);
                    }
                    let b = path[0];
                    path = &path[1..];
                    for range in self.bytes[(*start as usize)..].chunks(2).take(*count as _) {
                        let start = range[0];
                        let end = range[1];
                        if start <= b && b <= end {
                            continue 'op_loop;
                        }
                    }
                    return Err(NoMatch);
                }
                Op::ExceptWithinRanges { start, count } => {
                    if path.is_empty() {
                        return Err(NoMatch);
                    }
                    let b = path[0];
                    path = &path[1..];
                    for range in self.bytes[(*start as usize)..].chunks(2).take(*count as _) {
                        let start = range[0];
                        let end = range[1];
                        if b < start || end < b {
                            continue 'op_loop;
                        }
                    }
                    return Err(NoMatch);
                }
                //SubPattern { len } => {}
                Op::End => break,
                _ => (),
            }
        }

        Ok((path, op_index))
    }
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
                            return Err(InvalidGlobError::new());
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
                        None => return Err(InvalidGlobError::new()),
                        Some(b']') => return Ok(false),
                        Some(b) => b,
                    };
                    match peek_pattern_byte!() {
                        Some(b'-') => {
                            pattern_index += 1;
                            let end = match next_pattern_byte!() {
                                None | Some(b']') => return Err(InvalidGlobError::new()),
                                Some(b) => b,
                            };
                            if end < start {
                                return Err(InvalidGlobError::new());
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
                    None => return Err(InvalidGlobError::new()),
                }
            }
            Some(b']') => return Err(InvalidGlobError::new()),
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
                    return Err(InvalidGlobError::new());
                }
                inside_group = false;
            }
            Some(b',') => {
                if !inside_group {
                    return Err(InvalidGlobError::new());
                }
                match pattern[pattern_index..].iter().position(|&b| b == b'}') {
                    Some(i) => pattern_index += i + 1,
                    None => return Err(InvalidGlobError::new()),
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
    fn compile() {
        let mut glob = Glob::default();

        assert!(matches!(glob.compile(b""), Ok(())));
        assert!(matches!(glob.compile(b"abc"), Ok(())));
        assert!(matches!(glob.compile(b"a?c"), Ok(())));
        assert!(matches!(glob.compile(b"a[A-Z]c"), Ok(())));
        assert!(matches!(glob.compile(b"a[!0-9]c"), Ok(())));

        assert!(matches!(glob.compile(b"a*c"), Ok(())));
        assert!(matches!(glob.compile(b"a*/"), Ok(())));
        assert!(matches!(glob.compile(b"a*/c"), Ok(())));
        assert!(matches!(glob.compile(b"a*[0-9]/c"), Ok(())));
        assert!(matches!(glob.compile(b"a*bx*cy*d"), Ok(())));

        assert!(matches!(glob.compile(b"a**/"), Ok(())));
        assert!(matches!(glob.compile(b"a**/c"), Ok(())));

        assert!(matches!(glob.compile(b"a{b,c}d"), Ok(())));
        assert!(matches!(glob.compile(b"a*{b,c}d"), Ok(())));
        assert!(matches!(glob.compile(b"a*{b*,c}d"), Ok(())));
    }

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
