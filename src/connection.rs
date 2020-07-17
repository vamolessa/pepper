use std::{
    future::Future,
    io::{self, Read, Write},
    path::Path,
    pin::Pin,
};

use uds_windows::{UnixListener, UnixStream};

use futures::{
    stream::{FuturesUnordered, Stream},
    task::{Context, Poll},
};
use smol::Async;

use crate::event::Key;

pub struct ClientListener {
    listener: Async<UnixListener>,
}

impl ClientListener {
    pub fn listen<P>(path: P) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        Ok(Self {
            listener: Async::new(UnixListener::bind(path)?)?,
        })
    }

    pub async fn accept(&self) -> io::Result<ConnectionWithClient> {
        let (stream, _address) = self.listener.read_with(|l| l.accept()).await?;
        let stream = Async::new(stream)?;
        Ok(ConnectionWithClient(stream))
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TargetClient {
    All,
    Local,
    Remote(ConnectionWithClientHandle),
}

pub struct ConnectionWithClient(Async<UnixStream>);

pub struct ConnectionWithServer;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ConnectionWithClientHandle(usize);

#[derive(Default)]
pub struct ConnectionWithClientCollection {
    connections: Vec<Option<ConnectionWithClient>>,
    free_slots: Vec<ConnectionWithClientHandle>,
}

impl ConnectionWithClientCollection {
    pub fn add(&mut self, connection: ConnectionWithClient) -> ConnectionWithClientHandle {
        if let Some(handle) = self.free_slots.pop() {
            self.connections[handle.0] = Some(connection);
            handle
        } else {
            let index = self.connections.len();
            self.connections.push(Some(connection));
            ConnectionWithClientHandle(index)
        }
    }

    pub fn remove(&mut self, handle: ConnectionWithClientHandle) {
        self.connections[handle.0] = None;
        self.free_slots.push(handle);
    }

    pub fn get(&self, handle: ConnectionWithClientHandle) -> Option<&ConnectionWithClient> {
        self.connections[handle.0].as_ref()
    }
}
