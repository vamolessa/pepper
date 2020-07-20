pub use windows::*;

pub enum StreamId {
    Listener,
    Stream(usize),
}

impl StreamId {
    fn from_raw_id(id: u64) -> Self {
        match id {
            0 => StreamId::Listener,
            id => StreamId::Stream(id as _),
        }
    }

    fn raw_id(&self) -> u64 {
        match self {
            StreamId::Listener => 0,
            StreamId::Stream(id) => *id as u64 + 1,
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{io, os::windows::io::AsRawSocket, sync::mpsc};

    use wepoll_binding::{Epoll, EventFlag, Events};

    use super::*;

    pub struct EventManager {
        poll: Epoll,
        events: Events,
        event_sender: mpsc::Sender<StreamId>,
    }

    impl EventManager {
        pub fn new(
            event_sender: mpsc::Sender<StreamId>,
            capacity: usize,
        ) -> io::Result<Self> {
            Ok(Self {
                poll: Epoll::new()?,
                events: Events::with_capacity(capacity),
                event_sender,
            })
        }

        pub fn register<S>(&mut self, socket: &S, id: StreamId) -> io::Result<()>
        where
            S: AsRawSocket,
        {
            self.poll.register(socket, EventFlag::IN, id.raw_id())
        }

        pub fn unregister<S>(&mut self, socket: &S) -> io::Result<()>
        where
            S: AsRawSocket,
        {
            self.poll.deregister(socket)
        }

        pub fn poll(&mut self) -> io::Result<()> {
            self.poll.poll(&mut self.events, None)?;
            for event in self.events.iter() {
                if let Err(e) = self.event_sender.send(StreamId::from_raw_id(event.data())) {
                    return Err(io::Error::new(io::ErrorKind::Other, e));
                }
            }
            Ok(())
        }
    }
}
