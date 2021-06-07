use std::ops::Range;

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
    pub kind: CommandTokenKind,
    pub range: Range<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandCompileErrorKind {
    UnterminatedQuotedLiteral,
    InvalidFlagName,
    InvalidVariableName,
    InvalidEscaping,

    ExpectedTokenKind(CommandTokenKind),
    ExpectedStatement,
    ExpectedExpression,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandCompileError {
    pub kind: CommandCompileErrorKind,
    pub range: Range<usize>,
}

pub struct CommandTokenizer<'a> {
    pub source: &'a str,
    index: usize,
}
impl<'a> CommandTokenizer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source, index: 0 }
    }

    pub fn next(&mut self) -> Result<CommandToken, CommandCompileError> {
        fn consume_identifier(iter: &mut CommandTokenizer) {
            let source = &iter.source[iter.index..];
            let len = match source
                .find(|c| !matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-'))
            {
                Some(len) => len,
                None => source.len(),
            };
            iter.index += len;
        }
        fn single_byte_token(iter: &mut CommandTokenizer, kind: CommandTokenKind) -> CommandToken {
            let from = iter.index;
            iter.index += 1;
            CommandToken {
                kind,
                range: from..iter.index,
            }
        }

        let source_bytes = self.source.as_bytes();

        loop {
            loop {
                if self.index >= source_bytes.len() {
                    return Ok(CommandToken {
                        kind: CommandTokenKind::EndOfSource,
                        range: source_bytes.len()..source_bytes.len(),
                    });
                }
                if matches!(source_bytes[self.index], b' ' | b'\t') {
                    self.index += 1;
                } else {
                    break;
                }
            }

            match source_bytes[self.index] {
                delim @ b'"' | delim @ b'\'' => {
                    let from = self.index;
                    self.index += 1;
                    loop {
                        if self.index >= source_bytes.len() {
                            return Err(CommandCompileError {
                                kind: CommandCompileErrorKind::UnterminatedQuotedLiteral,
                                range: from..source_bytes.len(),
                            });
                        }

                        let byte = source_bytes[self.index];
                        if byte == b'\\' {
                            self.index += 2;
                        } else {
                            self.index += 1;
                            if byte == delim {
                                break;
                            }
                        }
                    }
                    return Ok(CommandToken {
                        kind: CommandTokenKind::QuotedLiteral,
                        range: from..self.index,
                    });
                }
                b'-' => {
                    let from = self.index;
                    self.index += 1;
                    consume_identifier(self);
                    let range = from..self.index;
                    if range.start + 1 == range.end {
                        return Err(CommandCompileError {
                            kind: CommandCompileErrorKind::InvalidFlagName,
                            range,
                        });
                    } else {
                        return Ok(CommandToken {
                            kind: CommandTokenKind::Flag,
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
                        return Err(CommandCompileError {
                            kind: CommandCompileErrorKind::InvalidVariableName,
                            range,
                        });
                    } else {
                        return Ok(CommandToken {
                            kind: CommandTokenKind::Variable,
                            range,
                        });
                    }
                }
                b'=' => return Ok(single_byte_token(self, CommandTokenKind::Equals)),
                b'{' => return Ok(single_byte_token(self, CommandTokenKind::OpenCurlyBrackets)),
                b'}' => {
                    return Ok(single_byte_token(
                        self,
                        CommandTokenKind::CloseCurlyBrackets,
                    ))
                }
                b'(' => return Ok(single_byte_token(self, CommandTokenKind::OpenParenthesis)),
                b')' => return Ok(single_byte_token(self, CommandTokenKind::CloseParenthesis)),
                b'\\' => {
                    let from = self.index;
                    self.index += 1;
                    match &source_bytes[self.index..] {
                        &[b'\n', ..] => self.index += 1,
                        &[b'\r', b'\n', ..] => self.index += 2,
                        _ => {
                            return Err(CommandCompileError {
                                kind: CommandCompileErrorKind::InvalidEscaping,
                                range: from..self.index,
                            });
                        }
                    }
                }
                b'\r' | b'\n' | b';' => {
                    let token = single_byte_token(self, CommandTokenKind::EndOfStatement);
                    while self.index < source_bytes.len()
                        && matches!(
                            source_bytes[self.index],
                            b' ' | b'\t' | b'\r' | b'\n' | b';'
                        )
                    {
                        self.index += 1;
                    }
                    return Ok(token);
                }
                _ => {
                    let from = self.index;
                    self.index += 1;
                    while self.index < source_bytes.len() {
                        match source_bytes[self.index] {
                            b'{' | b'}' | b'(' | b')' | b' ' | b'\t' | b'\r' | b'\n' | b';' => {
                                break
                            }
                            _ => self.index += 1,
                        }
                    }
                    return Ok(CommandToken {
                        kind: CommandTokenKind::Literal,
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

struct ByteCodeChunk {
    ops: Vec<Op>,
    texts: String,
}

struct Parser<'source> {
    tokenizer: CommandTokenizer<'source>,
    pub previous: CommandToken,
}
impl<'source> Parser<'source> {
    pub fn new(source: &'source str) -> Result<Self, CommandCompileError> {
        let mut tokenizer = CommandTokenizer::new(source);
        let previous = tokenizer.next()?;
        Ok(Self {
            tokenizer,
            previous,
        })
    }

    pub fn previous_str(&self) -> &'source str {
        &self.tokenizer.source[self.previous.range.clone()]
    }

    pub fn next(&mut self) -> Result<(), CommandCompileError> {
        let token = self.tokenizer.next()?;
        self.previous = token;
        Ok(())
    }

    pub fn consume(&mut self, kind: CommandTokenKind) -> Result<(), CommandCompileError> {
        if self.previous.kind == kind {
            self.next()
        } else {
            Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedTokenKind(kind),
                range: self.previous.range.clone(),
            })
        }
    }

    pub fn matches(&mut self, kind: CommandTokenKind) -> Result<bool, CommandCompileError> {
        let matches = self.previous.kind == kind;
        self.next()?;
        Ok(matches)
    }
}

fn compile(source: &str, chunk: &mut ByteCodeChunk) -> Result<(), CommandCompileError> {
    chunk.ops.clear();
    let mut parser = Parser::new(source)?;
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
    fn parse_top_level(
        parser: &mut Parser,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        if let CommandTokenKind::Literal = parser.previous.kind {
            let previous_str = parser.previous_str();
            parser.next()?;
            match previous_str {
                "source" => return parse_source(parser, chunk),
                "macro" => return parse_macro(parser, chunk),
                _ => (),
            }
        }

        parse_statement(parser, chunk)
    }

    fn parse_source(
        parser: &mut Parser,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        todo!()
    }

    fn parse_macro(
        parser: &mut Parser,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        todo!()
    }

    fn parse_statement(
        parser: &mut Parser,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        match parser.previous.kind {
            CommandTokenKind::Literal => parse_command_call(parser, chunk),
            CommandTokenKind::Variable => {
                let variable_name = parser.previous_str();
                parser.next()?;
                parser.consume(CommandTokenKind::Equals)?;
                parse_expression(parser, chunk)?;

                todo!();
            }
            _ => Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedStatement,
                range: parser.previous.range.clone(),
            }),
        }
    }

    fn parse_command_call(
        parser: &mut Parser,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        todo!();
    }

    fn parse_expression(
        parser: &mut Parser,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        let range_start = parser.previous.range.start;
        match parser.previous.kind {
            CommandTokenKind::Literal | CommandTokenKind::QuotedLiteral => {
                todo!();
            }
            _ => Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedExpression,
                range: range_start..parser.previous.range.end,
            }),
        }
    }

    while parser.previous.kind != CommandTokenKind::EndOfSource {
        parse_top_level(parser, chunk)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_tokenizer() {
        fn collect<'a>(source: &'a str) -> Vec<(CommandTokenKind, &'a str)> {
            let mut tokenizer = CommandTokenizer::new(source);
            let mut tokens = Vec::new();
            loop {
                let token = tokenizer.next().unwrap();
                match token.kind {
                    CommandTokenKind::EndOfSource => break,
                    _ => tokens.push((token.kind, &source[token.range])),
                }
            }
            tokens
        }

        use CommandTokenKind::*;
        assert!(collect("").is_empty());
        assert!(collect("  ").is_empty());
        assert_eq!(vec![(Literal, "command")], collect("command"));
        assert_eq!(vec![(QuotedLiteral, "'text'")], collect("'text'"));
        assert_eq!(
            vec![
                (Literal, "cmd"),
                (OpenParenthesis, "("),
                (Literal, "subcmd"),
                (CloseParenthesis, ")"),
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
            ],
            collect("cmd0;cmd1\r\n\ncmd2")
        );
        assert_eq!(
            vec![(Literal, "cmd0"), (Literal, "v0"), (Literal, "v1")],
            collect("cmd0 v0 \\\n \\\r\n v1")
        );
    }
}

