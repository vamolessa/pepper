use std::{
    io::{self, Read, Write},
    path::Path,
    net::Shutdown,
};

#[cfg(target_os = "windows")]
use uds_windows::{UnixListener, UnixStream};

use bincode::Options;

use crate::{editor::EditorOperation, event::Key};

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TargetClient {
    All,
    Local,
    Remote(ConnectionWithClientHandle),
}

pub struct ConnectionWithClient(UnixStream, Vec<u8>);

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ConnectionWithClientHandle(usize);

pub struct ConnectionWithClientCollection {
    listener: UnixListener,
    connections: Vec<Option<ConnectionWithClient>>,
    error_indexes: Vec<usize>,
}

impl ConnectionWithClientCollection {
    pub fn listen<P>(path: P) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        Ok(Self {
            listener: UnixListener::bind(path)?,
            connections: Vec::new(),
            error_indexes: Vec::new(),
        })
    }

    pub fn accept_connection(&mut self) -> io::Result<ConnectionWithClientHandle> {
        let (stream, _address) = self.listener.accept()?;
        stream.set_nonblocking(true);
        let connection = ConnectionWithClient(stream, Vec::new());

        for (i, slot) in self.connections.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(connection);
                return Ok(ConnectionWithClientHandle(i));
            }
        }

        let handle = ConnectionWithClientHandle(self.connections.len());
        self.connections.push(Some(connection));
        Ok(handle)
    }

    pub fn close_connection(&mut self, handle: ConnectionWithClientHandle) {
        if let Some(connection) = self.connections[handle.0].take() {
            connection.0.shutdown(Shutdown::Both);
        }
    }

    fn serialize_operation(mut buf: &mut Vec<u8>, operation: &EditorOperation, content: &str) {
        let _ = bincode_serializer().serialize_into(&mut buf, operation);
        if let EditorOperation::Content = operation {
            let _ = bincode_serializer().serialize_into(&mut buf, content);
        }
    }

    pub fn queue_operation(
        &mut self,
        handle: ConnectionWithClientHandle,
        operation: &EditorOperation,
        content: &str,
    ) {
        if let Some(connection) = &mut self.connections[handle.0] {
            Self::serialize_operation(&mut connection.1, operation, content);
        }
    }

    pub fn queue_operation_all(&mut self, operation: &EditorOperation, content: &str) {
        for connection in self.connections.iter_mut().flatten() {
            Self::serialize_operation(&mut connection.1, operation, content);
        }
    }

    pub fn send_queued_operations(&mut self) {
        self.error_indexes.clear();
        for (i, connection) in self
            .connections
            .iter_mut()
            .enumerate()
            .flat_map(|(i, c)| c.as_mut().map(|c| (i, c)))
        {
            if connection.1.len() > 0 {
                if connection.0.write_all(&connection.1[..]).is_err() {
                    self.error_indexes.push(i);
                }
            }
        }

        let mut error_indexes = Vec::new();
        std::mem::swap(&mut self.error_indexes, &mut error_indexes);
        for i in error_indexes.drain(..) {
            self.close_connection(ConnectionWithClientHandle(i));
        }
        std::mem::swap(&mut self.error_indexes, &mut error_indexes);

        for connection in self.connections.iter_mut().flatten() {
            connection.1.clear();
        }
    }
}

/*
struct DeserializeRead<R>
where
    R: Read,
{
    reader: R,
    buf: Vec<u8>,
    len: usize,
    position: usize,
}

impl<R> DeserializeRead<R>
where
    R: Read,
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

    pub fn deserialize<T>(&mut self) -> Result<T, futures::io::Error>
    where
        T: serde::de::DeserializeOwned,
        R: Read,
    {
        loop {
            if self.position == self.len {
                if self.len == self.buf.len() {
                    self.buf.resize(self.buf.len() * 2, 0);
                }

                let reader = Pin::new(&mut self.reader);
                match reader.poll_read(ctx, &mut self.buf[self.len..]) {
                    Poll::Pending => break Poll::Pending,
                    Poll::Ready(Ok(byte_count)) => {
                        if byte_count == 0 {
                            break Poll::Ready(Err(futures::io::Error::new(
                                futures::io::ErrorKind::UnexpectedEof,
                                "",
                            )));
                        }

                        self.len += byte_count;
                    }
                    Poll::Ready(Err(error)) => break Poll::Ready(Err(error)),
                }
            }

            let mut cursor = io::Cursor::new(&mut self.buf[..self.len]);
            cursor.set_position(self.position as _);

            let deserializer = bincode_serializer().with_limit((self.len - self.position) as _);
            match deserializer.deserialize_from(&mut cursor) {
                Ok(value) => {
                    self.position = cursor.position() as _;
                    if self.position == self.len {
                        self.len = 0;
                        self.position = 0;
                    }

                    break Poll::Ready(Ok(value));
                }
                Err(error) => {
                    match error.as_ref() {
                        bincode::ErrorKind::SizeLimit => {
                            dbg!("SIZE LIMIT");
                            self.buf.resize(self.buf.len() * 2, 0);
                        }
                        _ => {
                            break Poll::Ready(Err(futures::io::Error::new(
                                futures::io::ErrorKind::Other,
                                error,
                            )))
                        }
                    };
                }
            }
        }
    }
}
*/

pub struct ConnectionWithServer(UnixStream);
impl ConnectionWithServer {
    pub fn connect<P>(path: P) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        let stream = UnixStream::connect(path)?;
        stream.set_nonblocking(true);
        Ok(Self(stream))
    }
}

pub fn bincode_serializer() -> impl Options {
    bincode::options()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}
