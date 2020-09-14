use std::num::NonZeroUsize;

use crate::{
    pattern::Pattern,
    script::{ScriptEngineRef, ScriptResult, ScriptValue},
    syntax::{Syntax, SyntaxCollection, TokenKind},
    theme::{pico8_theme, Theme},
};

#[derive(Debug, Clone)]
pub struct ConfigValues {
    pub tab_size: NonZeroUsize,

    pub visual_empty: char,
    pub visual_space: char,
    pub visual_tab_first: char,
    pub visual_tab_repeat: char,

    pub picker_max_height: NonZeroUsize,
}

impl ConfigValues {
    pub fn get_from_name<'script>(
        &self,
        engine: ScriptEngineRef<'script>,
        name: &str,
    ) -> ScriptResult<ScriptValue<'script>> {
        macro_rules! char_to_string {
            ($c:expr) => {{
                let mut buf = [0; std::mem::size_of::<char>()];
                ScriptValue::String(engine.create_string($c.encode_utf8(&mut buf).as_bytes())?)
            }};
        }

        match name {
            stringify!(tab_size) => Ok(ScriptValue::Integer(self.tab_size.get() as _)),
            stringify!(visual_empty) => Ok(char_to_string!(self.visual_empty)),
            stringify!(visual_space) => Ok(char_to_string!(self.visual_space)),
            stringify!(visual_tab_first) => Ok(char_to_string!(self.visual_tab_first)),
            stringify!(visual_tab_repeat) => Ok(char_to_string!(self.visual_tab_repeat)),
            stringify!(picker_max_height) => {
                Ok(ScriptValue::Integer(self.picker_max_height.get() as _))
            }
            _ => Ok(ScriptValue::Nil),
        }
    }

    pub fn set_from_name(&mut self, name: &str, value: ScriptValue) {
        macro_rules! try_integer {
            ($value:expr) => {{
                let integer = match $value {
                    ScriptValue::Integer(i) if i > 0 => i,
                    _ => return,
                };
                NonZeroUsize::new(integer as _).unwrap()
            }};
        }
        macro_rules! try_char {
            ($value:expr) => {{
                match $value {
                    ScriptValue::String(s) => {
                        let s = match s.to_str() {
                            Ok(s) => s,
                            Err(_) => return,
                        };
                        match s.parse() {
                            Ok(c) => c,
                            Err(_) => return,
                        }
                    }
                    _ => return,
                }
            }};
        }

        match name {
            stringify!(tab_size) => self.tab_size = try_integer!(value),
            stringify!(visual_empty) => self.visual_empty = try_char!(value),
            stringify!(visual_space) => self.visual_space = try_char!(value),
            stringify!(visual_tab_first) => self.visual_tab_first = try_char!(value),
            stringify!(visual_tab_repeat) => self.visual_tab_repeat = try_char!(value),
            stringify!(picker_max_height) => self.picker_max_height = try_integer!(value),
            _ => (),
        }
    }
}

impl Default for ConfigValues {
    fn default() -> Self {
        Self {
            tab_size: NonZeroUsize::new(4).unwrap(),

            visual_empty: '~',
            visual_space: '.',
            visual_tab_first: '|',
            visual_tab_repeat: ' ',

            picker_max_height: NonZeroUsize::new(8).unwrap(),
        }
    }
}

pub struct Config {
    pub values: ConfigValues,
    pub theme: Theme,
    pub syntaxes: SyntaxCollection,
}

impl Default for Config {
    fn default() -> Self {
        let mut syntaxes = SyntaxCollection::new();
        set_rust_syntax(syntaxes.get_by_extension("rs"));
        set_lua_syntax(syntaxes.get_by_extension("lua"));

        Self {
            values: ConfigValues::default(),
            theme: pico8_theme(),
            syntaxes,
        }
    }
}

fn set_rust_syntax(syntax: &mut Syntax) {
    for keyword in &[
        "fn", "let", "if", "while", "for", "return", "mod", "use", "as", "in", "enum", "struct",
        "impl", "where", "mut", "pub",
    ] {
        syntax.add_rule(TokenKind::Keyword, Pattern::new(keyword).unwrap());
    }

    for symbol in &[
        "%(", "%)", "%[", "%]", "%{", "%}", ":", ";", ",", "=", "<", ">", "+", "-", "/", "*", "%%",
        "%.", "%!", "?", "&", "|", "@",
    ] {
        syntax.add_rule(TokenKind::Symbol, Pattern::new(symbol).unwrap());
    }

    for t in &[
        "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "str",
    ] {
        syntax.add_rule(TokenKind::Type, Pattern::new(t).unwrap());
    }
    syntax.add_rule(TokenKind::Type, Pattern::new("%u{%w}").unwrap());

    syntax.add_rule(TokenKind::Comment, Pattern::new("//{.}").unwrap());
    syntax.add_rule(TokenKind::Comment, Pattern::new("/*{!(*/).$}").unwrap());

    for literal in &["true", "false", "self"] {
        syntax.add_rule(TokenKind::Literal, Pattern::new(literal).unwrap());
    }

    syntax.add_rule(TokenKind::Literal, Pattern::new("'{(\\')!'.}").unwrap());
    syntax.add_rule(TokenKind::Literal, Pattern::new("%d{%w%._}").unwrap());
    syntax.add_rule(TokenKind::String, Pattern::new("\"{(\\\")!\".}").unwrap());

    syntax.add_rule(TokenKind::Type, Pattern::new("'%a{%w_}").unwrap());
    syntax.add_rule(TokenKind::Type, Pattern::new("%u{%w_}").unwrap());

    syntax.add_rule(TokenKind::Text, Pattern::new("%a{%w_}").unwrap());
    syntax.add_rule(TokenKind::Text, Pattern::new("_{%w_}").unwrap());
}

fn set_lua_syntax(syntax: &mut Syntax) {
    for keyword in &[
        "and", "break", "do", "else", "elseif", "end", "for", "function", "if", "in", "local",
        "not", "or", "repeat", "return", "then", "until", "while",
    ] {
        syntax.add_rule(TokenKind::Keyword, Pattern::new(keyword).unwrap());
    }

    for symbol in &[
        "+", "-", "*", "/", "%%", "^", "#", "<", ">", "=", "~", "%(", "%)", "%{", "%}", "%[", "%]",
        ";", ":", ",", "%.", "%.%.", "%.%.%.",
    ] {
        syntax.add_rule(TokenKind::Symbol, Pattern::new(symbol).unwrap());
    }

    syntax.add_rule(TokenKind::Comment, Pattern::new("--{.}").unwrap());
    syntax.add_rule(
        TokenKind::Comment,
        Pattern::new("--%[%[{!(%]%]).$}").unwrap(),
    );

    for literal in &["nil", "false", "true", "_G", "_ENV"] {
        syntax.add_rule(TokenKind::Literal, Pattern::new(literal).unwrap());
    }

    syntax.add_rule(TokenKind::Literal, Pattern::new("%d{%w%._}").unwrap());
    syntax.add_rule(TokenKind::String, Pattern::new("'{(\\')!'.}").unwrap());
    syntax.add_rule(TokenKind::String, Pattern::new("\"{(\\\")!\".}").unwrap());
    syntax.add_rule(TokenKind::String, Pattern::new("%[%[{!(%]%]).}").unwrap());

    syntax.add_rule(TokenKind::Text, Pattern::new("%a{%w_}").unwrap());
    syntax.add_rule(TokenKind::Text, Pattern::new("_{%w_}").unwrap());
}
