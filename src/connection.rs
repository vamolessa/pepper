use std::{
    convert::Into,
    io::{self, Read, Write},
    net::Shutdown,
    path::Path,
};

#[cfg(target_os = "windows")]
use uds_windows::{UnixListener, UnixStream};

use bincode::Options;

use crate::{
    editor::EditorOperation,
    event::Key,
    event_manager::{EventManager, StreamId},
};

pub struct RawBuf {
    buf: Vec<u8>,
    len: usize,
}
impl RawBuf {
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(1024);
        buf.resize(buf.capacity(), 0);
        Self { buf, len: 0 }
    }

    pub fn set_len(&mut self, len: usize) {
        self.len = len;
    }

    pub fn read_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn write_slice(&mut self) -> &mut [u8] {
        &mut self.buf[self.len..]
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
    read_buf: RawBuf,
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

    pub fn register_listener(&self, event_manager: &mut EventManager) -> io::Result<()> {
        event_manager.register_listener(&self.listener)
    }

    pub fn accept_connection(
        &mut self,
        event_manager: &mut EventManager,
    ) -> io::Result<ConnectionWithClientHandle> {
        let (stream, _address) = self.listener.accept()?;
        stream.set_nonblocking(true)?;
        let connection = ConnectionWithClient {
            stream,
            write_buf: Vec::with_capacity(8 * 1024),
            read_buf: RawBuf::new(),
        };

        for (i, slot) in self.connections.iter_mut().enumerate() {
            if slot.is_none() {
                let handle = ConnectionWithClientHandle(i);
                event_manager.register_stream(&connection.stream, handle.into())?;
                *slot = Some(connection);
                return Ok(handle);
            }
        }

        let handle = ConnectionWithClientHandle(self.connections.len());
        event_manager.register_stream(&connection.stream, handle.into())?;
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
        event_manager: &mut EventManager,
    ) -> io::Result<()> {
        for i in self.closed_connection_indexes.drain(..) {
            if let Some(connection) = self.connections[i].take() {
                event_manager.unregister_stream(&connection.stream)?;
            }
        }

        Ok(())
    }

    fn serialize_operation(buf: &mut Vec<u8>, operation: &EditorOperation, content: &str) {
        let _ = bincode_serializer().serialize_into(buf, operation);
        if let EditorOperation::Content = operation {
            //let _ = bincode_serializer().serialize_into(&mut buf, content);
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
    read_buf: RawBuf,
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
            read_buf: RawBuf::new(),
        })
    }

    pub fn close(&self) {
        let _ = &self.stream.shutdown(Shutdown::Both);
    }

    pub fn register_connection(&self, event_manager: &mut EventManager) -> io::Result<()> {
        event_manager.register_stream(&self.stream, StreamId(0))
    }

    pub fn send_key(&mut self, key: Key) -> io::Result<()> {
        match bincode_serializer().serialize_into(&mut self.stream, &key) {
            Ok(()) => Ok(()),
            Err(error) => Err(io::Error::new(io::ErrorKind::Other, error)),
        }
    }

    pub fn receive_operation(&mut self) -> io::Result<Option<EditorOperation>> {
        deserialize(&mut self.stream, &mut self.read_buf)
    }
}

fn bincode_serializer() -> impl Options {
    bincode::options()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

fn deserialize<T>(reader: &mut UnixStream, buf: &mut RawBuf) -> io::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    let start_index = buf.read_slice().len();
    match reader.read(buf.write_slice()) {
        Ok(len) => buf.set_len(start_index + len),
        Err(e) => match e.kind() {
            io::ErrorKind::WouldBlock => {
                buf.set_len(0);
                return Ok(None);
            }
            _ => {
                dbg!(&e);
                return Err(e);
            }
        },
    }

    let read_slice = buf.read_slice();
    if read_slice.len() == 0 {
        return Ok(None);
    }

    let deserializer = bincode_serializer().with_limit(read_slice.len() as _);
    match deserializer.deserialize_from(read_slice) {
        Ok(value) => Ok(Some(value)),
        Err(error) => match error.as_ref() {
            bincode::ErrorKind::SizeLimit => Ok(None),
            e => {
                dbg!(&e);
                Err(io::Error::new(io::ErrorKind::Other, error))
            }
        },
    }
}
