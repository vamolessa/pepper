use std::{io, sync::mpsc, thread};

use crate::client_event::{Key, LocalEvent};

use super::{Ui, UiResult};

pub struct NoneUi;
impl Ui for NoneUi {
    fn run_event_loop_in_background(
        &mut self,
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> thread::JoinHandle<()> {
        use io::BufRead;
        thread::spawn(move || {
            let stdin = io::stdin();
            let mut stdin = stdin.lock();
            let mut line = String::new();

            'main_loop: loop {
                if stdin.read_line(&mut line).is_err() || line.is_empty() {
                    break;
                }

                for key in Key::parse_all(&line) {
                    match key {
                        Ok(key) => {
                            if event_sender.send(LocalEvent::Key(key)).is_err() {
                                break 'main_loop;
                            }
                        }
                        Err(_) => break,
                    }
                }

                line.clear();
            }

            let _ = event_sender.send(LocalEvent::EndOfInput);
        })
    }

    fn display(&mut self, _buffer: &[u8]) -> UiResult<()> {
        Ok(())
    }
}
