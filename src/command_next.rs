use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
pub enum CommandCompileErrorKind {
    UnterminatedQuotedLiteral,
    InvalidFlagName,
    InvalidBindingName,

    ExpectedToken(CommandTokenKind),
    ExpectedStatement,
    ExpectedExpression,
    InvalidBindingDeclarationAtTopLevel,
    LiteralTooBig,
    InvalidLiteralEscaping,
    TooManyBindings,
    UndeclaredBinding,
    UndeclaredCommand,
    NoSuchFlag,
    CouldNotSourceFile,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandCompileError {
    pub kind: CommandCompileErrorKind,
    pub range: Range<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandTokenKind {
    Literal,
    QuotedLiteral,
    Flag,
    Equals,
    Binding,
    OpenCurlyBrackets,
    CloseCurlyBrackets,
    OpenParenthesis,
    CloseParenthesis,
    EndOfCommand,
    EndOfSource,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandToken {
    pub kind: CommandTokenKind,
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
                            kind: CommandCompileErrorKind::InvalidBindingName,
                            range,
                        });
                    } else {
                        return Ok(CommandToken {
                            kind: CommandTokenKind::Binding,
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
                b'\r' | b'\n' => {
                    let from = self.index;
                    while self.index < source_bytes.len()
                        && matches!(source_bytes[self.index], b' ' | b'\t' | b'\r' | b'\n')
                    {
                        self.index += 1;
                    }
                    return Ok(CommandToken {
                        kind: CommandTokenKind::EndOfCommand,
                        range: from..self.index,
                    });
                }
                _ => {
                    let from = self.index;
                    self.index += 1;
                    while self.index < source_bytes.len() {
                        match source_bytes[self.index] {
                            b'{' | b'}' | b'(' | b')' | b' ' | b'\t' | b'\r' | b'\n' => break,
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

#[derive(Default)]
struct LiteralValue {
    pub start: u16,
    pub len: u8,
}

enum Op {
    Return,
    PopToOutput,
    PushLiteral(LiteralValue),
    PushFromStack(u16),
    PopAsFlag(u16),
    CallBuiltinCommand(u16, u8),
    CallMacroCommand(u16, u8),
    CallRequestCommand(u16, u8),
}

#[derive(Clone, Copy)]
pub enum CommandSource {
    Builtin(usize),
    Macro(usize),
    Request(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    Commands,
    Buffers,
    Files,
    Custom(&'static [&'static str]),
}

pub struct BuiltinCommand {
    pub name: &'static str,
    pub alias: &'static str,
    pub hidden: bool,
    pub completions: &'static [CompletionSource],
    pub flags: &'static [&'static str],
    //pub func: CommandFn,
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

pub struct CommandManager {
    builtin_commands: &'static [BuiltinCommand],
    macro_commands: MacroCommandCollection,
}

struct ByteCodeChunk {
    ops: Vec<Op>,
    texts: String,
}
impl ByteCodeChunk {
    pub fn emit(&mut self, op: Op) {
        self.ops.push(op);
    }

    pub fn add_literal(
        &mut self,
        text: &str,
        range: Range<usize>,
    ) -> Result<LiteralValue, CommandCompileError> {
        if text.len() > u8::MAX as _ {
            return Err(CommandCompileError {
                kind: CommandCompileErrorKind::LiteralTooBig,
                range,
            });
        }

        let start = self.texts.len() as _;
        let len = text.len() as _;
        self.texts.push_str(text);
        Ok(LiteralValue { start, len })
    }

    pub fn add_escaped_literal(
        &mut self,
        mut text: &str,
        range: Range<usize>,
    ) -> Result<LiteralValue, CommandCompileError> {
        if text.len() > u8::MAX as _ {
            return Err(CommandCompileError {
                kind: CommandCompileErrorKind::LiteralTooBig,
                range,
            });
        }

        let start = self.texts.len();
        while let Some(i) = text.find('\\') {
            self.texts.push_str(&text[..i]);
            text = &text[i + 1..];
            match text.as_bytes() {
                &[b'\\', ..] => self.texts.push('\\'),
                &[b'\'', ..] => self.texts.push('\''),
                &[b'\"', ..] => self.texts.push('\"'),
                &[b'\n', ..] => self.texts.push('\n'),
                &[b'\r', ..] => self.texts.push('\r'),
                &[b'\t', ..] => self.texts.push('\t'),
                &[b'\0', ..] => self.texts.push('\0'),
                _ => {
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::InvalidLiteralEscaping,
                        range,
                    })
                }
            }
        }
        self.texts.push_str(text);

        let len = (self.texts.len() - start) as _;
        let start = start as _;
        Ok(LiteralValue { start, len })
    }
}

struct Binding {
    range: Range<u32>,
}

struct Compiler<'source, 'state> {
    tokenizer: CommandTokenizer<'source>,
    path: Option<&'state Path>,
    pub previous_token: CommandToken,
    pub bindings: &'state mut Vec<Binding>,
    bindings_previous_len: usize,
    builtin_commands: &'static [BuiltinCommand],
    macro_commands: &'state MacroCommandCollection,
}
impl<'source, 'state> Compiler<'source, 'state> {
    pub fn new(
        source: &'source str,
        path: Option<&'state Path>,
        bindings: &'state mut Vec<Binding>,
        builtin_commands: &'static [BuiltinCommand],
        macro_commands: &'state MacroCommandCollection,
    ) -> Result<Self, CommandCompileError> {
        let mut tokenizer = CommandTokenizer::new(source);
        let previous_token = tokenizer.next()?;
        let bindings_previous_len = bindings.len();
        Ok(Self {
            tokenizer,
            path,
            previous_token,
            bindings,
            bindings_previous_len,
            builtin_commands,
            macro_commands,
        })
    }

    pub fn previous_token_str(&self) -> &'source str {
        &self.tokenizer.source[self.previous_token.range.clone()]
    }

    pub fn next_token(&mut self) -> Result<(), CommandCompileError> {
        let token = self.tokenizer.next()?;
        self.previous_token = token;
        Ok(())
    }

    pub fn consume_token(&mut self, kind: CommandTokenKind) -> Result<(), CommandCompileError> {
        if self.previous_token.kind == kind {
            self.next_token()
        } else {
            Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedToken(kind),
                range: self.previous_token.range.clone(),
            })
        }
    }

    pub fn find_command(&self, name: &str) -> Option<CommandSource> {
        if let Some(i) = self.macro_commands.commands.iter().position(|c| {
            let range = c.name_range.start as usize..c.name_range.end as usize;
            &self.macro_commands.names[range] == name
        }) {
            return Some(CommandSource::Macro(i));
        }

        /*
        if let Some(i) = self.request_commands.iter().position(|c| c.name == name) {
            return Some(CommandSource::Request(i));
        }
        */

        if let Some(i) = self
            .builtin_commands
            .iter()
            .position(|c| c.alias == name || c.name == name)
        {
            return Some(CommandSource::Builtin(i));
        }

        None
    }

    pub fn declare_binding(&mut self, range: Range<usize>) -> Result<(), CommandCompileError> {
        if self.bindings.len() >= u16::MAX as _ {
            Err(CommandCompileError {
                kind: CommandCompileErrorKind::TooManyBindings,
                range,
            })
        } else {
            let range = range.start as _..range.end as _;
            self.bindings.push(Binding { range });
            Ok(())
        }
    }

    pub fn find_binding_stack_index(&self, name: &str) -> Option<u16> {
        let source = self.tokenizer.source;
        self.bindings
            .iter()
            .rposition(|b| &source[b.range.start as usize..b.range.end as usize] == name)
            .map(|i| i as _)
    }

    pub fn compile_into(&mut self, chunk: &mut ByteCodeChunk) -> Result<(), CommandCompileError> {
        chunk.ops.clear();
        match compile(self, chunk) {
            Ok(()) => (),
            Err(error) => {
                chunk.ops.clear();
                return Err(error);
            }
        }
        Ok(())
    }
}
impl<'source, 'state> Drop for Compiler<'source, 'state> {
    fn drop(&mut self) {
        self.bindings.truncate(self.bindings_previous_len);
    }
}

fn compile(compiler: &mut Compiler, chunk: &mut ByteCodeChunk) -> Result<(), CommandCompileError> {
    fn parse_top_level(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        if let CommandTokenKind::Literal = compiler.previous_token.kind {
            let previous_str = compiler.previous_token_str();
            compiler.next_token()?;
            match previous_str {
                "source" => return parse_source(compiler, chunk),
                "macro" => return parse_macro(compiler, chunk),
                _ => (),
            }
        }

        parse_statement(compiler, chunk, true)
    }

    fn parse_source(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        compiler.next_token()?;
        compiler.consume_token(CommandTokenKind::QuotedLiteral)?;

        let path = Path::new(compiler.previous_token_str());
        let path = if path.is_absolute() {
            path.into()
        } else {
            let mut buf = PathBuf::new();
            if let Some(path) = compiler.path {
                buf.push(path);
            }
            buf.push(path);
            buf
        };

        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(_) => {
                return Err(CommandCompileError {
                    kind: CommandCompileErrorKind::CouldNotSourceFile,
                    range: compiler.previous_token.range.clone(),
                })
            }
        };

        let mut compiler = Compiler::new(
            &source,
            Some(&path),
            compiler.bindings,
            compiler.builtin_commands,
            compiler.macro_commands,
        )?;
        compiler.compile_into(chunk)?;
        Ok(())
    }

    fn parse_macro(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        compiler.next_token()?;
        compiler.consume_token(CommandTokenKind::Literal)?;
        let name = compiler.previous_token_str();
        todo!()
    }

    fn parse_statement(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
        is_top_level: bool,
    ) -> Result<(), CommandCompileError> {
        match compiler.previous_token.kind {
            CommandTokenKind::Literal | CommandTokenKind::OpenParenthesis => {
                parse_command_call(compiler, chunk)?;
                chunk.emit(Op::PopToOutput);
                Ok(())
            }
            CommandTokenKind::QuotedLiteral => {
                parse_expression(compiler, chunk)?;
                chunk.emit(Op::PopToOutput);
                Ok(())
            }
            CommandTokenKind::Binding => {
                if is_top_level {
                    return Err(CommandCompileError {
                        kind: CommandCompileErrorKind::InvalidBindingDeclarationAtTopLevel,
                        range: compiler.previous_token.range.clone(),
                    });
                }

                compiler.declare_binding(compiler.previous_token.range.clone())?;

                compiler.next_token()?;
                compiler.consume_token(CommandTokenKind::Equals)?;

                parse_expression(compiler, chunk)?;
                Ok(())
            }
            CommandTokenKind::EndOfCommand => Ok(()),
            _ => Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedStatement,
                range: compiler.previous_token.range.clone(),
            }),
        }
    }

    fn parse_command_call(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        let end_token_kind = match compiler.previous_token.kind {
            CommandTokenKind::Literal => CommandTokenKind::Literal,
            CommandTokenKind::OpenParenthesis => {
                compiler.consume_token(CommandTokenKind::Literal)?;
                CommandTokenKind::CloseParenthesis
            }
            _ => {
                return Err(CommandCompileError {
                    kind: CommandCompileErrorKind::ExpectedToken(CommandTokenKind::Literal),
                    range: compiler.previous_token.range.clone(),
                })
            }
        };

        let range_start = compiler.previous_token.range.start;
        let command_name = compiler.previous_token_str();
        compiler.next_token()?;

        let command_source = match compiler.find_command(command_name) {
            Some(source) => source,
            None => {
                return Err(CommandCompileError {
                    kind: CommandCompileErrorKind::UndeclaredCommand,
                    range: compiler.previous_token.range.clone(),
                })
            }
        };

        loop {
            fn find_flag_index_from_previous_token(
                compiler: &Compiler,
                command_source: CommandSource,
            ) -> Result<usize, CommandCompileError> {
                if let CommandSource::Builtin(i) = command_source {
                    let flag_name = compiler.previous_token_str();
                    for (i, &flag) in compiler.builtin_commands[i].flags.iter().enumerate() {
                        if flag == flag_name {
                            return Ok(i);
                        }
                    }
                }

                Err(CommandCompileError {
                    kind: CommandCompileErrorKind::NoSuchFlag,
                    range: compiler.previous_token.range.clone(),
                })
            }

            if compiler.previous_token.kind == CommandTokenKind::Flag {
                let flag_index =
                    find_flag_index_from_previous_token(compiler, command_source)? as _;
                compiler.next_token()?;

                match compiler.previous_token.kind {
                    CommandTokenKind::Equals => {
                        compiler.next_token()?;
                        parse_expression(compiler, chunk)?;
                    }
                    _ => chunk.emit(Op::PushLiteral(LiteralValue::default())),
                }

                chunk.emit(Op::PopAsFlag(flag_index));
            } else if compiler.previous_token.kind == end_token_kind {
                compiler.next_token()?;
                break;
            } else {
                parse_expression(compiler, chunk)?;
            }
        }

        todo!();
    }

    fn parse_expression(
        compiler: &mut Compiler,
        chunk: &mut ByteCodeChunk,
    ) -> Result<(), CommandCompileError> {
        let range_start = compiler.previous_token.range.start;
        match compiler.previous_token.kind {
            CommandTokenKind::Literal => {
                let literal = compiler.previous_token_str();
                let literal = chunk.add_literal(literal, compiler.previous_token.range.clone())?;
                chunk.emit(Op::PushLiteral(literal));
                compiler.next_token()?;
                Ok(())
            }
            CommandTokenKind::QuotedLiteral => {
                let literal = compiler.previous_token_str();
                let literal = &literal[1..];
                let literal = &literal[..literal.len() - 1];
                let literal =
                    chunk.add_escaped_literal(literal, compiler.previous_token.range.clone())?;
                chunk.emit(Op::PushLiteral(literal));
                compiler.next_token()?;
                Ok(())
            }
            CommandTokenKind::OpenParenthesis => {
                parse_command_call(compiler, chunk)?;
                compiler.consume_token(CommandTokenKind::CloseParenthesis)?;
                Ok(())
            }
            CommandTokenKind::Binding => {
                let binding_name = compiler.previous_token_str();
                match compiler.find_binding_stack_index(binding_name) {
                    Some(index) => {
                        compiler.next_token()?;
                        chunk.emit(Op::PushFromStack(index));
                        Ok(())
                    }
                    None => Err(CommandCompileError {
                        kind: CommandCompileErrorKind::UndeclaredBinding,
                        range: compiler.previous_token.range.clone(),
                    }),
                }
            }
            CommandTokenKind::EndOfCommand => Ok(()),
            _ => Err(CommandCompileError {
                kind: CommandCompileErrorKind::ExpectedExpression,
                range: range_start..compiler.previous_token.range.end,
            }),
        }
    }

    while compiler.previous_token.kind != CommandTokenKind::EndOfSource {
        parse_top_level(compiler, chunk)?;
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
                (Binding, "$binding"),
                (Flag, "-flag"),
                (Equals, "="),
                (Literal, "value"),
                (Equals, "="),
                (Literal, "not-flag"),
            ],
            collect("cmd $binding -flag=value = not-flag")
        );
        assert_eq!(
            vec![
                (Literal, "cmd0"),
                (Literal, "cmd1"),
                (EndOfCommand, "\r\n\n \t \n  "),
                (Literal, "cmd2"),
            ],
            collect("cmd0 cmd1 \t\r\n\n \t \n  cmd2")
        );
    }
}

