use std::io::{StdoutLock, Write};

use crossterm::{
    cursor, handle_command,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
    Result,
};

use crate::buffer::Buffer;
use crate::theme::Theme;

pub struct TerminalView<'a> {
    stdout: StdoutLock<'a>,
    size: (u16, u16),

    buffer: Buffer,
    theme: Theme,
}

impl<'a> TerminalView<'a> {
    pub fn new(stdout: StdoutLock<'a>, buffer: Buffer) -> Result<Self> {
        let mut s = Self {
            stdout,
            size: (0, 0),
            buffer,
            theme: Theme {
                foreground: Color::White,
                background: Color::Black,
            },
        };

        handle_command!(s.stdout, terminal::EnterAlternateScreen)?;
        s.stdout.flush()?;
        handle_command!(s.stdout, cursor::Hide)?;
        s.stdout.flush()?;

        terminal::enable_raw_mode()?;
        s.query_size();
        Ok(s)
    }

    pub fn query_size(&mut self) {
        self.size = terminal::size().unwrap_or((0, 0));
    }

    pub fn move_cursor_left(&mut self) {
        self.buffer.cursor.column_index -= 1;
    }

    pub fn move_cursor_down(&mut self) {
        self.buffer.cursor.line_index += 1;
    }

    pub fn move_cursor_up(&mut self) {
        self.buffer.cursor.line_index -= 1;
    }

    pub fn move_cursor_right(&mut self) {
        self.buffer.cursor.column_index += 1;
    }

    pub fn print(&mut self) -> Result<()> {
        handle_command!(self.stdout, cursor::MoveTo(0, 0))?;

        handle_command!(self.stdout, SetForegroundColor(self.theme.foreground))?;
        handle_command!(self.stdout, SetBackgroundColor(self.theme.background))?;
        for (i, line) in self.buffer.lines.iter().enumerate() {
            if i == self.buffer.cursor.line_index {
                let mut line_chars = line.chars();
                let mut before_cursor_count = 0;

                while before_cursor_count < self.buffer.cursor.column_index {
                    if let Some(c) = line_chars.next() {
                        before_cursor_count += 1;
                        handle_command!(self.stdout, Print(c))?;
                    } else {
                        break;
                    }
                }

                handle_command!(self.stdout, SetForegroundColor(self.theme.background))?;
                handle_command!(self.stdout, SetBackgroundColor(self.theme.foreground))?;
                if let Some(c) = &mut line_chars.next() {
                    handle_command!(self.stdout, Print(*c))?;
                } else {
                    handle_command!(self.stdout, Print(' '))?;
                }

                handle_command!(self.stdout, SetForegroundColor(self.theme.foreground))?;
                handle_command!(self.stdout, SetBackgroundColor(self.theme.background))?;
                for c in &mut line_chars {
                    handle_command!(self.stdout, Print(c))?;
                }
                handle_command!(self.stdout, Clear(ClearType::UntilNewLine))?;
                handle_command!(self.stdout, cursor::MoveToNextLine(1))?;
            } else {
                handle_command!(self.stdout, Print(line))?;
                handle_command!(self.stdout, Clear(ClearType::UntilNewLine))?;
                handle_command!(self.stdout, cursor::MoveToNextLine(1))?;
            }
        }

        for _ in self.buffer.lines.len()..self.size.1 as usize {
            handle_command!(self.stdout, Print('~'))?;
            handle_command!(self.stdout, Clear(ClearType::UntilNewLine))?;
            handle_command!(self.stdout, cursor::MoveToNextLine(1))?;
        }

        handle_command!(self.stdout, ResetColor)?;
        self.stdout.flush()?;
        Ok(())
    }
}

impl<'a> Drop for TerminalView<'a> {
    fn drop(&mut self) {
        handle_command!(self.stdout, terminal::LeaveAlternateScreen).unwrap();
        handle_command!(self.stdout, cursor::Show).unwrap();
        terminal::disable_raw_mode().unwrap();
    }
}
