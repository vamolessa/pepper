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
    EndOfSource,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandToken {
    pub kind: Result<CommandTokenKind, CommandTokenError>,
    pub range: Range<usize>,
}

pub struct CommandTokenIter<'a> {
    bytes: &'a [u8],
    index: usize,
}
impl<'a> CommandTokenIter<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            index: 0,
        }
    }

    pub fn get_token_bytes(&self, range: Range<usize>) -> &'a [u8] {
        &self.bytes[range]
    }
}
impl<'a> Iterator for CommandTokenIter<'a> {
    type Item = CommandToken;
    fn next(&mut self) -> Option<Self::Item> {
        fn error(
            iter: &mut CommandTokenIter,
            error: CommandTokenError,
            range: Range<usize>,
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
            Some(CommandToken {
                kind: Ok(kind),
                range: from..iter.index,
            })
        }

        loop {
            loop {
                if self.index == self.bytes.len() {
                    self.index += 1;
                    return Some(CommandToken {
                        kind: Ok(CommandTokenKind::EndOfSource),
                        range: self.bytes.len()..self.bytes.len(),
                    });
                } else if self.index > self.bytes.len() {
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
                                from..self.bytes.len(),
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
                    break Some(CommandToken {
                        kind: Ok(CommandTokenKind::QuotedLiteral),
                        range: from..self.index,
                    });
                }
                b'-' => {
                    let from = self.index;
                    self.index += 1;
                    consume_identifier(self);
                    let range = from..self.index;
                    if range.start + 1 == range.end {
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
                    let range = from..self.index;
                    if range.start + 1 == range.end {
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
                            break Some(error(
                                self,
                                CommandTokenError::InvalidEscaping,
                                from..self.index,
                            ))
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
                    break Some(CommandToken {
                        kind: Ok(CommandTokenKind::Literal),
                        range: from..self.index,
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

pub enum CommandCompileError {
    UnexpectedEndOfTokens,
    TokenError(CommandTokenError),
}
impl From<CommandTokenError> for CommandCompileError {
    fn from(error: CommandTokenError) -> Self {
        Self::TokenError(error)
    }
}

struct ByteCodeChunk {
    ops: Vec<Op>,
    texts: String,
}

struct Parser<'source> {
    tokens: CommandTokenIter<'source>,
    pub previous_kind: CommandTokenKind,
    pub previous_range: Range<usize>,
}
impl<'source> Parser<'source>  {
    pub fn new(source: &'source str) -> Self {
        Self {
            tokens: CommandTokenIter::new(source),
            previous_kind: CommandTokenKind::Literal,
            previous_range: 0..0,
        }
    }

    pub fn next(&mut self) -> Result<(), CommandCompileError> {
        match self.tokens.next() {
            Some(token) => match token.kind {
                Ok(kind) => {
                    self.previous_kind = kind;
                    self.previous_range = token.range;
                    Ok(())
                }
                Err(error) => Err(error.into()),
            },
            None => Err(CommandCompileError::UnexpectedEndOfTokens),
        }
    }
}

fn compile(source: &str, chunk: &mut ByteCodeChunk) -> Result<(), CommandCompileError> {
    chunk.ops.clear();
    let mut parser = Parser::new(source);
    parser.next()?;
    match compile_into(&mut parser, chunk) {
        Ok(()) => (),
        Err(error) => {
            chunk.ops.clear();
            return Err(error);
        }
    }
    Ok(())
}

fn compile_into(parser: &mut Parser, chunk: &mut ByteCodeChunk) -> Result<(), CommandCompileError> {
    fn statement(
        parser: &mut Parser,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        parser.next()?;
        Ok(())
    }

    loop {
        statement(parser, chunk)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_tokenizer() {
        fn collect<'a>(source: &'a str) -> Vec<(CommandTokenKind, &'a str)> {
            CommandTokenIter::new(source)
                .map(|t| (t.kind.unwrap(), &source[t.range]))
                .collect()
        }

        use CommandTokenKind::*;
        assert_eq!(vec![(EndOfSource, "")], collect(""));
        assert_eq!(vec![(EndOfSource, "")], collect("  "));
        assert_eq!(
            vec![(Literal, "command"), (EndOfSource, "")],
            collect("command")
        );
        assert_eq!(
            vec![(QuotedLiteral, "'text'"), (EndOfSource, "")],
            collect("'text'")
        );
        assert_eq!(
            vec![
                (Literal, "cmd"),
                (OpenParenthesis, "("),
                (Literal, "subcmd"),
                (CloseParenthesis, ")"),
                (EndOfSource, ""),
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
                (Literal, "not-flag"),
                (EndOfSource, ""),
            ],
            collect("cmd $var -flag=value = not-flag")
        );
        assert_eq!(
            vec![
                (Literal, "cmd0"),
                (EndOfStatement, ";"),
                (Literal, "cmd1"),
                (EndOfStatement, "\r"),
                (Literal, "cmd2"),
                (EndOfSource, ""),
            ],
            collect("cmd0;cmd1\r\n\ncmd2")
        );
        assert_eq!(
            vec![
                (Literal, "cmd0"),
                (Literal, "v0"),
                (Literal, "v1"),
                (EndOfSource, ""),
            ],
            collect("cmd0 v0 \\\n \\\r\n v1")
        );
    }
}

