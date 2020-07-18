use std::{io, mem, path::Path, pin::Pin, task::Poll};

use uds_windows::{UnixListener, UnixStream};

use futures::{
    io::{AsyncRead, AsyncReadExt, BufReader, ReadHalf},
    stream::{self, FusedStream, StreamExt},
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

pub struct ConnectionWithClientCollection {
    next_connection_handle: ConnectionWithClientHandle,
    connections: Vec<Option<ConnectionWithClient>>,
}

impl ConnectionWithClientCollection {
    pub fn new() -> Self {
        Self {
            next_connection_handle: ConnectionWithClientHandle(0),
            connections: Vec::new(),
        }
    }

    pub fn add(&mut self, connection: ConnectionWithClient) -> ConnectionWithClientHandle {
        self.next_connection_handle

        //if let Some(handle) = self.free_slots.pop() {
        //    self.connections[handle.0] = Some(RefCell::new(connection));
        //    handle
        //} else {
        //    let index = self.connections.len();
        //    self.connections.push(Some(RefCell::new(connection)));
        //    ConnectionWithClientHandle(index)
        //}
    }
}

pub fn client_key_stream(
    handle: ConnectionWithClientHandle,
    reader: ReadHalf<Async<UnixStream>>,
) -> impl FusedStream<Item = (ConnectionWithClientHandle, Key)> {
    //let mut reader = BufReader::with_capacity(512, reader);
    let mut reader = reader;
    stream::poll_fn(move |ctx| {
        let reader = Pin::new(&mut reader);
        let mut buf = [0; mem::size_of::<Key>()];
        match reader.poll_read(ctx, &mut buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(byte_count)) => match bincode::deserialize(&buf[..byte_count]) {
                Ok(key) => Poll::Ready(Some((handle, key))),
                Err(_) => Poll::Ready(None),
            },
            Poll::Ready(Err(_)) => Poll::Ready(None),
        }
    })
    .fuse()
}
