use std::{
    io,
    sync::{Arc, Barrier, Mutex},
    thread,
};

use crate::event::Event;

#[derive(Debug, Clone, Copy)]
pub struct StreamId(pub usize);

#[derive(Debug, Clone, Copy)]
pub enum ConnectionEvent {
    NewConnection,
    StreamIn(StreamId),
    StreamError(StreamId),
}

impl ConnectionEvent {
    fn from_raw_id(id: u64, success: bool) -> Self {
        match (id, success) {
            (0, _) => ConnectionEvent::NewConnection,
            (id, true) => ConnectionEvent::StreamIn(StreamId(id as usize - 1)),
            (id, false) => ConnectionEvent::StreamError(StreamId(id as usize - 1)),
        }
    }

    fn raw_id(&self) -> u64 {
        match self {
            ConnectionEvent::NewConnection => 0,
            ConnectionEvent::StreamIn(id) | ConnectionEvent::StreamError(id) => id.0 as u64 + 1,
        }
    }
}

pub fn run_event_loop(
    event_manager: Arc<Mutex<EventManager>>,
    barrier: Arc<Barrier>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        while event_manager.lock().unwrap().poll()? {
            barrier.wait();
        }
        Ok(())
    })
}

#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "windows")]
mod windows {
    use std::{io, sync::mpsc};

    use uds_windows::{UnixListener, UnixStream};
    use wepoll_binding::{Epoll, EventFlag, Events};

    use super::*;

    pub struct EventManager {
        poll: Epoll,
        events: Events,
        event_sender: mpsc::Sender<Event>,
    }

    impl EventManager {
        pub fn new(event_sender: mpsc::Sender<Event>, capacity: usize) -> io::Result<Self> {
            Ok(Self {
                poll: Epoll::new()?,
                events: Events::with_capacity(capacity),
                event_sender,
            })
        }

        pub fn register_listener(&mut self, listener: &UnixListener) -> io::Result<()> {
            self.poll.register(
                listener,
                EventFlag::IN | EventFlag::ERR,
                ConnectionEvent::NewConnection.raw_id(),
            )
        }

        pub fn register_stream(&mut self, stream: &UnixStream, id: StreamId) -> io::Result<()> {
            self.poll.register(
                stream,
                EventFlag::IN | EventFlag::RDHUP | EventFlag::HUP | EventFlag::ERR,
                ConnectionEvent::StreamIn(id).raw_id(),
            )
        }

        pub fn unregister_stream(&mut self, stream: &UnixStream) -> io::Result<()> {
            self.poll.deregister(stream)
        }

        pub fn poll(&mut self) -> io::Result<bool> {
            self.poll.poll(&mut self.events, None)?;
            for event in self.events.iter() {
                let success = (event.flags() & !EventFlag::IN).is_empty();
                let event = ConnectionEvent::from_raw_id(event.data(), success);
                if self.event_sender.send(Event::Connection(event)).is_err() {
                    return Ok(false);
                }
            }
            self.events.clear();
            Ok(true)
        }
    }
}
