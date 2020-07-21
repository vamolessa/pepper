use std::{io, thread};

use crate::event::Event;

#[derive(Debug, Clone, Copy)]
pub enum StreamId {
    Listener,
    Stream(usize),
}

impl StreamId {
    fn from_raw_id(id: u64) -> Self {
        match id {
            0 => StreamId::Listener,
            id => StreamId::Stream(id as usize - 1),
        }
    }

    fn raw_id(&self) -> u64 {
        match self {
            StreamId::Listener => 0,
            StreamId::Stream(id) => *id as u64 + 1,
        }
    }
}

pub fn run_event_loop(mut event_manager: EventManager) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        while event_manager.poll()? {}
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
            self.poll
                .register(listener, EventFlag::IN, StreamId::Listener.raw_id())
        }

        pub fn register_stream(&mut self, stream: &UnixStream, id: usize) -> io::Result<()> {
            self.poll
                .register(stream, EventFlag::IN, StreamId::Stream(id).raw_id())
        }

        pub fn unregister_listener(&mut self, listener: &UnixListener) -> io::Result<()> {
            self.poll.deregister(listener)
        }

        pub fn unregister_stream(&mut self, stream: &UnixStream) -> io::Result<()> {
            self.poll.deregister(stream)
        }

        pub fn poll(&mut self) -> io::Result<bool> {
            self.poll.poll(&mut self.events, None)?;
            for event in self.events.iter() {
                if self
                    .event_sender
                    .send(Event::Stream(StreamId::from_raw_id(event.data())))
                    .is_err()
                {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}
