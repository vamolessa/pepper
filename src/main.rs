mod buffer;
mod buffer_position;
mod buffer_view;
mod config;
mod cursor;
mod editor;
mod event;
mod history;
mod mode;
mod theme;
mod tui;
mod viewport;

fn main() {
    ctrlc::set_handler(|| {}).unwrap();

    let stdout = std::io::stdout();
    let stdout = stdout.lock();

    let text = include_str!("main.rs");
    let content = buffer::BufferContent::from_str(text);

    let mut editor = editor::Editor::new();
    editor.new_buffer_from_content(content);

    tui::show(stdout, editor).unwrap();
}
