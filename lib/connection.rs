use crate::{
    client_event::ClientEvent,
    editor::EditorLoop,
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

#[derive(Default)]
struct ClientEventDeserializationBuf {
    buf: Vec<u8>,
}

impl ClientEventDeserializationBuf {
    pub fn receive_events<F>(&mut self, bytes: &[u8], mut func: F) -> EditorLoop
    where
        F: FnMut(ClientEvent) -> EditorLoop,
    {
        self.buf.extend_from_slice(bytes);
        let mut editor_loop = EditorLoop::Continue;
        let mut deserializer = DeserializationSlice::from_slice(&self.buf);

        loop {
            if deserializer.as_slice().is_empty() {
                break;
            }

            match ClientEvent::deserialize(&mut deserializer) {
                Ok(event) => {
                    editor_loop = func(event);
                    if editor_loop.is_quit() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        let rest_len = deserializer.as_slice().len();
        let start = self.buf.len() - rest_len;
        self.buf.copy_within(start.., 0);
        self.buf.truncate(rest_len);

        editor_loop
    }
}

#[derive(Default)]
pub struct ClientEventDeserializationBufCollection {
    bufs: Vec<ClientEventDeserializationBuf>,
}

impl ClientEventDeserializationBufCollection {
    pub fn receive_events<F>(&mut self, index: usize, bytes: &[u8], func: F) -> EditorLoop
    where
        F: FnMut(ClientEvent) -> EditorLoop,
    {
        if index >= self.bufs.len() {
            self.bufs.resize_with(index + 1, Default::default);
        }

        self.bufs[index].receive_events(bytes, func)
    }
}