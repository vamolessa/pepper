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
