use std::{
    io,
    sync::{mpsc, Arc},
    thread,
};

use crate::event::Event;

#[derive(Debug, Clone, Copy)]
pub struct StreamId(pub usize);

#[derive(Debug, Clone, Copy)]
pub enum ConnectionEvent {
    NewConnection,
    Stream(StreamId),
}

impl ConnectionEvent {
    fn from_raw_id(id: usize) -> Self {
        match id {
            0 => ConnectionEvent::NewConnection,
            id => ConnectionEvent::Stream(StreamId(id - 1)),
        }
    }

    fn raw_id(&self) -> usize {
        match self {
            ConnectionEvent::NewConnection => 0,
            ConnectionEvent::Stream(id) => id.0 + 1,
        }
    }
}

#[cfg(target_os = "windows")]
use uds_windows::{UnixListener, UnixStream};

pub struct EventManager {
    poller: Arc<polling::Poller>,
}

impl EventManager {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            poller: Arc::new(polling::Poller::new()?),
        })
    }

    pub fn run_event_loop_in_background(
        self,
        event_sender: mpsc::Sender<Event>,
    ) -> thread::JoinHandle<io::Result<()>> {
        let mut events = Vec::new();
        thread::spawn(move || 'event_loop: loop {
            self.poller.wait(&mut events, None)?;
            for event in &events {
                let event = ConnectionEvent::from_raw_id(event.key);
                if event_sender.send(Event::Connection(event)).is_err() {
                    break 'event_loop Ok(());
                }
            }

            events.clear();
        })
    }

    pub fn registry(&self) -> EventRegistry {
        EventRegistry {
            poller: self.poller.clone(),
        }
    }
}

pub struct EventRegistry {
    poller: Arc<polling::Poller>,
}

impl EventRegistry {
    pub fn register_listener(&self, listener: &UnixListener) -> io::Result<()> {
        self.poller.insert(listener)?;
        self.listen_next_listener_event(listener)?;
        Ok(())
    }

    pub fn register_stream(&self, stream: &UnixStream, id: StreamId) -> io::Result<()> {
        self.poller.insert(stream)?;
        self.listen_next_stream_event(stream, id)?;
        Ok(())
    }

    pub fn listen_next_listener_event(&self, listener: &UnixListener) -> io::Result<()> {
        let id = ConnectionEvent::NewConnection.raw_id();
        self.poller.interest(listener, polling::Event::readable(id))
    }

    pub fn listen_next_stream_event(&self, stream: &UnixStream, id: StreamId) -> io::Result<()> {
        let id = ConnectionEvent::Stream(id).raw_id();
        self.poller.interest(stream, polling::Event::readable(id))
    }

    pub fn unregister_stream(&self, stream: &UnixStream) -> io::Result<()> {
        self.poller.remove(stream)
    }
}
