use std::{env, fmt, fs::File, io::Read, num::NonZeroUsize, path::PathBuf, str::FromStr};

use serde_derive::{Deserialize, Serialize};

use crate::{
    command::{CommandCollection, ConfigCommandContext},
    pattern::Pattern,
    syntax::{Syntax, SyntaxCollection, TokenKind},
    theme::{pico8_theme, Theme},
};

pub enum ParseConfigError {
    ConfigNotFound,
    ParseError(Box<dyn fmt::Display>),
    UnexpectedEndOfValues,
}

impl fmt::Display for ParseConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ConfigNotFound => write!(f, "could not find config"),
            Self::ParseError(e) => write!(f, "config parse error: {}", e),
            Self::UnexpectedEndOfValues => write!(f, "unexpected end of values for config"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValues {
    pub tab_size: NonZeroUsize,
    pub visual_empty: char,
    pub visual_space: char,
    pub visual_tab: (char, char),
}

impl ConfigValues {
    pub fn parse_and_set<'a>(
        &mut self,
        name: &str,
        values: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(), ParseConfigError> {
        fn parse<T>(value: &str) -> Result<T, ParseConfigError>
        where
            T: FromStr,
            T::Err: 'static + fmt::Display,
        {
            value
                .parse()
                .map_err(|e| ParseConfigError::ParseError(Box::new(e)))
        }

        macro_rules! parse_next {
            () => {
                match values.next() {
                    Some(value) => parse(value)?,
                    None => return Err(ParseConfigError::UnexpectedEndOfValues),
                }
            };
        }

        macro_rules! match_and_parse {
            ($($name:ident = $value:expr,)*) => {
                match name {
                    $(stringify!($name) => self.$name = $value,)*
                    _ => return Err(ParseConfigError::ConfigNotFound),
                }
            }
        }

        match_and_parse! {
            tab_size = parse_next!(),
            visual_empty = parse_next!(),
            visual_space = parse_next!(),
            visual_tab = (parse_next!(), parse_next!()),
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
            visual_tab: ('|', ' '),
        }
    }
}

pub struct Config {
    pub values: ConfigValues,
    pub theme: Theme,
    pub syntaxes: SyntaxCollection,
}

impl Config {
    pub fn load_into_operations(
        commands: &CommandCollection,
        ctx: &mut ConfigCommandContext,
    ) -> Result<(), String> {
        let path = match env::var("PEPPERC") {
            Ok(path) => PathBuf::from(path)
                .canonicalize()
                .map_err(|e| e.to_string())?,
            Err(_) => return Ok(()),
        };
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(e) => return Err(e.to_string()),
        };
        let mut contents = String::with_capacity(2 * 1024);
        file.read_to_string(&mut contents)
            .map_err(|e| e.to_string())?;

        for (i, line) in contents
            .lines()
            .enumerate()
            .filter(|(_, l)| l.starts_with('#'))
        {
            commands
                .parse_and_execut_config_command(ctx, line)
                .map_err(|e| format!("error at {:?}:{} {}", path, i + 1, e))?;
        }

        Ok(())
    }
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
