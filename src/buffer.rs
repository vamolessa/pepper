pub struct Cursor {
    pub line_index: usize,
    pub column_index: usize,
}

pub struct Buffer {
    pub lines: Vec<String>,
    pub cursor: Cursor,
}

impl Buffer {
    pub fn from_str(text: &str) -> Self {
        Self {
            lines: text.lines().map(Into::into).collect(),
            cursor: Cursor {
                line_index: 0,
                column_index: 0,
            },
        }
    }
}
