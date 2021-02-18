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

pub struct ClientEventIter<'data> {
    buf: &'data mut Vec<u8>,
    read_len: usize,
}
impl<'this, 'data: 'this> ClientEventIter<'data> {
    pub fn next(&'this mut self) -> Option<ClientEvent<'data>> {
        let slice = &self.buf[self.read_len..];
        if slice.is_empty() {
            return None;
        }

        let slice = unsafe { std::slice::from_raw_parts(slice.as_ptr(), slice.len()) };

        let mut deserializer = DeserializationSlice::from_slice(slice);
        match ClientEvent::deserialize(&mut deserializer) {
            Ok(event) => {
                self.read_len += slice.len() - deserializer.as_slice().len();
                Some(event)
            }
            Err(_) => {
                self.read_len = self.buf.len();
                None
            }
        }
    }
}
impl<'data> Drop for ClientEventIter<'data> {
    fn drop(&mut self) {
        let rest_len = self.buf.len() - self.read_len;
        self.buf.copy_within(self.read_len.., 0);
        self.buf.truncate(rest_len);
    }
}

#[derive(Default)]
struct ClientEventDeserializationBuf(Vec<u8>);

// TODO: rename to ClientEventReceiver
#[derive(Default)]
pub struct ClientEventDeserializationBufCollection {
    bufs: Vec<ClientEventDeserializationBuf>,
}

impl ClientEventDeserializationBufCollection {
    pub fn receive_events<'a>(
        &'a mut self,
        client_handle: ClientHandle,
        bytes: &[u8],
    ) -> ClientEventIter<'a> {
        let index = client_handle.into_index();
        if index >= self.bufs.len() {
            self.bufs.resize_with(index + 1, Default::default);
        }

        let buf = &mut self.bufs[index].0;
        buf.extend_from_slice(bytes);
        ClientEventIter { buf, read_len: 0 }
    }
}
