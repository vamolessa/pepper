pub struct Config {
    pub tab_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tab_size: 4
        }
    }
}
