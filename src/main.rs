use crossterm::{event, Result};
use std::io::stdout;

mod buffer;
mod terminal_view;

fn main() -> Result<()> {
    let stdout = stdout();
    let stdout = stdout.lock();

    let buffer = buffer::Buffer::from_str(include_str!("main.rs"));
    let mut view = terminal_view::TerminalView::new(stdout, buffer)?;
    view.print()?;

    loop {
        match event::read()? {
            event::Event::Key(event::KeyEvent {
                code: event::KeyCode::Char('q'),
                ..
            }) => break,
            _ => (),
        }
    }

    Ok(())
}
