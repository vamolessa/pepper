use std::{sync::mpsc, thread};

use crate::client_event::LocalEvent;

use super::{read_keys_from_stdin, Ui, UiResult};

pub struct NoneUi;
impl Ui for NoneUi {
    fn run_event_loop_in_background(
        &mut self,
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || read_keys_from_stdin(event_sender))
    }

    fn display(&mut self, _buffer: &[u8]) -> UiResult<()> {
        Ok(())
    }
}
