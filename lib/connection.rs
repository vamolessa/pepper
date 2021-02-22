// TODO: merge with editor.rs??

use crate::{
    client::ClientHandle,
    client_event::ClientEvent,
    serialization::{DeserializationSlice, Serialize},
};

/*
struct ReadBuf {
    buf: Vec<u8>,
    len: usize,
}

impl ReadBuf {
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(2 * 1024);
        buf.resize(buf.capacity(), 0);
        Self { buf, len: 0 }
    }

    pub fn read_from<R>(&mut self, mut reader: R) -> io::Result<&[u8]>
    where
        R: Read,
    {
        self.len = 0;
        loop {
            match reader.read(&mut self.buf[self.len..]) {
                Ok(len) => {
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

        Ok(&self.buf[..self.len])
    }
}
*/

pub struct ClientEventIter {
    buf_index: usize,
    read_len: usize,
}
impl ClientEventIter {
    pub fn next<'a>(&mut self, receiver: &'a ClientEventReceiver) -> Option<ClientEvent<'a>> {
        let buf = &receiver.bufs[self.buf_index];
        let slice = &buf[self.read_len..];
        if slice.is_empty() {
            return None;
        }

        let mut deserializer = DeserializationSlice::from_slice(slice);
        match ClientEvent::deserialize(&mut deserializer) {
            Ok(event) => {
                self.read_len = buf.len() - deserializer.as_slice().len();
                Some(event)
            }
            Err(_) => {
                self.read_len = buf.len();
                None
            }
        }
    }

    pub fn finish(&self, receiver: &mut ClientEventReceiver) {
        let buf = &mut receiver.bufs[self.buf_index];
        let rest_len = buf.len() - self.read_len;
        buf.copy_within(self.read_len.., 0);
        buf.truncate(rest_len);
    }
}

#[derive(Default)]
pub struct ClientEventReceiver {
    bufs: Vec<Vec<u8>>,
}

impl ClientEventReceiver {
    pub fn receive_events(&mut self, client_handle: ClientHandle, bytes: &[u8]) -> ClientEventIter {
        let buf_index = client_handle.into_index();
        if buf_index >= self.bufs.len() {
            self.bufs.resize_with(buf_index + 1, Default::default);
        }
        let buf = &mut self.bufs[buf_index];
        buf.extend_from_slice(bytes);
        ClientEventIter {
            buf_index,
            read_len: 0,
        }
    }
}
