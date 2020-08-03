use crate::{
    pattern::Pattern,
    syntax::{Syntax, SyntaxCollection, TokenKind},
    theme::Theme,
};

pub struct Config {
    pub theme: Theme,
    pub syntaxes: SyntaxCollection,
    pub tab_size: usize,
}

impl Config {
    pub fn reload(&mut self) {
        //
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            syntaxes: default_syntaxes(),
            tab_size: 4,
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
    let mut syntax = Syntax::new();
    syntax.add_extension("toml".into());

    syntax.add_rule(TokenKind::Symbol, Pattern::new("=").unwrap());
    syntax.add_rule(TokenKind::Keyword, Pattern::new("%[{%w!%]}").unwrap());
    syntax.add_rule(TokenKind::Keyword, Pattern::new("%[%[{%w!%]}%]").unwrap());
    syntax.add_rule(TokenKind::String, Pattern::new("\"{!\".}").unwrap());

    syntax
}

fn rust_syntax() -> Syntax {
    let mut syntax = Syntax::new();
    syntax.add_extension("rs".into());

    for keyword in &["fn", "let", "if", "while", "for", "return", "mod", "use", "as", "in"] {
        syntax.add_rule(TokenKind::Keyword, Pattern::new(keyword).unwrap());
    }

    for symbol in &[
        "%(", "%)", "%[", "%]", "%{", "%}", ":", ";", ",", "=", "<", ">", "+", "-", "/", "*", "%.",
        "%!", "&", "|"
    ] {
        syntax.add_rule(TokenKind::Symbol, Pattern::new(symbol).unwrap());
    }

    for modifier in &["pub", "mut"] {
        syntax.add_rule(TokenKind::Modifier, Pattern::new(modifier).unwrap());
    }

    for literal in &["true", "false", "self"] {
        syntax.add_rule(TokenKind::Literal, Pattern::new(literal).unwrap());
    }

    syntax.add_rule(TokenKind::Comment, Pattern::new("//{.}").unwrap());
    syntax.add_rule(TokenKind::Comment, Pattern::new("/*{!(*/).$}").unwrap());

    syntax.add_rule(TokenKind::Literal, Pattern::new("'{(\\')!'.}").unwrap());
    syntax.add_rule(TokenKind::Literal, Pattern::new("%d{%w%.}").unwrap());
    syntax.add_rule(TokenKind::String, Pattern::new("\"{(\\\")!\".}").unwrap());
    syntax.add_rule(TokenKind::Modifier, Pattern::new("'%a{%w}").unwrap());

    syntax.add_rule(TokenKind::Text, Pattern::new("%a{%w}").unwrap());

    syntax
}
