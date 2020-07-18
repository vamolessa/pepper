use std::{
    cell::{RefCell, RefMut},
    future::Future,
    io::{self, Read, Write},
    path::Path,
    pin::Pin,
    task::Poll,
};

use uds_windows::{UnixListener, UnixStream};

use futures::{
    future::FusedFuture,
    io::{AsyncRead, AsyncReadExt, ReadHalf},
    pin_mut,
    stream::{self, FusedStream, FuturesUnordered, StreamExt},
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
        Ok(ConnectionWithClient::new(stream))
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TargetClient {
    All,
    Local,
    Remote(ConnectionWithClientHandle),
}

pub struct ConnectionWithClient {
    stream: Async<UnixStream>,
}

impl ConnectionWithClient {
    fn new(stream: Async<UnixStream>) -> Self {
        Self { stream }
    }

    pub async fn read_key(
        &mut self,
        handle: ConnectionWithClientHandle,
    ) -> io::Result<(ConnectionWithClientHandle, Key)> {
        let mut buf = [0; 256];
        let _byte_count = self.stream.read_with_mut(|s| s.read(&mut buf)).await?;
        Ok((handle, Key::None))
    }
}

pub struct ConnectionWithServer;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ConnectionWithClientHandle(usize);

#[derive(Default)]
pub struct ConnectionWithClientCollection {
    connections: Vec<Option<RefCell<ConnectionWithClient>>>,
    free_slots: Vec<ConnectionWithClientHandle>,
}

impl ConnectionWithClientCollection {
    pub fn add(&mut self, connection: ConnectionWithClient) -> ConnectionWithClientHandle {
        if let Some(handle) = self.free_slots.pop() {
            self.connections[handle.0] = Some(RefCell::new(connection));
            handle
        } else {
            let index = self.connections.len();
            self.connections.push(Some(RefCell::new(connection)));
            ConnectionWithClientHandle(index)
        }
    }

    pub fn remove(&mut self, handle: ConnectionWithClientHandle) {
        self.connections[handle.0] = None;
        self.free_slots.push(handle);
    }

    pub fn get(&self, handle: ConnectionWithClientHandle) -> Option<RefMut<ConnectionWithClient>> {
        if let Some(connection) = &self.connections[handle.0] {
            if let Ok(connection) = connection.try_borrow_mut() {
                return Some(connection);
            }
        }

        None
    }
}

pub fn client_stream(
    handle: ConnectionWithClientHandle,
    mut reader: ReadHalf<Async<UnixStream>>,
) -> impl FusedStream<Item = (ConnectionWithClientHandle, Key)> {
    stream::poll_fn(move |ctx| {
        let reader = Pin::new(&mut reader);
        let mut buf = [0; 128];
        match reader.poll_read(ctx, &mut buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(_byte_count)) => Poll::Ready(Some((handle, Key::None))),
            Poll::Ready(Err(_)) => Poll::Ready(None),
        }
    })
    .fuse()
}
