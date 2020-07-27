use std::{
    sync::{Arc, Barrier},
    thread,
};

use crate::event::Event;

#[derive(Debug, Clone, Copy)]
pub struct StreamId(pub usize);

#[derive(Debug, Clone, Copy)]
pub enum EventResult {
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy)]
pub enum ConnectionEvent {
    NewConnection(EventResult),
    Stream(StreamId, EventResult),
}

impl ConnectionEvent {
    fn from_raw_id(id: u64, result: EventResult) -> Self {
        match id {
            0 => ConnectionEvent::NewConnection(result),
            id => ConnectionEvent::Stream(StreamId(id as usize - 1), result),
        }
    }

    fn raw_id(&self) -> u64 {
        match self {
            ConnectionEvent::NewConnection(_) => 0,
            ConnectionEvent::Stream(id, _) => id.0 as u64 + 1,
        }
    }
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
        poll: Arc<Epoll>,
        events: Events,
        event_sender: mpsc::Sender<Event>,
    }

    impl EventManager {
        pub fn new(event_sender: mpsc::Sender<Event>, capacity: usize) -> io::Result<Self> {
            Ok(Self {
                poll: Arc::new(Epoll::new()?),
                events: Events::with_capacity(capacity),
                event_sender,
            })
        }

        pub fn registry(&self) -> io::Result<EventRegistry> {
            Ok(EventRegistry {
                poll: self.poll.clone(),
            })
        }

        pub fn run_event_loop_in_background(
            mut self,
            barrier: Arc<Barrier>,
        ) -> thread::JoinHandle<io::Result<()>> {
            thread::spawn(move || {
                while self.poll_and_send_events()? {
                    barrier.wait();
                }
                Ok(())
            })
        }

        fn poll_and_send_events(&mut self) -> io::Result<bool> {
            self.poll.poll(&mut self.events, None)?;
            for event in self.events.iter() {
                let result = match (event.flags() & !EventFlag::IN).is_empty() {
                    true => EventResult::Ok,
                    false => EventResult::Error,
                };
                let event = ConnectionEvent::from_raw_id(event.data(), result);
                if self.event_sender.send(Event::Connection(event)).is_err() {
                    return Ok(false);
                }
            }
            self.events.clear();
            Ok(true)
        }
    }

    pub struct EventRegistry {
        poll: Arc<Epoll>,
    }

    impl EventRegistry {
        pub fn register_listener(&self, listener: &UnixListener) -> io::Result<()> {
            self.poll.register(
                listener,
                EventFlag::IN | EventFlag::ERR,
                ConnectionEvent::NewConnection(EventResult::Ok).raw_id(),
            )
        }

        pub fn register_stream(&self, stream: &UnixStream, id: StreamId) -> io::Result<()> {
            self.poll.register(
                stream,
                EventFlag::IN | EventFlag::RDHUP | EventFlag::HUP | EventFlag::ERR,
                ConnectionEvent::Stream(id, EventResult::Ok).raw_id(),
            )
        }

        pub fn unregister_stream(&self, stream: &UnixStream) -> io::Result<()> {
            self.poll.deregister(stream)
        }
    }
}
