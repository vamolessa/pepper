mod buffer;
mod buffer_position;
mod buffer_view;
mod command;
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
    let buffer_handle = editor.buffers.add(buffer::Buffer::new(None, content));
    let buffer_view_index = editor
        .buffer_views
        .add(buffer_view::BufferView::with_handle(buffer_handle));
    editor
        .viewports
        .current_viewport_mut()
        .set_current_buffer_view_handle(buffer_view_index);

    tui::show(stdout, editor).unwrap();
}
