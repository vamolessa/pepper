use crossterm::{event, Result};
use std::io::stdout;

mod buffer;
mod buffer_view;
mod terminal_view;
mod theme;

fn main() -> Result<()> {
    ctrlc::set_handler(|| {}).unwrap();

    let stdout = stdout();
    let stdout = stdout.lock();

    let mut view = terminal_view::TerminalView::new(stdout)?;
    let handle = view
        .buffers
        .add(buffer::Buffer::from_str(include_str!("main.rs")));
    view.buffer_views.push(buffer_view::BufferView::with_handle(handle));
    view.print(0)?;

    loop {
        let bv = &mut view.buffer_views[0];
        let bs =&view.buffers;
        match event::read()? {
            event::Event::Key(event::KeyEvent {
                code: event::KeyCode::Char('q'),
                ..
            }) => break,
            event::Event::Key(event::KeyEvent {
                code: event::KeyCode::Char('h'),
                ..
            }) => {
                bv.move_cursor_left();
                view.print(0)?;
            }
            event::Event::Key(event::KeyEvent {
                code: event::KeyCode::Char('j'),
                ..
            }) => {
                bv.move_cursor_down(bs);
                view.print(0)?;
            }
            event::Event::Key(event::KeyEvent {
                code: event::KeyCode::Char('k'),
                ..
            }) => {
                bv.move_cursor_up(bs);
                view.print(0)?;
            }
            event::Event::Key(event::KeyEvent {
                code: event::KeyCode::Char('l'),
                ..
            }) => {
                bv.move_cursor_right(bs);
                view.print(0)?;
            }
            _ => (),
        }
    }

    Ok(())
}
