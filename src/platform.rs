// TODO: move Key and ConnectionEvent here
use crate::{event_manager::ConnectionEvent, client_event::Key};

use crate::client::TargetClient;

pub enum PlatformEvent<'a> {
    Close,
    Idle,
    Resize(usize, usize),
    Key(Key),
    ClientOpen(TargetClient),
    ClientClose(TargetClient),
    ClientMessage(TargetClient, &'a [u8]),
    ServerMessage(&'a [u8]),
    ProcessStdout(usize, &'a [u8]),
}

pub trait Platform {

}

/*
for event in platfor_events.iter() {
    match event {
        PlatformEvent::Idle => (),
        PlatformEvent::Key(key) => {
            editor.on_key(key);
        }
        PlatformEvent::
    }
}
*/
