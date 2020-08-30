use std::{fmt, num::NonZeroUsize, str::FromStr};

use crate::{
    pattern::Pattern,
    syntax::{Syntax, SyntaxCollection, TokenKind},
    theme::{pico8_theme, Theme},
};

pub enum ParseConfigError {
    ConfigNotFound,
    ParseError(Box<dyn fmt::Display>),
}

impl fmt::Display for ParseConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ConfigNotFound => write!(f, "could not find config"),
            Self::ParseError(e) => write!(f, "config parse error: {}", e),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigValues {
    pub tab_size: NonZeroUsize,
    pub visual_empty: char,
    pub visual_space: char,
    pub visual_tab_first: char,
    pub visual_tab_repeat: char,
}

impl ConfigValues {
    pub fn parse_and_set<'a>(&mut self, name: &str, value: &str) -> Result<(), ParseConfigError> {
        fn parse<T>(value: &str) -> Result<T, ParseConfigError>
        where
            T: FromStr,
            T::Err: 'static + fmt::Display,
        {
            value
                .parse()
                .map_err(|e| ParseConfigError::ParseError(Box::new(e)))
        }

        macro_rules! match_and_parse {
            ($($name:ident,)*) => {
                match name {
                    $(stringify!($name) => self.$name = parse(value)?,)*
                    _ => return Err(ParseConfigError::ConfigNotFound),
                }
            }
        }

        match_and_parse! {
            tab_size,
            visual_empty,
            visual_space,
            visual_tab_first,
            visual_tab_repeat,
        }

        Ok(())
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
        let mut syntaxes = SyntaxCollection::default();
        set_rust_syntax(syntaxes.get_by_extension("rs"));

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
        "%(", "%)", "%[", "%]", "%{", "%}", ":", ";", ",", "=", "<", ">", "+", "-", "/", "*", "%.",
        "%!", "?", "&", "|", "@",
    ] {
        syntax.add_rule(TokenKind::Symbol, Pattern::new(symbol).unwrap());
    }

    for t in &["bool", "u32", "f32"] {
        syntax.add_rule(TokenKind::Type, Pattern::new(t).unwrap());
    }

    for literal in &["true", "false", "self"] {
        syntax.add_rule(TokenKind::Literal, Pattern::new(literal).unwrap());
    }

    syntax.add_rule(TokenKind::Comment, Pattern::new("//{.}").unwrap());
    syntax.add_rule(TokenKind::Comment, Pattern::new("/*{!(*/).$}").unwrap());

    syntax.add_rule(TokenKind::Literal, Pattern::new("'{(\\')!'.}").unwrap());
    syntax.add_rule(TokenKind::Literal, Pattern::new("%d{%w%._}").unwrap());
    syntax.add_rule(TokenKind::String, Pattern::new("\"{(\\\")!\".}").unwrap());

    syntax.add_rule(TokenKind::Type, Pattern::new("'%a{%w_}").unwrap());
    syntax.add_rule(TokenKind::Type, Pattern::new("%u{%w_}").unwrap());

    syntax.add_rule(TokenKind::Text, Pattern::new("%a{%w_}").unwrap());
    syntax.add_rule(TokenKind::Text, Pattern::new("_{%w_}").unwrap());
}
