use std::io::{StdoutLock, Write};

use crossterm::{
    cursor, handle_command,
    style::{Print, ResetColor},
    terminal::{self, Clear, ClearType},
    Result,
};

use crate::buffer::Buffer;

pub struct TerminalView<'a> {
    stdout: StdoutLock<'a>,
    size: (u16, u16),

    buffer: Buffer,
}

impl<'a> TerminalView<'a> {
    pub fn new(stdout: StdoutLock<'a>, buffer: Buffer) -> Result<Self> {
        let mut s = Self {
            stdout,
            size: (0, 0),
            buffer,
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

    pub fn print(&mut self) -> Result<()> {
        handle_command!(self.stdout, cursor::MoveTo(0, 0))?;

        for line in &self.buffer.lines {
            handle_command!(self.stdout, Print(line))?;
            handle_command!(self.stdout, Clear(ClearType::UntilNewLine))?;
            handle_command!(self.stdout, cursor::MoveToNextLine(1))?;
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
