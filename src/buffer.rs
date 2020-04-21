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

    pub fn move_cursor_left(&mut self) {
        if self.cursor.column_index > 0 {
            self.cursor.column_index -= 1;
        }
    }

    pub fn move_cursor_down(&mut self) {
        if self.cursor.line_index < self.lines.len() - 1 {
            self.cursor.line_index += 1;
            let line_len = self.lines[self.cursor.line_index]
                .chars()
                .count();
            self.cursor.column_index = self.cursor.column_index.min(line_len);
        }
    }

    pub fn move_cursor_up(&mut self) {
        if self.cursor.line_index > 0 {
            self.cursor.line_index -= 1;
            let line_len = self.lines[self.cursor.line_index]
                .chars()
                .count();
            self.cursor.column_index = self.cursor.column_index.min(line_len);
        }
    }

    pub fn move_cursor_right(&mut self) {
        let line_len = self.lines[self.cursor.line_index]
            .chars()
            .count();
        if self.cursor.column_index < line_len {
            self.cursor.column_index += 1;
        }
    }
}
