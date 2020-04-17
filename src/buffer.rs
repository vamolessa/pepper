pub struct Buffer {
    pub lines: Vec<String>,
}

impl Buffer {
    pub fn from_str(text: &str) -> Self {
        Self {
            lines: text.lines().map(Into::into).collect(),
        }
    }
}
