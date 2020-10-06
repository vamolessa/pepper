use std::num::NonZeroUsize;

use crate::{
    pattern::Pattern,
    syntax::{Syntax, SyntaxCollection, TokenKind},
    theme::{pico8_theme, Theme},
};

#[derive(Debug, Clone)]
pub struct ConfigValues {
    pub tab_size: NonZeroUsize,
    pub indent_with_tabs: bool,

    pub visual_empty: char,
    pub visual_space: char,
    pub visual_tab_first: char,
    pub visual_tab_repeat: char,

    pub picker_max_height: NonZeroUsize,
}

impl Default for ConfigValues {
    fn default() -> Self {
        Self {
            tab_size: NonZeroUsize::new(4).unwrap(),
            indent_with_tabs: true,

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
