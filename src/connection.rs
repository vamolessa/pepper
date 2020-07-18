use std::{io, mem, path::Path, pin::Pin, task::Poll};

use uds_windows::{UnixListener, UnixStream};

use futures::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufWriter, ReadHalf, WriteHalf},
    stream::{self, FuturesUnordered, SelectAll, Stream, StreamExt},
};
use smol::Async;

use crate::{editor::EditorOperation, event::Key};

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
pub struct ClientOperationWriter(BufWriter<WriteHalf<Async<UnixStream>>>);
pub struct ConnectionWithServer;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ConnectionWithClientHandle(usize);

pub struct ConnectionWithClientCollection {
    operation_writers: Vec<Option<ClientOperationWriter>>,
    serialization_buf: Vec<u8>,
}

impl ConnectionWithClientCollection {
    pub fn new() -> Self {
        Self {
            operation_writers: Vec::new(),
            serialization_buf: Vec::new(),
        }
    }

    pub fn add_and_get_reader(&mut self, connection: ConnectionWithClient) -> ClientKeyReader {
        let (reader, writer) = connection.0.split();
        let writer = ClientOperationWriter(BufWriter::new(writer));

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

    pub async fn send_operation(
        &mut self,
        handle: ConnectionWithClientHandle,
        operation: &EditorOperation,
    ) -> Result<(), ConnectionWithClientHandle> {
        if let Some(writer) = &mut self.operation_writers[handle.0] {
            self.serialization_buf.clear();
            if let Err(_) = bincode::serialize_into(&mut self.serialization_buf, operation) {
                return Err(handle);
            }

            if let Err(_) = writer.0.write_all(&self.serialization_buf[..]).await {
                return Err(handle);
            }
        }

        Ok(())
    }

    pub async fn send_operation_to_all(
        &mut self,
        operation: &EditorOperation,
    ) -> Result<(), Option<ConnectionWithClientHandle>> {
        self.serialization_buf.clear();
        if let Err(_) = bincode::serialize_into(&mut self.serialization_buf, operation) {
            return Err(None);
        }

        for (i, writer) in self
            .operation_writers
            .iter_mut()
            .enumerate()
            .flat_map(|(i, w)| w.as_mut().map(|w| (i, w)))
        {
            if let Err(_) = writer.0.write_all(&self.serialization_buf[..]).await {
                return Err(Some(ConnectionWithClientHandle(i)));
            }
        }

        Ok(())
    }

    pub async fn flush_all(&mut self) {
        let mut futures = FuturesUnordered::new();
        for writer in self.operation_writers.iter_mut().flatten() {
            futures.push(writer.0.flush());
        }
        while futures.next().await.is_some() {}
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
