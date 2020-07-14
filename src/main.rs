mod application;
mod buffer;
mod buffer_position;
mod buffer_view;
mod client;
mod command;
mod config;
mod connection;
mod cursor;
mod editor;
mod event;
mod history;
mod keymap;
mod mode;
mod theme;
mod tui;

fn main() {
    ctrlc::set_handler(|| {}).unwrap();

    let stdout = std::io::stdout();
    let stdout = stdout.lock();
    let ui = tui::Tui::new(stdout);

    /*
    let text = include_str!("main.rs");
    let content = buffer::BufferContent::from_str(text);

    let mut editor = editor::Editor::new();

    // custom bindings
    {
        editor
            .keymaps
            .parse_map(mode::Mode::Normal.discriminant(), "qq", ":quit<c-m>")
            .unwrap();
        editor
            .keymaps
            .parse_map(mode::Mode::Normal.discriminant(), "edit", "i")
            .unwrap();
        editor
            .keymaps
            .parse_map(mode::Mode::Normal.discriminant(), "dl", "vld")
            .unwrap();
        editor
            .keymaps
            .parse_map(mode::Mode::Normal.discriminant(), "dh", "vvhd")
            .unwrap();
        editor
            .keymaps
            .parse_map(
                mode::Mode::Normal.discriminant(),
                "<c-f>",
                ":find-command<c-m>",
            )
            .unwrap();
    }

    let buffer_handle = editor.buffers.add(buffer::Buffer::new(None, content));
    let buffer_view_index = editor
        .buffer_views
        .add(buffer_view::BufferView::with_handle(buffer_handle));
    editor
        .viewports
        .current_viewport_mut()
        .set_current_buffer_view_handle(buffer_view_index);
    */

    smol::run(async { application::run_server_with_client(tui::event_stream(), ui).await })
        .unwrap();
}
