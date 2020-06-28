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

    let mut editor = editor::Editor::default();
    editor.new_buffer_from_content(content);

    let frame_durations = tui::show(stdout, editor).unwrap();
    eprintln!("frame count: {}", frame_durations.len());
    if frame_durations.len() > 0 {
        eprintln!(
            "mean frame: {:?}",
            frame_durations.iter().sum::<std::time::Duration>() / frame_durations.len() as u32
        );
        eprintln!(
            "median frame: {:?}",
            frame_durations[frame_durations.len() / 2]
        );
        eprintln!("min frame: {:?}", frame_durations.iter().min().unwrap());
        eprintln!("max frame: {:?}", frame_durations.iter().max().unwrap());

        eprintln!();
        eprintln!("frames:");
        for frame in frame_durations {
            eprintln!("{:?}", frame.as_millis());
        }
    }
}
