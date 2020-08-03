use crate::{syntax::SyntaxCollection, theme::Theme};

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
            syntaxes: SyntaxCollection::default(),
            tab_size: 4,
        }
    }
}
