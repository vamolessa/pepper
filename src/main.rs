use std::io::stdout;

mod buffer;
mod buffer_view;
mod command;
mod config;
mod editor;
mod event;
mod mode;
mod theme;
mod tui;

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

    tui::show(stdout, editor).unwrap();
}
