use std::io::{StdoutLock, Write};

use crossterm::{
    cursor, handle_command,
    style::{Print, ResetColor},
    terminal::{Clear, ClearType},
    Result,
};

use crate::buffer::Buffer;

pub struct TerminalView<'a> {
    stdout: StdoutLock<'a>,
    buffer: Buffer,
}

impl<'a> TerminalView<'a> {
    pub fn new(stdout: StdoutLock<'a>, buffer: Buffer) -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self { stdout, buffer })
    }

    pub fn print(&mut self) -> Result<()> {
        for line in &self.buffer.lines {
            handle_command!(self.stdout, Print(line))?;
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
        crossterm::terminal::disable_raw_mode().unwrap();
    }
}
