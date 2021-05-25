use std::{
    io,
    os::unix::{
        io::{AsRawFd, RawFd},
        net::{UnixListener, UnixStream},
    },
    sync::atomic::{AtomicIsize, Ordering},
    time::Duration,
};

use pepper::{
    application::{AnyError, ApplicationEvent, ClientApplication, ServerApplication},
    client::ClientHandle,
    platform::{BufPool, Key, Platform, PlatformRequest, ProcessHandle},
    Args,
};

mod unix_utils;
use unix_utils::{
    errno, get_terminal_size, is_pipped, parse_terminal_keys, read, read_from_connection, run,
    suspend_process, Process, RawMode,
};

const MAX_CLIENT_COUNT: usize = 20;
const MAX_PROCESS_COUNT: usize = 42;
const MAX_TRIGGERED_EVENT_COUNT: usize = 32;

pub fn main() {
    run(run_server, run_client);
}

struct EventFd(RawFd);
impl EventFd {
    pub fn new() -> Self {
        let fd = unsafe { libc::eventfd(0, 0) };
        if fd == -1 {
            panic!("could not create event fd");
        }
        Self(fd)
    }

    pub fn write(fd: RawFd) {
        let mut buf = 1u64.to_ne_bytes();
        let result = unsafe { libc::write(fd, buf.as_mut_ptr() as _, buf.len() as _) };
        if result != buf.len() as _ {
            panic!("could not write to event fd");
        }
    }

    pub fn read(&self) {
        let mut buf = [0; 8];
        if read(self.0, &mut buf) != Ok(buf.len()) {
            panic!("could not read from event fd");
        }
    }
}
impl AsRawFd for EventFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}
impl Drop for EventFd {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

struct SignalFd(RawFd);
impl SignalFd {
    pub fn new(signal: libc::c_int) -> Self {
        unsafe {
            let mut signals = std::mem::zeroed();
            let result = libc::sigemptyset(&mut signals);
            if result == -1 {
                panic!("could not create signal fd");
            }
            let result = libc::sigaddset(&mut signals, signal);
            if result == -1 {
                panic!("could not create signal fd");
            }
            let result = libc::sigprocmask(libc::SIG_BLOCK, &signals, std::ptr::null_mut());
            if result == -1 {
                panic!("could not create signal fd");
            }
            let fd = libc::signalfd(-1, &signals, 0);
            if fd == -1 {
                panic!("could not create signal fd");
            }
            Self(fd)
        }
    }

    pub fn read(&self) {
        let mut buf = [0u8; std::mem::size_of::<libc::signalfd_siginfo>()];
        if read(self.0, &mut buf) != Ok(buf.len()) {
            panic!("could not read from signal fd");
        }
    }
}
impl AsRawFd for SignalFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}
impl Drop for SignalFd {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

struct EpollEvents([libc::epoll_event; MAX_TRIGGERED_EVENT_COUNT]);
impl EpollEvents {
    pub fn new() -> Self {
        const DEFAULT_EVENT: libc::epoll_event = libc::epoll_event { events: 0, u64: 0 };
        Self([DEFAULT_EVENT; MAX_TRIGGERED_EVENT_COUNT])
    }
}
struct Epoll(RawFd);
impl Epoll {
    pub fn new() -> Self {
        let fd = unsafe { libc::epoll_create1(0) };
        if fd == -1 {
            panic!("could not create epoll");
        }
        Self(fd)
    }

    pub fn add(&self, fd: RawFd, index: usize) {
        let mut event = libc::epoll_event {
            events: (libc::EPOLLIN | libc::EPOLLERR | libc::EPOLLRDHUP | libc::EPOLLHUP) as _,
            u64: index as _,
        };
        let result = unsafe { libc::epoll_ctl(self.0, libc::EPOLL_CTL_ADD, fd, &mut event) };
        if result == -1 {
            panic!("could not add event");
        }
    }

    pub fn remove(&self, fd: RawFd) {
        let mut event = libc::epoll_event { events: 0, u64: 0 };
        unsafe { libc::epoll_ctl(self.0, libc::EPOLL_CTL_DEL, fd, &mut event) };
    }

    pub fn wait<'a>(
        &self,
        events: &'a mut EpollEvents,
        timeout: Option<Duration>,
    ) -> impl 'a + ExactSizeIterator<Item = usize> {
        let timeout = match timeout {
            Some(duration) => duration.as_millis() as _,
            None => -1,
        };
        let mut len = unsafe {
            libc::epoll_wait(self.0, events.0.as_mut_ptr(), events.0.len() as _, timeout)
        };
        if len == -1 {
            if errno() == libc::EINTR {
                len = 0;
            } else {
                panic!("could not wait for events");
            }
        }

        events.0[..len as usize].iter().map(|e| e.u64 as _)
    }
}
impl Drop for Epoll {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

fn run_server(args: Args, listener: UnixListener) -> Result<(), AnyError> {
    use io::Write;

    const NONE_PROCESS: Option<Process> = None;
    static NEW_REQUEST_EVENT_FD: AtomicIsize = AtomicIsize::new(-1);

    let new_request_event = EventFd::new();
    NEW_REQUEST_EVENT_FD.store(new_request_event.as_raw_fd() as _, Ordering::Relaxed);

    let (request_sender, request_receiver) = ServerApplication::platform_request_channel();
    let platform = Platform::new(
        || EventFd::write(NEW_REQUEST_EVENT_FD.load(Ordering::Relaxed) as _),
        request_sender,
    );
    let event_sender = ServerApplication::run(args, platform);

    let mut client_connections: [Option<UnixStream>; MAX_CLIENT_COUNT] = Default::default();
    let mut processes = [NONE_PROCESS; MAX_PROCESS_COUNT];
    let mut buf_pool = BufPool::default();

    let mut timeout = Some(ServerApplication::idle_duration());

    const CLIENTS_START_INDEX: usize = 1 + 1;
    const CLIENTS_LAST_INDEX: usize = CLIENTS_START_INDEX + MAX_CLIENT_COUNT - 1;
    const PROCESSES_START_INDEX: usize = CLIENTS_LAST_INDEX + 1;
    const PROCESSES_LAST_INDEX: usize = PROCESSES_START_INDEX + MAX_PROCESS_COUNT - 1;

    let epoll = Epoll::new();
    epoll.add(new_request_event.as_raw_fd(), 0);
    epoll.add(listener.as_raw_fd(), 1);
    let mut epoll_events = EpollEvents::new();

    loop {
        let events = epoll.wait(&mut epoll_events, timeout);
        if events.len() == 0 {
            timeout = None;
            event_sender.send(ApplicationEvent::Idle)?;
            continue;
        }

        for event_index in events {
            match event_index {
                0 => {
                    new_request_event.read();
                    for request in request_receiver.try_iter() {
                        match request {
                            PlatformRequest::Exit => return Ok(()),
                            PlatformRequest::WriteToClient { handle, buf } => {
                                let index = handle.into_index();
                                if let Some(ref mut connection) = client_connections[index] {
                                    if connection.write_all(buf.as_bytes()).is_err() {
                                        epoll.remove(connection.as_raw_fd());
                                        client_connections[index] = None;
                                        event_sender
                                            .send(ApplicationEvent::ConnectionClose { handle })?;
                                    }
                                }
                            }
                            PlatformRequest::CloseClient { handle } => {
                                let index = handle.into_index();
                                if let Some(connection) = client_connections[index].take() {
                                    epoll.remove(connection.as_raw_fd());
                                }
                                event_sender.send(ApplicationEvent::ConnectionClose { handle })?;
                            }
                            PlatformRequest::SpawnProcess {
                                tag,
                                mut command,
                                buf_len,
                            } => {
                                let mut spawned = false;
                                for (i, p) in processes.iter_mut().enumerate() {
                                    if p.is_some() {
                                        continue;
                                    }

                                    let handle = ProcessHandle(i);
                                    if let Ok(child) = command.spawn() {
                                        let process = Process::new(child, tag, buf_len);
                                        if let Some(fd) = process.try_as_raw_fd() {
                                            epoll.add(fd, PROCESSES_START_INDEX + i);
                                        }
                                        *p = Some(process);
                                        event_sender.send(ApplicationEvent::ProcessSpawned {
                                            tag,
                                            handle,
                                        })?;
                                        spawned = true;
                                    }
                                    break;
                                }
                                if !spawned {
                                    event_sender.send(ApplicationEvent::ProcessExit { tag })?;
                                }
                            }
                            PlatformRequest::WriteToProcess { handle, buf } => {
                                let index = handle.0;
                                if let Some(ref mut process) = processes[index] {
                                    if !process.write(buf.as_bytes()) {
                                        if let Some(fd) = process.try_as_raw_fd() {
                                            epoll.remove(fd);
                                        }
                                        let tag = process.tag();
                                        process.kill();
                                        processes[index] = None;
                                        event_sender.send(ApplicationEvent::ProcessExit { tag })?;
                                    }
                                }
                            }
                            PlatformRequest::CloseProcessInput { handle } => {
                                if let Some(ref mut process) = processes[handle.0] {
                                    process.close_input();
                                }
                            }
                            PlatformRequest::KillProcess { handle } => {
                                let index = handle.0;
                                if let Some(ref mut process) = processes[index] {
                                    if let Some(fd) = process.try_as_raw_fd() {
                                        epoll.remove(fd);
                                    }
                                    let tag = process.tag();
                                    process.kill();
                                    processes[index] = None;
                                    event_sender.send(ApplicationEvent::ProcessExit { tag })?;
                                }
                            }
                        }
                    }
                }
                1 => match listener.accept() {
                    Ok((connection, _)) => {
                        for (i, c) in client_connections.iter_mut().enumerate() {
                            if c.is_none() {
                                epoll.add(connection.as_raw_fd(), CLIENTS_START_INDEX + i);
                                *c = Some(connection);
                                let handle = ClientHandle::from_index(i).unwrap();
                                event_sender.send(ApplicationEvent::ConnectionOpen { handle })?;
                                break;
                            }
                        }
                    }
                    Err(error) => panic!("could not accept connection {}", error),
                },
                CLIENTS_START_INDEX..=CLIENTS_LAST_INDEX => {
                    let index = event_index - CLIENTS_START_INDEX;
                    if let Some(ref mut connection) = client_connections[index] {
                        let handle = ClientHandle::from_index(index).unwrap();
                        match read_from_connection(
                            connection,
                            &mut buf_pool,
                            ServerApplication::connection_buffer_len(),
                        ) {
                            Ok(buf) if !buf.as_bytes().is_empty() => {
                                event_sender
                                    .send(ApplicationEvent::ConnectionOutput { handle, buf })?;
                            }
                            _ => {
                                epoll.remove(connection.as_raw_fd());
                                client_connections[index] = None;
                                event_sender.send(ApplicationEvent::ConnectionClose { handle })?;
                            }
                        }
                    }

                    timeout = Some(ServerApplication::idle_duration());
                }
                PROCESSES_START_INDEX..=PROCESSES_LAST_INDEX => {
                    let index = event_index - PROCESSES_START_INDEX;
                    if let Some(ref mut process) = processes[index] {
                        let tag = process.tag();
                        match process.read(&mut buf_pool) {
                            Ok(None) => (),
                            Ok(Some(buf)) if !buf.as_bytes().is_empty() => {
                                event_sender.send(ApplicationEvent::ProcessOutput { tag, buf })?;
                            }
                            _ => {
                                if let Some(fd) = process.try_as_raw_fd() {
                                    epoll.remove(fd);
                                }
                                process.kill();
                                processes[index] = None;
                                event_sender.send(ApplicationEvent::ProcessExit { tag })?;
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

fn run_client(args: Args, mut connection: UnixStream) {
    use io::{Read, Write};

    let mut buf = [0; 2];
    match connection.read_exact(&mut buf) {
        Ok(()) => (),
        _ => return,
    }
    let is_first_client = buf[0] != 0;
    let client_index = buf[1];

    let client_handle = ClientHandle::from_index(client_index as _).unwrap();
    let is_pipped = is_pipped();

    let stdout = io::stdout();
    let mut application = ClientApplication::new(client_handle, stdout.lock(), is_pipped);
    let bytes = application.init(args, is_first_client);
    if connection.write_all(bytes).is_err() {
        return;
    }

    let mut raw_mode;
    let resize_signal;

    let epoll = Epoll::new();
    epoll.add(connection.as_raw_fd(), 0);
    epoll.add(libc::STDIN_FILENO, 1);
    let mut epoll_events = EpollEvents::new();

    if is_pipped {
        raw_mode = None;
        resize_signal = None;
    } else {
        raw_mode = Some(RawMode::enter());
        let signal = SignalFd::new(libc::SIGWINCH);
        epoll.add(signal.as_raw_fd(), 2);
        resize_signal = Some(signal);

        let size = get_terminal_size();
        let (_, bytes) = application.update(Some(size), &[Key::None], &[], &[]);
        if connection.write_all(bytes).is_err() {
            return;
        }
    }

    let backspace_code = match raw_mode {
        Some(ref raw) => raw.backspace_code(),
        None => 0,
    };
    let mut keys = Vec::new();

    const BUF_LEN: usize =
        if ClientApplication::connection_buffer_len() > ClientApplication::stdin_buffer_len() {
            ClientApplication::connection_buffer_len()
        } else {
            ClientApplication::stdin_buffer_len()
        };
    let mut buf = [0; BUF_LEN];

    'main_loop: loop {
        for event_index in epoll.wait(&mut epoll_events, None) {
            let mut resize = None;
            let mut stdin_bytes = &[][..];
            let mut server_bytes = &[][..];

            keys.clear();

            match event_index {
                0 => match connection.read(&mut buf) {
                    Ok(0) | Err(_) => break 'main_loop,
                    Ok(len) => server_bytes = &buf[..len],
                },
                1 => match read(libc::STDIN_FILENO, &mut buf) {
                    Ok(0) | Err(()) => {
                        epoll.remove(libc::STDIN_FILENO);
                        continue;
                    }
                    Ok(len) => {
                        let bytes = &buf[..len];

                        if is_pipped {
                            stdin_bytes = bytes;
                        } else {
                            parse_terminal_keys(bytes, backspace_code, &mut keys);
                        }
                    }
                },
                2 => {
                    if let Some(ref signal) = resize_signal {
                        signal.read();
                        resize = Some(get_terminal_size());
                    }
                }
                _ => unreachable!(),
            }

            let (suspend, bytes) = application.update(resize, &keys, stdin_bytes, server_bytes);
            if connection.write_all(bytes).is_err() {
                break;
            }
            if suspend {
                suspend_process(&mut application, &mut raw_mode);
            }
        }
    }

    drop(raw_mode);
}

