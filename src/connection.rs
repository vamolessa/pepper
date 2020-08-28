use std::{
    io::{self, Read, Write},
    net::Shutdown,
    path::Path,
};

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
use uds_windows::{UnixListener, UnixStream};

use crate::{
    client_event::{Key, KeyDeserializeResult, KeyDeserializer, KeySerializer},
    editor::EditorLoop,
    editor_operation::{
        EditorOperation, EditorOperationDeserializeResult, EditorOperationDeserializer,
        EditorOperationSerializer,
    },
    event_manager::EventRegistry,
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

    pub fn guard(&mut self) -> ReadGuard {
        ReadGuard(self)
    }
}

struct ReadGuard<'a>(&'a mut ReadBuf);

impl<'a> ReadGuard<'a> {
    pub fn read_from<R>(&mut self, mut reader: R) -> io::Result<()>
    where
        R: Read,
    {
        loop {
            match reader.read(&mut self.0.buf[self.0.len..]) {
                Ok(len) => {
                    self.0.len += len;

                    if self.0.len < self.0.buf.len() {
                        break;
                    }

                    self.0.buf.resize(self.0.buf.len() * 2, 0);
                }
                Err(e) => match e.kind() {
                    io::ErrorKind::WouldBlock => break,
                    _ => return Err(e),
                },
            }
        }

        Ok(())
    }

    pub fn as_bytes(&'a self) -> &'a [u8] {
        &self.0.buf[self.0.position..self.0.len]
    }
}

impl<'a> Drop for ReadGuard<'a> {
    fn drop(&mut self) {
        self.0.len = 0;
        self.0.position = 0;
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TargetClient {
    All,
    Local,
    Remote(ConnectionWithClientHandle),
}

pub struct ConnectionWithClient(UnixStream);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ConnectionWithClientHandle(usize);
impl ConnectionWithClientHandle {
    pub fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub fn into_index(self) -> usize {
        self.0
    }
}

pub struct ConnectionWithClientCollection {
    listener: UnixListener,
    connections: Vec<Option<ConnectionWithClient>>,
    closed_connection_indexes: Vec<usize>,
    read_buf: ReadBuf,
}

impl ConnectionWithClientCollection {
    pub fn listen<P>(path: P) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;

        Ok(Self {
            listener,
            connections: Vec::new(),
            closed_connection_indexes: Vec::new(),
            read_buf: ReadBuf::new(),
        })
    }

    pub fn register_listener(&self, event_registry: &EventRegistry) -> io::Result<()> {
        event_registry.register_listener(&self.listener)
    }

    pub fn listen_next_listener_event(&self, event_registry: &EventRegistry) -> io::Result<()> {
        event_registry.listen_next_listener_event(&self.listener)
    }

    pub fn accept_connection(
        &mut self,
        event_registry: &EventRegistry,
    ) -> io::Result<ConnectionWithClientHandle> {
        let (stream, _address) = self.listener.accept()?;
        stream.set_nonblocking(true)?;
        let connection = ConnectionWithClient(stream);

        for (i, slot) in self.connections.iter_mut().enumerate() {
            if slot.is_none() {
                let handle = ConnectionWithClientHandle(i);
                event_registry.register_stream(&connection.0, handle.into())?;
                *slot = Some(connection);
                return Ok(handle);
            }
        }

        let handle = ConnectionWithClientHandle(self.connections.len());
        event_registry.register_stream(&connection.0, handle.into())?;
        self.connections.push(Some(connection));
        Ok(handle)
    }

    pub fn listen_next_connection_event(
        &self,
        handle: ConnectionWithClientHandle,
        event_registry: &EventRegistry,
    ) -> io::Result<()> {
        if let Some(connection) = &self.connections[handle.0] {
            event_registry.listen_next_stream_event(&connection.0, handle.into())?;
        }

        Ok(())
    }

    pub fn close_connection(&mut self, handle: ConnectionWithClientHandle) {
        if let Some(connection) = &self.connections[handle.0] {
            let _ = connection.0.shutdown(Shutdown::Both);
            self.closed_connection_indexes.push(handle.0);
        }
    }

    pub fn close_all_connections(&mut self) {
        for connection in self.connections.iter().flatten() {
            let _ = connection.0.shutdown(Shutdown::Both);
        }
    }

    pub fn unregister_closed_connections(
        &mut self,
        event_registry: &EventRegistry,
    ) -> io::Result<()> {
        for i in self.closed_connection_indexes.drain(..) {
            if let Some(connection) = self.connections[i].take() {
                event_registry.unregister_stream(&connection.0)?;
            }
        }

        Ok(())
    }

    pub fn send_serialized_operations(
        &mut self,
        handle: ConnectionWithClientHandle,
        serializer: &EditorOperationSerializer,
    ) {
        let bytes = serializer.remote_bytes(handle);
        if bytes.is_empty() {
            return;
        }

        let stream = match &mut self.connections[handle.0] {
            Some(connection) => &mut connection.0,
            None => return,
        };

        if let Err(_) = stream.write_all(bytes).and_then(|_| stream.flush()) {
            self.close_connection(handle);
        }
    }

    pub fn receive_keys<F>(
        &mut self,
        handle: ConnectionWithClientHandle,
        mut callback: F,
    ) -> io::Result<EditorLoop>
    where
        F: FnMut(Key) -> EditorLoop,
    {
        let connection = match &mut self.connections[handle.0] {
            Some(connection) => connection,
            None => return Ok(EditorLoop::Quit),
        };

        let mut read_guard = self.read_buf.guard();
        read_guard.read_from(&mut connection.0)?;
        let mut last_result = EditorLoop::Quit;
        let mut deserializer = KeyDeserializer::from_slice(read_guard.as_bytes());

        loop {
            match deserializer.deserialize_next() {
                KeyDeserializeResult::Some(key) => {
                    last_result = callback(key);
                    if let EditorLoop::Quit = last_result {
                        break;
                    }
                }
                KeyDeserializeResult::None => break,
                KeyDeserializeResult::Error => return Err(io::Error::from(io::ErrorKind::Other)),
            }
        }

        Ok(last_result)
    }

    pub fn all_handles(&self) -> impl Iterator<Item = ConnectionWithClientHandle> {
        (0..self.connections.len()).map(|i| ConnectionWithClientHandle(i))
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

    pub fn set_blocking(&mut self) -> io::Result<()> {
        self.stream.set_nonblocking(false)
    }

    pub fn close(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }

    pub fn register_connection(&self, event_registry: &EventRegistry) -> io::Result<()> {
        event_registry.register_stream(
            &self.stream,
            ConnectionWithClientHandle::from_index(0).into(),
        )
    }

    pub fn listen_next_event(&self, event_registry: &EventRegistry) -> io::Result<()> {
        event_registry.listen_next_stream_event(
            &self.stream,
            ConnectionWithClientHandle::from_index(0).into(),
        )
    }

    pub fn send_serialized_keys(&mut self, serializer: &mut KeySerializer) -> io::Result<()> {
        let bytes = serializer.bytes();
        if bytes.is_empty() {
            return Ok(());
        }

        let result = self
            .stream
            .write_all(bytes)
            .and_then(|_| self.stream.flush());

        serializer.clear();
        result
    }

    pub fn receive_operations<F>(&mut self, mut callback: F) -> io::Result<Option<()>>
    where
        F: FnMut(EditorOperation<'_>),
    {
        let mut read_guard = self.read_buf.guard();
        read_guard.read_from(&mut self.stream)?;
        let mut last_response = None;
        let mut deserializer = EditorOperationDeserializer::from_slice(read_guard.as_bytes());

        loop {
            match deserializer.deserialize_next() {
                EditorOperationDeserializeResult::Some(operation) => {
                    callback(operation);
                    last_response = Some(());
                }
                EditorOperationDeserializeResult::None => break,
                EditorOperationDeserializeResult::Error => {
                    return Err(io::Error::from(io::ErrorKind::Other))
                }
            }
        }

        Ok(last_response)
    }
}
