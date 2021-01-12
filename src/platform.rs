// TODO: move Key here
use crate::client_event::Key;

pub enum PlatformEvent {
    Idle,
    Close,
    Key(Key),
}
