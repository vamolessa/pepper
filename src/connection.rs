use std::{io, mem, path::Path, pin::Pin, task::Poll};

use uds_windows::{UnixListener, UnixStream};

use futures::{
    future::TryFutureExt,
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
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
pub struct ClientOperationWriter(WriteHalf<Async<UnixStream>>, Vec<u8>);

pub struct ConnectionWithServer;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ConnectionWithClientHandle(usize);

pub struct ConnectionWithClientCollection {
    operation_writers: Vec<Option<ClientOperationWriter>>,
    error_indexes: Vec<usize>,
}

impl ConnectionWithClientCollection {
    pub fn new() -> Self {
        Self {
            operation_writers: Vec::new(),
            error_indexes: Vec::new(),
        }
    }

    pub fn add_and_get_reader(&mut self, connection: ConnectionWithClient) -> ClientKeyReader {
        let (reader, writer) = connection.0.split();
        let writer = ClientOperationWriter(writer, Vec::new());

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

    pub fn queue_operation(
        &mut self,
        handle: ConnectionWithClientHandle,
        operation: &EditorOperation,
    ) {
        if let Some(writer) = &mut self.operation_writers[handle.0] {
            let _ = bincode::serialize_into(&mut writer.1, operation);
        }
    }

    pub fn queue_operation_all(&mut self, operation: &EditorOperation) {
        for writer in self.operation_writers.iter_mut().flatten() {
            let _ = bincode::serialize_into(&mut writer.1, operation);
        }
    }

    pub async fn send_queued_operations(&mut self) {
        let mut futures = FuturesUnordered::new();
        for (i, writer) in self
            .operation_writers
            .iter_mut()
            .enumerate()
            .flat_map(|(i, w)| w.as_mut().map(|w| (i, w)))
        {
            if writer.1.len() > 0 {
                let future = writer.0.write_all(&writer.1[..]).map_err(move |_| i);
                futures.push(future);
            }
        }

        self.error_indexes.clear();
        loop {
            match futures.next().await {
                Some(Ok(_)) => (),
                Some(Err(i)) => self.error_indexes.push(i),
                None => break,
            }
        }

        drop(futures);
        for i in &self.error_indexes {
            self.operation_writers[*i] = None;
        }
        for writer in self.operation_writers.iter_mut().flatten() {
            writer.1.clear();
        }
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
