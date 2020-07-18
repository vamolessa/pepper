use std::{io, mem, path::Path, pin::Pin, task::Poll};

use uds_windows::{UnixListener, UnixStream};

use futures::{
    io::{AsyncRead, AsyncReadExt, ReadHalf, WriteHalf},
    stream::{self, SelectAll, Stream},
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
pub struct ClientKeyReader(ConnectionWithClientHandle, ReadHalf<Async<UnixStream>>);
pub struct ClientOperationWriter(WriteHalf<Async<UnixStream>>);
pub struct ConnectionWithServer;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ConnectionWithClientHandle(usize);

pub struct ConnectionWithClientCollection {
    operation_writers: Vec<Option<ClientOperationWriter>>,
}

impl ConnectionWithClientCollection {
    pub fn new() -> Self {
        Self {
            operation_writers: Vec::new(),
        }
    }

    pub fn add_and_get_reader(
        &mut self,
        connection: ConnectionWithClient,
    ) -> ClientKeyReader {
        let (reader, writer) = connection.0.split();
        let writer = ClientOperationWriter(writer);

        for (i, slot) in self.operation_writers.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(writer);
                let handle = ConnectionWithClientHandle(i);
                return ClientKeyReader(handle, reader);
            }
        }

        let handle = ConnectionWithClientHandle(self.operation_writers.len());
        self.operation_writers.push(Some(writer));

        ClientKeyReader(handle, reader)
    }
}

pub struct ClientKeyStreams;
impl ClientKeyStreams {
    pub fn new<S>() -> SelectAll<S>
    where
        S: Unpin + Stream<Item = (ConnectionWithClientHandle, Key)>,
    {
        SelectAll::new()
    }

    pub fn stream_from_reader(
        reader: ClientKeyReader,
    ) -> impl Stream<Item = (ConnectionWithClientHandle, Key)> {
        //let mut reader = BufReader::with_capacity(512, reader);
        let mut reader = reader;
        stream::poll_fn(move |ctx| {
            let r = Pin::new(&mut reader.1);
            let mut buf = [0; mem::size_of::<Key>()];
            match r.poll_read(ctx, &mut buf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(byte_count)) => match bincode::deserialize(&buf[..byte_count]) {
                    Ok(key) => Poll::Ready(Some((reader.0, key))),
                    Err(_) => Poll::Ready(None),
                },
                Poll::Ready(Err(_)) => Poll::Ready(None),
            }
        })
    }
}
