use crate::theme::Theme;

pub struct Config {
    pub theme: Theme,
    pub tab_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            tab_size: 4,
        }
    }
}
