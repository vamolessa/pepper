use std::{fmt, num::NonZeroUsize, str::FromStr};

use crate::{
    pattern::Pattern,
    syntax::{Syntax, SyntaxCollection, TokenKind},
    theme::{pico8_theme, Theme},
};

pub enum ParseConfigError {
    ConfigNotFound(String),
    ParseError(String, Box<dyn fmt::Display>),
    UnexpectedEndOfValues(String),
}

impl fmt::Display for ParseConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ConfigNotFound(name) => write!(f, "could not find config '{}'", name),
            Self::ParseError(name, e) => write!(f, "config '{}' parse error: {}", name, e),
            Self::UnexpectedEndOfValues(name) => {
                write!(f, "unexpected end of values for config '{}'", name)
            }
        }
    }
}

pub struct Config {
    pub theme: Theme,
    pub syntaxes: SyntaxCollection,

    pub tab_size: NonZeroUsize,
    pub visual_empty: char,
    pub visual_space: char,
    pub vusual_tab: (char, char),
}

impl Config {
    pub fn load(&mut self) {
        //
    }

    pub fn parse_and_set<'a>(
        &mut self,
        name: &str,
        values: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(), ParseConfigError> {
        fn parse<T>(name: &str, value: &str) -> Result<T, ParseConfigError>
        where
            T: FromStr,
            T::Err: 'static + fmt::Display,
        {
            value
                .parse()
                .map_err(|e| ParseConfigError::ParseError(name.into(), Box::new(e)))
        }

        macro_rules! parse_next {
            () => {
                match values.next() {
                    Some(value) => parse(name, value)?,
                    None => return Err(ParseConfigError::UnexpectedEndOfValues(name.into())),
                }
            };
        }

        macro_rules! match_and_parse {
            ($($name:ident = $value:expr,)*) => {
                match name {
                    $(stringify!($name) => self.$name = $value,)*
                    _ => return Err(ParseConfigError::ConfigNotFound(name.into())),
                }
            }
        }

        match_and_parse! {
            tab_size = parse_next!(),
            visual_empty = parse_next!(),
            visual_space = parse_next!(),
            vusual_tab = (parse_next!(), parse_next!()),
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: pico8_theme(),
            syntaxes: default_syntaxes(),
            tab_size: NonZeroUsize::new(4).unwrap(),
            visual_empty: '~',
            visual_space: '.',
            vusual_tab: ('|', ' '),
        }
    }
}

fn default_syntaxes() -> SyntaxCollection {
    let mut syntaxes = SyntaxCollection::default();
    syntaxes.add(toml_syntax());
    syntaxes.add(rust_syntax());
    syntaxes
}

fn toml_syntax() -> Syntax {
    let mut syntax = Syntax::with_extension("toml".into());
    syntax.add_rule(TokenKind::Symbol, Pattern::new("=").unwrap());
    syntax.add_rule(TokenKind::Keyword, Pattern::new("%[{%w!%]}").unwrap());
    syntax.add_rule(TokenKind::Keyword, Pattern::new("%[%[{%w!%]}%]").unwrap());
    syntax.add_rule(TokenKind::String, Pattern::new("\"{!\".}").unwrap());

    syntax
}

fn rust_syntax() -> Syntax {
    let mut syntax = Syntax::with_extension("rs".into());

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

    syntax
}
