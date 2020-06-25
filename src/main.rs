use std::io::stdout;

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

    let stdout = stdout();
    let stdout = stdout.lock();

    //let text = include_str!("main.rs");
    let text = r#"banana
banana



banana
apple
pencil
zebra
yellow

hope everything works fine! :')
"#;
    let content = buffer::BufferContent::from_str(text);

    let mut editor = editor::Editor::default();
    editor.new_buffer_from_content(content);

    tui::show(stdout, editor).unwrap();
}
