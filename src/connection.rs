use std::{
    convert::TryFrom,
    io::{self, BufReader, Read, Write},
    net::Shutdown,
    path::Path,
};

#[cfg(target_os = "windows")]
use uds_windows::{UnixListener, UnixStream};

use bincode::Options;

use crate::{event_manager::StreamId, editor::EditorOperation, event::Key};

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TargetClient {
    All,
    Local,
    Remote(ConnectionWithClientHandle),
}

pub struct ConnectionWithClient(BufReader<UnixStream>, Vec<u8>);

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ConnectionWithClientHandle(usize);
impl TryFrom<StreamId> for ConnectionWithClientHandle {
    type Error = ();

    fn try_from(stream_id: StreamId) -> Result<Self, ()> {
        match stream_id {
            StreamId::Listener => Err(()),
            StreamId::Stream(id) => Ok(Self(id)),
        }
    }
}

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
        stream.set_nonblocking(true)?;
        let connection = ConnectionWithClient(BufReader::with_capacity(128, stream), Vec::new());

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
            let _ = connection.0.get_ref().shutdown(Shutdown::Both);
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
                if connection.0.get_mut().write_all(&connection.1[..]).is_err() {
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

    pub fn receive_key(&mut self, handle: ConnectionWithClientHandle) -> io::Result<Option<Key>> {
        match &mut self.connections[handle.0] {
            Some(connection) => deserialize(&mut connection.0),
            None => Ok(None),
        }
    }
}

pub struct ConnectionWithServer(BufReader<UnixStream>);
impl ConnectionWithServer {
    pub fn connect<P>(path: P) -> io::Result<Self>
    where
        P: AsRef<Path>,
    {
        let stream = UnixStream::connect(path)?;
        stream.set_nonblocking(true)?;
        Ok(Self(BufReader::new(stream)))
    }

    pub fn send_key(&mut self, key: Key) -> io::Result<()> {
        serialize(self.0.get_mut(), &key)
    }

    pub fn receive_operation(&mut self) -> io::Result<Option<EditorOperation>> {
        deserialize(&mut self.0)
    }
}

fn bincode_serializer() -> impl Options {
    bincode::options()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

fn serialize<T>(mut writer: &mut UnixStream, value: &T) -> io::Result<()>
where
    T: serde::ser::Serialize,
{
    match bincode_serializer().serialize_into(&mut writer, value) {
        Ok(()) => Ok(()),
        Err(error) => {
            writer.shutdown(Shutdown::Both)?;
            Err(io::Error::new(io::ErrorKind::Other, error))
        }
    }
}

fn deserialize<T>(mut reader: &mut BufReader<UnixStream>) -> io::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    let buffer = reader.buffer();
    let deserializer = bincode_serializer().with_limit(buffer.len() as _);
    match deserializer.deserialize_from(&mut reader) {
        Ok(value) => Ok(Some(value)),
        Err(error) => match error.as_ref() {
            bincode::ErrorKind::SizeLimit => Ok(None),
            _ => {
                reader.get_mut().shutdown(Shutdown::Both)?;
                Err(io::Error::new(io::ErrorKind::Other, error))
            }
        },
    }
}
