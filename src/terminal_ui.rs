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

use crate::{buffer::BufferCollection, buffer_view::BufferView, theme::Theme};

pub struct TerminalUi<'a> {
    stdout: StdoutLock<'a>,

    pub buffers: BufferCollection,
    pub buffer_views: Vec<BufferView>,
    theme: Theme,
}

impl<'a> TerminalUi<'a> {
    pub fn new(stdout: StdoutLock<'a>) -> Result<Self> {
        let mut s = Self {
            stdout,
            buffers: Default::default(),
            buffer_views: Default::default(),
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
        Ok(s)
    }

    pub fn update_buffer_views_size(&mut self) {
        let size = terminal::size().unwrap_or((0, 0));
        for view in &mut self.buffer_views {
            view.size = size;
        }
    }

    pub fn print(&mut self, view_index: usize) -> Result<()> {
        let buffer_view = &self.buffer_views[view_index];
        let buffer = &self.buffers[buffer_view.buffer_handle];

        handle_command!(self.stdout, cursor::MoveTo(0, 0))?;
        handle_command!(self.stdout, cursor::Hide)?;

        handle_command!(self.stdout, SetForegroundColor(self.theme.foreground))?;
        handle_command!(self.stdout, SetBackgroundColor(self.theme.background))?;

        let mut was_inside_selection = false;
        for (y, line) in buffer
            .lines
            .iter()
            .skip(buffer_view.scroll as usize)
            .take(buffer_view.size.1 as usize)
            .enumerate()
        {
            let y = y + buffer_view.scroll as usize;
            for (x, c) in line.chars().chain(iter::once(' ')).enumerate() {
                let inside_selection = x == buffer_view.cursor.column_index as usize
                    && y == buffer_view.cursor.line_index as usize;
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
        for _ in buffer.lines.len()..buffer_view.size.1 as usize {
            handle_command!(self.stdout, Print('~'))?;
            handle_command!(self.stdout, Clear(ClearType::UntilNewLine))?;
            handle_command!(self.stdout, cursor::MoveToNextLine(1))?;
        }

        handle_command!(self.stdout, ResetColor)?;
        self.stdout.flush()?;
        Ok(())
    }
}

impl<'a> Drop for TerminalUi<'a> {
    fn drop(&mut self) {
        handle_command!(self.stdout, terminal::LeaveAlternateScreen).unwrap();
        handle_command!(self.stdout, cursor::Show).unwrap();
        terminal::disable_raw_mode().unwrap();
    }
}
