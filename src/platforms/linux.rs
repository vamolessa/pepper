use std::{
    io,
    os::unix::{
        io::{AsRawFd, RawFd},
        net::{UnixListener, UnixStream},
    },
    time::Duration,
};

use pepper::{
    application::{ClientApplication, ServerApplication},
    client::ClientHandle,
    platform::{Key, PlatformEvent, PlatformRequest, ProcessHandle},
    Args,
};

mod unix_utils;
use unix_utils::{
    is_pipped, read, read_from_connection, run, suspend_process, write_all_bytes, Process, Terminal,
};

const MAX_CLIENT_COUNT: usize = 20;
const MAX_PROCESS_COUNT: usize = 43;
const MAX_TRIGGERED_EVENT_COUNT: usize = 32;

pub fn try_launching_debugger() {}

pub fn main() {
    run(run_server, run_client);
}

fn errno() -> libc::c_int {
    unsafe { *libc::__errno_location() }
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
        let mut buf = [0; std::mem::size_of::<libc::signalfd_siginfo>()];
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

fn run_server(args: Args, listener: UnixListener) {
    use io::Write;

    const NONE_PROCESS: Option<Process> = None;

    let mut application = match ServerApplication::new(args) {
        Some(application) => application,
        None => return,
    };

    let mut client_connections: [Option<UnixStream>; MAX_CLIENT_COUNT] = Default::default();
    let mut processes = [NONE_PROCESS; MAX_PROCESS_COUNT];

    let mut events = Vec::new();
    let mut timeout = None;

    const CLIENTS_START_INDEX: usize = 1;
    const CLIENTS_LAST_INDEX: usize = CLIENTS_START_INDEX + MAX_CLIENT_COUNT - 1;
    const PROCESSES_START_INDEX: usize = CLIENTS_LAST_INDEX + 1;
    const PROCESSES_LAST_INDEX: usize = PROCESSES_START_INDEX + MAX_PROCESS_COUNT - 1;

    let epoll = Epoll::new();
    epoll.add(listener.as_raw_fd(), 0);
    let mut epoll_events = EpollEvents::new();

    loop {
        let epoll_events = epoll.wait(&mut epoll_events, timeout);
        if epoll_events.len() == 0 {
            match timeout {
                Some(Duration::ZERO) => timeout = Some(ServerApplication::idle_duration()),
                Some(_) => {
                    events.push(PlatformEvent::Idle);
                    timeout = None;
                }
                None => unreachable!(),
            }
        }

        for event_index in epoll_events {
            match event_index {
                0 => match listener.accept() {
                    Ok((connection, _)) => {
                        for (i, c) in client_connections.iter_mut().enumerate() {
                            if c.is_none() {
                                epoll.add(connection.as_raw_fd(), CLIENTS_START_INDEX + i);
                                *c = Some(connection);
                                let handle = ClientHandle::from_index(i).unwrap();
                                events.push(PlatformEvent::ConnectionOpen { handle });
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
                            &mut application.platform.buf_pool,
                            ServerApplication::connection_buffer_len(),
                        ) {
                            Ok(buf) => events.push(PlatformEvent::ConnectionOutput { handle, buf }),
                            Err(()) => {
                                epoll.remove(connection.as_raw_fd());
                                client_connections[index] = None;
                                events.push(PlatformEvent::ConnectionClose { handle });
                            }
                        }
                    }
                }
                PROCESSES_START_INDEX..=PROCESSES_LAST_INDEX => {
                    let index = event_index - PROCESSES_START_INDEX;
                    if let Some(ref mut process) = processes[index] {
                        let tag = process.tag();
                        match process.read(&mut application.platform.buf_pool) {
                            Ok(None) => (),
                            Ok(Some(buf)) => events.push(PlatformEvent::ProcessOutput { tag, buf }),
                            Err(()) => {
                                if let Some(fd) = process.try_as_raw_fd() {
                                    epoll.remove(fd);
                                }
                                process.kill();
                                processes[index] = None;
                                events.push(PlatformEvent::ProcessExit { tag });
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }

        application.update(events.drain(..));
        let mut requests = application.platform.requests.drain();
        while let Some(request) = requests.next() {
            match request {
                PlatformRequest::Quit => {
                    for request in requests {
                        if let PlatformRequest::WriteToClient { buf, .. }
                        | PlatformRequest::WriteToProcess { buf, .. } = request
                        {
                            application.platform.buf_pool.release(buf);
                        }
                    }
                    return;
                }
                PlatformRequest::Redraw => timeout = Some(Duration::ZERO),
                PlatformRequest::WriteToClient { handle, buf } => {
                    let index = handle.into_index();
                    if let Some(ref mut connection) = client_connections[index] {
                        if connection.write_all(buf.as_bytes()).is_err() {
                            epoll.remove(connection.as_raw_fd());
                            client_connections[index] = None;
                            events.push(PlatformEvent::ConnectionClose { handle });
                        }
                    }
                    application.platform.buf_pool.release(buf);
                }
                PlatformRequest::CloseClient { handle } => {
                    let index = handle.into_index();
                    if let Some(connection) = client_connections[index].take() {
                        epoll.remove(connection.as_raw_fd());
                    }
                    events.push(PlatformEvent::ConnectionClose { handle });
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

                        let handle = ProcessHandle(i as _);
                        if let Ok(child) = command.spawn() {
                            let process = Process::new(child, tag, buf_len);
                            if let Some(fd) = process.try_as_raw_fd() {
                                epoll.add(fd, PROCESSES_START_INDEX + i);
                            }
                            *p = Some(process);
                            events.push(PlatformEvent::ProcessSpawned { tag, handle });
                            spawned = true;
                        }
                        break;
                    }
                    if !spawned {
                        events.push(PlatformEvent::ProcessExit { tag });
                    }
                }
                PlatformRequest::WriteToProcess { handle, buf } => {
                    let index = handle.0 as usize;
                    if let Some(ref mut process) = processes[index] {
                        if !process.write(buf.as_bytes()) {
                            if let Some(fd) = process.try_as_raw_fd() {
                                epoll.remove(fd);
                            }
                            let tag = process.tag();
                            process.kill();
                            processes[index] = None;
                            events.push(PlatformEvent::ProcessExit { tag });
                        }
                    }
                    application.platform.buf_pool.release(buf);
                }
                PlatformRequest::CloseProcessInput { handle } => {
                    if let Some(ref mut process) = processes[handle.0 as usize] {
                        process.close_input();
                    }
                }
                PlatformRequest::KillProcess { handle } => {
                    let index = handle.0 as usize;
                    if let Some(ref mut process) = processes[index] {
                        if let Some(fd) = process.try_as_raw_fd() {
                            epoll.remove(fd);
                        }
                        let tag = process.tag();
                        process.kill();
                        processes[index] = None;
                        events.push(PlatformEvent::ProcessExit { tag });
                    }
                }
            }
        }

        if !events.is_empty() {
            timeout = Some(Duration::ZERO);
        }
    }
}

fn run_client(args: Args, mut connection: UnixStream) {
    use io::{Read, Write};

    let terminal = if args.quit {
        None
    } else {
        Some(Terminal::new())
    };

    let mut application = ClientApplication::new(terminal.as_ref().map(Terminal::to_file));
    let bytes = application.init(args);
    if connection.write_all(bytes).is_err() {
        return;
    }

    let epoll = Epoll::new();
    epoll.add(connection.as_raw_fd(), 1);
    if is_pipped(libc::STDIN_FILENO) {
        epoll.add(libc::STDIN_FILENO, 3);
    }

    let mut epoll_events = EpollEvents::new();

    let resize_signal;
    if let Some(terminal) = &terminal {
        terminal.enter_raw_mode();
        epoll.add(terminal.as_raw_fd(), 0);

        let signal = SignalFd::new(libc::SIGWINCH);
        epoll.add(signal.as_raw_fd(), 2);
        resize_signal = Some(signal);

        let size = terminal.get_size();
        let (_, bytes) = application.update(Some(size), &[Key::None], None, &[]);
        if connection.write_all(bytes).is_err() {
            return;
        }
    } else {
        resize_signal = None;
    }

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
            let mut stdin_bytes = None;
            let mut server_bytes = &[][..];

            keys.clear();

            match event_index {
                0 => {
                    if let Some(terminal) = &terminal {
                        match read(terminal.as_raw_fd(), &mut buf) {
                            Ok(0) | Err(()) => break 'main_loop,
                            Ok(len) => terminal.parse_keys(&buf[..len], &mut keys),
                        }
                    }
                }
                1 => match connection.read(&mut buf) {
                    Ok(0) | Err(_) => break 'main_loop,
                    Ok(len) => server_bytes = &buf[..len],
                },
                2 => {
                    if let Some(ref signal) = resize_signal {
                        signal.read();
                        resize = terminal.as_ref().map(Terminal::get_size);
                    }
                }
                3 => match read(libc::STDIN_FILENO, &mut buf) {
                    Ok(0) | Err(()) => {
                        epoll.remove(libc::STDIN_FILENO);
                        stdin_bytes = Some(&[][..]);
                    }
                    Ok(len) => stdin_bytes = Some(&buf[..len]),
                },
                _ => unreachable!(),
            }

            let (suspend, bytes) = application.update(resize, &keys, stdin_bytes, server_bytes);
            if connection.write_all(bytes).is_err() {
                break;
            }
            if suspend {
                suspend_process(&mut application, &terminal);
            }
        }
    }

    if is_pipped(libc::STDOUT_FILENO) {
        let bytes = application.get_stdout_bytes();
        write_all_bytes(libc::STDOUT_FILENO, bytes);
    }

    drop(terminal);
    drop(application);
}
