use std::{
    io::{StdoutLock, Write},
    iter,
};

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
    window_size: (u16, u16),

    buffer: Buffer,
    theme: Theme,
}

impl<'a> TerminalView<'a> {
    pub fn new(stdout: StdoutLock<'a>, buffer: Buffer) -> Result<Self> {
        let mut s = Self {
            stdout,
            window_size: (0, 0),
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
        self.window_size = terminal::size().unwrap_or((0, 0));
    }

    pub fn move_cursor_left(&mut self) {
        if self.buffer.cursor.column_index > 0 {
            self.buffer.cursor.column_index -= 1;
        }
    }

    pub fn move_cursor_down(&mut self) {
        if self.buffer.cursor.line_index < self.buffer.lines.len() + 1 {
            self.buffer.cursor.line_index += 1;
        }
    }

    pub fn move_cursor_up(&mut self) {
        if self.buffer.cursor.line_index > 0 {
            self.buffer.cursor.line_index -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if true {
        self.buffer.cursor.column_index += 1;
        }
    }

    pub fn print(&mut self) -> Result<()> {
        handle_command!(self.stdout, cursor::MoveTo(0, 0))?;

        handle_command!(self.stdout, SetForegroundColor(self.theme.foreground))?;
        handle_command!(self.stdout, SetBackgroundColor(self.theme.background))?;

        let mut was_inside_selection = false;
        for (y, line) in self.buffer.lines.iter().enumerate() {
            for (x, c) in line.chars().chain(iter::once(' ')).enumerate() {
                let inside_selection =
                    x == self.buffer.cursor.column_index && y == self.buffer.cursor.line_index;
                if was_inside_selection != inside_selection {
                    was_inside_selection = inside_selection;
                    if inside_selection {
                        handle_command!(self.stdout, SetForegroundColor(self.theme.background))?;
                        handle_command!(self.stdout, SetBackgroundColor(self.theme.foreground))?;
                    } else {
                        handle_command!(self.stdout, SetForegroundColor(self.theme.foreground))?;
                        handle_command!(self.stdout, SetBackgroundColor(self.theme.background))?;
                    }
                }

                match c {
                    '\t' => handle_command!(self.stdout, Print("    "))?,
                    _ => handle_command!(self.stdout, Print(c))?,
                }
            }

            handle_command!(self.stdout, SetForegroundColor(self.theme.foreground))?;
            handle_command!(self.stdout, SetBackgroundColor(self.theme.background))?;
            handle_command!(self.stdout, Clear(ClearType::UntilNewLine))?;
            handle_command!(self.stdout, cursor::MoveToNextLine(1))?;
        }

        handle_command!(self.stdout, SetForegroundColor(self.theme.foreground))?;
        handle_command!(self.stdout, SetBackgroundColor(self.theme.background))?;
        for _ in self.buffer.lines.len()..self.window_size.1 as usize {
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
