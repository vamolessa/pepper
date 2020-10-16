use std::num::NonZeroUsize;

use crate::{
    syntax::SyntaxCollection,
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
        Self {
            values: ConfigValues::default(),
            theme: pico8_theme(),
            syntaxes: SyntaxCollection::new(),
        }
    }
}
