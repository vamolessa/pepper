use std::{
    convert::Into,
    io::{self, Cursor, Read, Write},
    net::Shutdown,
    path::Path,
};

#[cfg(target_os = "windows")]
use uds_windows::{UnixListener, UnixStream};

use bincode::Options;

use crate::{
    editor::EditorOperation,
    event::Key,
    event_manager::{EventRegistry, StreamId},
};

struct ReadBuf {
    buf: Vec<u8>,
    len: usize,
    position: usize,
}

impl ReadBuf {
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(2 * 1024);
        buf.resize(buf.capacity(), 0);
        Self {
            buf,
            len: 0,
            position: 0,
        }
    }

    pub fn slice(&self) -> &[u8] {
        &self.buf[self.position..self.len]
    }

    pub fn seek(&mut self, offset: usize) {
        self.position += offset;
        if self.position == self.len {
            self.len = 0;
            self.position = 0;
        }
    }

    pub fn read_into<R>(&mut self, mut reader: R) -> io::Result<usize>
    where
        R: Read,
    {
        let mut total_bytes = 0;
        loop {
            match reader.read(&mut self.buf[self.len..]) {
                Ok(len) => {
                    total_bytes += len;
                    self.len += len;

                    if self.len < self.buf.len() {
                        break;
                    }

                    self.buf.resize(self.buf.len() * 2, 0);
                }
                Err(e) => match e.kind() {
                    io::ErrorKind::WouldBlock => break,
                    _ => return Err(e),
                },
            }
        }

        Ok(total_bytes)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TargetClient {
    All,
    Local,
    Remote(ConnectionWithClientHandle),
}

pub struct ConnectionWithClient {
    stream: UnixStream,
    write_buf: Vec<u8>,
    read_buf: ReadBuf,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ConnectionWithClientHandle(usize);
impl Into<ConnectionWithClientHandle> for StreamId {
    fn into(self) -> ConnectionWithClientHandle {
        ConnectionWithClientHandle(self.0)
    }
}
impl Into<StreamId> for ConnectionWithClientHandle {
    fn into(self) -> StreamId {
        StreamId(self.0)
    }
}

pub struct ConnectionWithClientCollection {
    listener: UnixListener,
    connections: Vec<Option<ConnectionWithClient>>,
    closed_connection_indexes: Vec<usize>,
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
            closed_connection_indexes: Vec::new(),
        })
    }

    pub fn register_listener(&self, event_registry: &EventRegistry) -> io::Result<()> {
        event_registry.register_listener(&self.listener)
    }

    pub fn accept_connection(
        &mut self,
        event_registry: &EventRegistry,
    ) -> io::Result<ConnectionWithClientHandle> {
        let (stream, _address) = self.listener.accept()?;
        stream.set_nonblocking(true)?;
        let connection = ConnectionWithClient {
            stream,
            write_buf: Vec::with_capacity(8 * 1024),
            read_buf: ReadBuf::new(),
        };

        for (i, slot) in self.connections.iter_mut().enumerate() {
            if slot.is_none() {
                let handle = ConnectionWithClientHandle(i);
                event_registry.register_stream(&connection.stream, handle.into())?;
                *slot = Some(connection);
                return Ok(handle);
            }
        }

        let handle = ConnectionWithClientHandle(self.connections.len());
        event_registry.register_stream(&connection.stream, handle.into())?;
        self.connections.push(Some(connection));
        Ok(handle)
    }

    pub fn close_connection(&mut self, handle: ConnectionWithClientHandle) {
        if let Some(connection) = &self.connections[handle.0] {
            let _ = &connection.stream.shutdown(Shutdown::Both);
            self.closed_connection_indexes.push(handle.0);
        }
    }

    pub fn close_all_connections(&mut self) {
        for connection in self.connections.iter().flatten() {
            let _ = &connection.stream.shutdown(Shutdown::Both);
        }
    }

    pub fn unregister_closed_connections(
        &mut self,
        event_registry: &EventRegistry,
    ) -> io::Result<()> {
        for i in self.closed_connection_indexes.drain(..) {
            if let Some(connection) = self.connections[i].take() {
                event_registry.unregister_stream(&connection.stream)?;
            }
        }

        Ok(())
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
            Self::serialize_operation(&mut connection.write_buf, operation, content);
        }
    }

    pub fn queue_operation_all(&mut self, operation: &EditorOperation, content: &str) {
        for connection in self.connections.iter_mut().flatten() {
            Self::serialize_operation(&mut connection.write_buf, operation, content);
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
            let write_slice = &connection.write_buf[..];
            if write_slice.len() > 0 {
                if connection.stream.write_all(write_slice).is_err() {
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
            connection.write_buf.clear();
        }
    }

    pub fn receive_key(&mut self, handle: ConnectionWithClientHandle) -> io::Result<Option<Key>> {
        match &mut self.connections[handle.0] {
            Some(connection) => deserialize(&mut connection.stream, &mut connection.read_buf),
            None => Ok(None),
        }
    }
}

pub struct ConnectionWithServer {
    stream: UnixStream,
    read_buf: ReadBuf,
}

impl ConnectionWithServer {
    pub fn connect<P>(path: P) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        let stream = UnixStream::connect(path)?;
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            read_buf: ReadBuf::new(),
        })
    }

    pub fn close(&self) {
        let _ = &self.stream.shutdown(Shutdown::Both);
    }

    pub fn register_connection(&self, event_registry: &EventRegistry) -> io::Result<()> {
        event_registry.register_stream(&self.stream, StreamId(0))
    }

    pub fn send_key(&mut self, key: Key) -> io::Result<()> {
        match bincode_serializer().serialize_into(&mut self.stream, &key) {
            Ok(()) => Ok(()),
            Err(error) => Err(io::Error::new(io::ErrorKind::Other, error)),
        }
    }

    pub fn receive_operation(&mut self) -> io::Result<Option<(EditorOperation, String)>> {
        match deserialize(&mut self.stream, &mut self.read_buf)? {
            None => Ok(None),
            Some(EditorOperation::Content) => {
                match deserialize(&mut self.stream, &mut self.read_buf)? {
                    Some(content) => Ok(Some((EditorOperation::Content, content))),
                    None => Ok(None),
                }
            }
            Some(operation) => Ok(Some((operation, String::new()))),
        }
    }
}

fn bincode_serializer() -> impl Options {
    bincode::options()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

fn deserialize<T>(mut reader: &mut UnixStream, buf: &mut ReadBuf) -> io::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    loop {
        let slice = buf.slice();
        let deserializer = bincode_serializer().with_limit(slice.len() as _);
        let mut cursor = Cursor::new(slice);
        match deserializer.deserialize_from(&mut cursor) {
            Ok(value) => {
                let position = cursor.position() as _;
                buf.seek(position);
                break Ok(Some(value));
            }
            Err(error) => match error.as_ref() {
                bincode::ErrorKind::SizeLimit => (),
                _ => break Err(io::Error::new(io::ErrorKind::Other, error)),
            },
        }

        if buf.read_into(&mut reader)? == 0 {
            break Ok(None);
        }
    }
}
