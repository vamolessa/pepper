use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub enum CommandTokenError {
    UnterminatedQuotedLiteral,
    InvalidFlagName,
    InvalidVariableName,
    InvalidEscaping,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandTokenKind {
    Literal,
    QuotedLiteral,
    Flag,
    Equals,
    Variable,
    OpenCurlyBrackets,
    CloseCurlyBrackets,
    OpenParenthesis,
    CloseParenthesis,
    EndOfStatement,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandTokenRange {
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandToken {
    pub kind: Result<CommandTokenKind, CommandTokenError>,
    pub range: CommandTokenRange,
}

pub struct CommandTokenIter<'a> {
    bytes: &'a [u8],
    index: usize,
}
impl<'a> CommandTokenIter<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            index: 0,
        }
    }
}
impl<'a> Iterator for CommandTokenIter<'a> {
    type Item = CommandToken;
    fn next(&mut self) -> Option<Self::Item> {
        fn error(
            iter: &mut CommandTokenIter,
            error: CommandTokenError,
            range: CommandTokenRange,
        ) -> CommandToken {
            iter.index = iter.bytes.len();
            CommandToken {
                kind: Err(error),
                range,
            }
        }
        fn consume_identifier(iter: &mut CommandTokenIter) {
            let bytes = &iter.bytes[iter.index..];
            let len = match bytes
                .iter()
                .position(|b| !matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-'))
            {
                Some(len) => len,
                None => bytes.len(),
            };
            iter.index += len;
        }
        fn single_byte_token(
            iter: &mut CommandTokenIter,
            kind: CommandTokenKind,
        ) -> Option<CommandToken> {
            let from = iter.index;
            iter.index += 1;
            let range = CommandTokenRange {
                from,
                to: iter.index,
            };
            Some(CommandToken {
                kind: Ok(kind),
                range,
            })
        }

        loop {
            loop {
                if self.index >= self.bytes.len() {
                    return None;
                }
                if matches!(self.bytes[self.index], b' ' | b'\t') {
                    self.index += 1;
                } else {
                    break;
                }
            }

            match self.bytes[self.index] {
                delim @ b'"' | delim @ b'\'' => {
                    let from = self.index;
                    self.index += 1;
                    loop {
                        if self.index >= self.bytes.len() {
                            return Some(error(
                                self,
                                CommandTokenError::UnterminatedQuotedLiteral,
                                CommandTokenRange {
                                    from,
                                    to: self.bytes.len(),
                                },
                            ));
                        }

                        let byte = self.bytes[self.index];
                        if byte == b'\\' {
                            self.index += 2;
                        } else {
                            self.index += 1;
                            if byte == delim {
                                break;
                            }
                        }
                    }
                    let to = self.index;
                    let range = CommandTokenRange { from, to };
                    break Some(CommandToken {
                        kind: Ok(CommandTokenKind::QuotedLiteral),
                        range,
                    });
                }
                b'-' => {
                    let from = self.index;
                    self.index += 1;
                    consume_identifier(self);
                    let to = self.index;
                    let range = CommandTokenRange { from, to };
                    if range.from + 1 == range.to {
                        break Some(error(self, CommandTokenError::InvalidFlagName, range));
                    } else {
                        break Some(CommandToken {
                            kind: Ok(CommandTokenKind::Flag),
                            range,
                        });
                    }
                }
                b'$' => {
                    let from = self.index;
                    self.index += 1;
                    consume_identifier(self);
                    let to = self.index;
                    let range = CommandTokenRange { from, to };
                    if range.from + 1 == range.to {
                        break Some(error(self, CommandTokenError::InvalidVariableName, range));
                    } else {
                        break Some(CommandToken {
                            kind: Ok(CommandTokenKind::Variable),
                            range,
                        });
                    }
                }
                b'=' => break single_byte_token(self, CommandTokenKind::Equals),
                b'{' => break single_byte_token(self, CommandTokenKind::OpenCurlyBrackets),
                b'}' => break single_byte_token(self, CommandTokenKind::CloseCurlyBrackets),
                b'(' => break single_byte_token(self, CommandTokenKind::OpenParenthesis),
                b')' => break single_byte_token(self, CommandTokenKind::CloseParenthesis),
                b'\\' => {
                    let from = self.index;
                    self.index += 1;
                    match &self.bytes[self.index..] {
                        &[b'\n', ..] => self.index += 1,
                        &[b'\r', b'\n', ..] => self.index += 2,
                        _ => {
                            let to = self.index;
                            let range = CommandTokenRange { from, to };
                            break Some(error(self, CommandTokenError::InvalidEscaping, range));
                        }
                    }
                }
                b'\r' | b'\n' | b';' => {
                    let token = single_byte_token(self, CommandTokenKind::EndOfStatement);
                    while self.index < self.bytes.len()
                        && matches!(self.bytes[self.index], b' ' | b'\t' | b'\r' | b'\n' | b';')
                    {
                        self.index += 1;
                    }
                    break token;
                }
                _ => {
                    let from = self.index;
                    self.index += 1;
                    while self.index < self.bytes.len() {
                        match self.bytes[self.index] {
                            b'{' | b'}' | b'(' | b')' | b' ' | b'\t' | b'\r' | b'\n' | b';' => {
                                break
                            }
                            _ => self.index += 1,
                        }
                    }
                    let to = self.index;
                    let range = CommandTokenRange { from, to };
                    break Some(CommandToken {
                        kind: Ok(CommandTokenKind::Literal),
                        range,
                    });
                }
            }
        }
    }
}

enum Op {
    Return,
    BuiltinCommand(usize),
    MacroCommand(usize),
    RequestCommand(usize),
}

struct MacroCommand {
    name_range: Range<u32>,
    op_start_index: u32,
    params_len: u8,
}

struct MacroCommandCollection {
    names: String,
    chunk: ByteCodeChunk,
    commands: Vec<MacroCommand>,
}

struct ByteCodeChunk {
    ops: Vec<Op>,
    texts: String,
}

fn compile(commands: &str, macros: &mut MacroCommandCollection, chunk: &mut ByteCodeChunk) {
    chunk.ops.clear();
    todo!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_tokenizer() {
        fn collect<'a>(text: &'a str) -> Vec<(CommandTokenKind, &'a str)> {
            CommandTokenIter::new(text)
                .map(|t| (t.kind.unwrap(), &text[t.range.from..t.range.to]))
                .collect()
        }

        use CommandTokenKind::*;
        assert!(collect("").is_empty());
        assert_eq!(vec![(Literal, "command")], collect("command"));
        assert_eq!(vec![(QuotedLiteral, "'text'")], collect("'text'"));
        assert_eq!(
            vec![
                (Literal, "cmd"),
                (OpenParenthesis, "("),
                (Literal, "subcmd"),
                (CloseParenthesis, ")")
            ],
            collect("cmd (subcmd)")
        );
        assert_eq!(
            vec![
                (Literal, "cmd"),
                (Variable, "$var"),
                (Flag, "-flag"),
                (Equals, "="),
                (Literal, "value"),
                (Equals, "="),
                (Literal, "not-flag")
            ],
            collect("cmd $var -flag=value = not-flag")
        );
        assert_eq!(
            vec![
                (Literal, "cmd0"),
                (EndOfStatement, ";"),
                (Literal, "cmd1"),
                (EndOfStatement, "\r"),
                (Literal, "cmd2")
            ],
            collect("cmd0;cmd1\r\n\ncmd2")
        );
        assert_eq!(
            vec![(Literal, "cmd0"), (Literal, "v0"), (Literal, "v1")],
            collect("cmd0 v0 \\\n \\\r\n v1")
        );
    }
}

