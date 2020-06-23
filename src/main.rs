use std::io::stdout;

mod buffer;
mod cursor;
mod buffer_position;
mod buffer_view;
mod config;
mod editor;
mod event;
mod history;
mod mode;
mod theme;
mod tui;
mod viewport;

fn main() {
    ctrlc::set_handler(|| {}).unwrap();

    let stdout = stdout();
    let stdout = stdout.lock();

    let mut editor = editor::Editor::default();
    let content = buffer::BufferContent::from_str(include_str!("main.rs"));
    editor.new_buffer_from_content(content);

    tui::show(stdout, editor).unwrap();
}
