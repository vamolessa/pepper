use std::{
    io,
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};

use uds_windows::{UnixListener, UnixStream};

use futures::{
    future::TryFutureExt,
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    stream::{self, FusedStream, FuturesUnordered, SelectAll, Stream, StreamExt},
};
use smol::Async;

use crate::{editor::EditorOperation, event::Key};

pub struct ClientListener(Async<UnixListener>);
impl ClientListener {
    pub fn listen<P>(path: P) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        Ok(Self(Async::new(UnixListener::bind(path)?)?))
    }

    pub async fn accept(&self) -> io::Result<ConnectionWithClient> {
        let (stream, _address) = self.0.read_with(|l| l.accept()).await?;
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

    fn serialize_operation(mut buf: &mut Vec<u8>, operation: &EditorOperation, content: &str) {
        let _ = bincode::serialize_into(&mut buf, operation);
        if let EditorOperation::Content = operation {
            let _ = bincode::serialize_into(&mut buf, content);
        }
    }

    pub fn queue_operation(
        &mut self,
        handle: ConnectionWithClientHandle,
        operation: &EditorOperation,
        content: &str,
    ) {
        if let Some(writer) = &mut self.operation_writers[handle.0] {
            Self::serialize_operation(&mut writer.1, operation, content);
        }
    }

    pub fn queue_operation_all(&mut self, operation: &EditorOperation, content: &str) {
        for writer in self.operation_writers.iter_mut().flatten() {
            Self::serialize_operation(&mut writer.1, operation, content);
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

struct DeserializeRead<R>
where
    R: Unpin + AsyncRead,
{
    reader: R,
    buf: Vec<u8>,
    len: usize,
    position: usize,
}

impl<R> DeserializeRead<R>
where
    R: Unpin + AsyncRead,
{
    pub fn new(reader: R, capacity: usize) -> Self {
        let mut buf = Vec::with_capacity(capacity);
        buf.resize(capacity, 0);
        Self {
            reader,
            buf,
            len: 0,
            position: 0,
        }
    }

    pub fn poll_deserialize<T>(&mut self, ctx: &mut Context) -> Poll<Result<T, futures::io::Error>>
    where
        T: serde::de::DeserializeOwned,
        R: AsyncRead,
    {
        loop {
            if self.position == self.len {
                if self.len == self.buf.len() {
                    self.buf.resize(self.buf.len() * 2, 0);
                }

                let reader = Pin::new(&mut self.reader);
                match reader.poll_read(ctx, &mut self.buf[self.len..]) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(byte_count)) => {
                        self.len += byte_count;
                    }
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                }
            }

            let mut cursor = io::Cursor::new(&mut self.buf[..self.len]);
            cursor.set_position(self.position as _);
            match bincode::deserialize_from(&mut cursor) {
                Ok(value) => {
                    self.position = cursor.position() as _;
                    if self.position == self.len {
                        self.len = 0;
                        self.position = 0;
                    }

                    return Poll::Ready(Ok(value));
                }
                Err(error) => {
                    match error.as_ref() {
                        bincode::ErrorKind::Io(error) => match error.kind() {
                            io::ErrorKind::UnexpectedEof => {
                                self.buf.resize(self.buf.len() * 2, 0);
                                continue;
                            }
                            _ => (),
                        },
                        _ => (),
                    }
                    return Poll::Ready(Err(futures::io::Error::new(
                        futures::io::ErrorKind::Other,
                        error,
                    )));
                }
            }
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

    pub fn from_reader(
        reader: ClientKeyReader,
    ) -> impl Stream<Item = (ConnectionWithClientHandle, Key)> {
        let handle = reader.0;
        let mut reader = DeserializeRead::new(reader.1, 32);
        stream::poll_fn(move |ctx| match reader.poll_deserialize(ctx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(key)) => Poll::Ready(Some((handle, key))),
            Poll::Ready(Err(error)) => {
                dbg!(error);
                Poll::Ready(None)
            }
        })
    }
}

pub struct ConnectionWithServer(Async<UnixStream>);
impl ConnectionWithServer {
    pub fn connect<P>(path: P) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        Ok(Self(Async::new(UnixStream::connect(path)?)?))
    }

    pub fn split(self) -> (ServerOperationReader, ServerKeyWriter) {
        let (reader, writer) = self.0.split();
        (
            ServerOperationReader(reader),
            ServerKeyWriter(writer, Vec::new()),
        )
    }
}

pub struct ServerOperationReader(ReadHalf<Async<UnixStream>>);
impl ServerOperationReader {
    pub fn to_stream(self) -> impl FusedStream<Item = (EditorOperation, String)> {
        enum State {
            ReadingOperation,
            ReadingContent,
        }

        let mut reader = DeserializeRead::new(self.0, 8 * 1024);
        let mut state = State::ReadingOperation;
        stream::poll_fn(move |ctx| loop {
            match state {
                State::ReadingOperation => match reader.poll_deserialize(ctx) {
                    Poll::Pending => break Poll::Pending,
                    Poll::Ready(Ok(operation)) => match operation {
                        EditorOperation::Content => {
                            state = State::ReadingContent;
                            continue;
                        }
                        _ => break Poll::Ready(Some((operation, String::new()))),
                    },
                    Poll::Ready(Err(error)) => {
                        dbg!(error);
                        break Poll::Ready(None);
                    }
                },
                State::ReadingContent => match reader.poll_deserialize(ctx) {
                    Poll::Pending => break Poll::Pending,
                    Poll::Ready(Ok(content)) => {
                        state = State::ReadingOperation;
                        break Poll::Ready(Some((EditorOperation::Content, content)));
                    }
                    Poll::Ready(Err(error)) => {
                        dbg!(error);
                        break Poll::Ready(None);
                    }
                },
            }
        })
        .fuse()
    }
}

pub struct ServerKeyWriter(WriteHalf<Async<UnixStream>>, Vec<u8>);
impl ServerKeyWriter {
    pub async fn send(&mut self, key: Key) -> io::Result<()> {
        let _ = bincode::serialize_into(&mut self.1, &key);
        self.0.write_all(&self.1[..]).await?;
        self.1.clear();
        Ok(())
    }
}
