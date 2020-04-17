use std::io::{stdout, Write};

use crossterm::{
    cursor, handle_command,
    style::{Print, ResetColor},
    terminal::{Clear, ClearType},
    Result,
};

mod buffer;

fn main() -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;

    let stdout = stdout();
    let mut stdout = stdout.lock();

    let src = include_str!("main.rs");

    for line in src.lines() {
        handle_command!(stdout, Print(line))?;
        handle_command!(stdout, Clear(ClearType::UntilNewLine))?;
        handle_command!(stdout, cursor::MoveToNextLine(1))?;
    }

    handle_command!(stdout, ResetColor)?;
    stdout.flush()?;
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}
