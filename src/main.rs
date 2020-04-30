use std::io::stdout;

pub mod event {
    pub type Event = crossterm::event::Event;
    pub type KeyCode = crossterm::event::KeyCode;
    pub type KeyEvent = crossterm::event::KeyEvent;
}

mod buffer;
mod buffer_view;
mod config;
mod editor;
mod modes;
mod terminal_ui;
mod theme;

fn main() {
    ctrlc::set_handler(|| {}).unwrap();

    let stdout = stdout();
    let stdout = stdout.lock();

    let mut editor = editor::Editor::default();
    let handle = editor
        .buffers
        .add(buffer::Buffer::from_str(include_str!("main.rs")));
    editor
        .buffer_views
        .push(buffer_view::BufferView::with_handle(handle));

    terminal_ui::show(stdout, editor).unwrap();
}
