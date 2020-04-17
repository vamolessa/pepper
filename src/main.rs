use crossterm::Result;
use std::io::stdout;

mod buffer;
mod terminal_view;

fn main() -> Result<()> {
    let stdout = stdout();
    let stdout = stdout.lock();

    let src = include_str!("main.rs");
    let mut view = terminal_view::TerminalView::new(stdout, buffer::Buffer::from_str(src))?;
    view.print()?;

    Ok(())
}
