use std::{
    io,
    os::unix::{
        io::{AsRawFd, RawFd},
        net::{UnixListener, UnixStream},
    },
    sync::{
        atomic::{AtomicIsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use pepper::{
    application::{AnyError, ApplicationEvent, ClientApplication, ServerApplication},
    client::ClientHandle,
    platform::{BufPool, Platform, PlatformRequest, ProcessHandle},
    Args,
};

mod unix_utils;
use unix_utils::{
    get_terminal_size, is_pipped, parse_terminal_keys, read, read_from_connection, run, Process,
    RawMode,
};

const MAX_CLIENT_COUNT: usize = 20;
const MAX_PROCESS_COUNT: usize = 42;
const MAX_TRIGGERED_EVENT_COUNT: usize = 32;

pub fn main() {
    /*
    static KQUEUE_FD: AtomicIsize = AtomicIsize::new(-1);

    let raw_mode = RawMode::enter();
    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    //let mut buf = [0; 64];
    let mut buf = [0; 1];
    let mut keys = Vec::new();

    let kqueue = Kqueue::new();
    kqueue.add(Event::FlushRequests(false), 0);
    kqueue.add(Event::Fd(stdin.as_raw_fd()), 1);
    kqueue.add(Event::Resize, 2);
    let mut kqueue_events = KqueueEvents::new();

    KQUEUE_FD.store(kqueue.as_raw_fd() as _, Ordering::Relaxed);

    std::thread::spawn(|| {
        for _ in 0..10 {
            print!("sending flush request\r\n");
            let fd = KQUEUE_FD.load(Ordering::Relaxed) as _;
            let event = Event::FlushRequests(true).into_kevent(libc::EV_ADD, 0);
            if !modify_kqueue(fd, &event) {
                print!("error trigerring flush events\r\n");
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });

    let (width, height) = get_terminal_size();
    print!("terminal size: {}, {}\r\n", width, height);

    'main_loop: loop {
        let events = kqueue.wait(&mut kqueue_events, None);
        for event in events {
            let event = match event {
                Ok(event) => event,
                Err(()) => panic!("ops something bad happened"),
            };
            match event.index {
                0 => print!("received flush request\r\n"),
                1 => {
                    //use io::Read;
                    //let len = stdin.read(&mut buf).unwrap();
                    let result = unsafe {
                        libc::read(stdin.as_raw_fd(), buf.as_mut_ptr() as _, buf.len() as _)
                    };
                    if result == -1 {
                        panic!("something wrong reading from stdin");
                    }
                    let len = result as usize;

                    keys.clear();
                    parse_terminal_keys(&buf[..len], &mut keys);
                    for &key in &keys {
                        print!("{} bytes: {}\r\n", key, event.data);
                        if key == Key::Char('q') {
                            break 'main_loop;
                        }
                    }
                }
                2 => {
                    let (width, height) = get_terminal_size();
                    print!("terminal size: {}, {}\r\n", width, height);
                    kqueue.remove(Event::Resize);
                }
                _ => unreachable!(),
            };
        }
    }

    drop(raw_mode);
    */
    run(run_server, run_client);
}

enum Event {
    Resize,
    FlushRequests(bool),
    Fd(RawFd),
}
impl Event {
    pub fn into_kevent(self, flags: u16, index: usize) -> libc::kevent {
        match self {
            Self::Resize => libc::kevent {
                ident: libc::SIGWINCH as _,
                filter: libc::EVFILT_SIGNAL,
                flags,
                fflags: 0,
                data: 0,
                udata: index as _,
            },
            Self::FlushRequests(triggered) => libc::kevent {
                ident: 0,
                filter: libc::EVFILT_USER,
                flags: flags | libc::EV_ONESHOT,
                fflags: if triggered { libc::NOTE_TRIGGER } else { 0 },
                data: 0,
                udata: index as _,
            },
            Self::Fd(fd) => libc::kevent {
                ident: fd as _,
                filter: libc::EVFILT_READ,
                flags,
                fflags: 0,
                data: 0,
                udata: index as _,
            },
        }
    }
}

struct TriggeredEvent {
    pub index: usize,
    pub data: isize,
}

struct KqueueEvents([libc::kevent; MAX_TRIGGERED_EVENT_COUNT]);
impl KqueueEvents {
    pub fn new() -> Self {
        const DEFAULT_KEVENT: libc::kevent = libc::kevent {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        Self([DEFAULT_KEVENT; MAX_TRIGGERED_EVENT_COUNT])
    }
}

fn modify_kqueue(fd: RawFd, event: &libc::kevent) -> bool {
    unsafe { libc::kevent(fd, event as _, 1, std::ptr::null_mut(), 0, std::ptr::null()) == 0 }
}

struct Kqueue(RawFd);
impl Kqueue {
    pub fn new() -> Self {
        let fd = unsafe { libc::kqueue() };
        if fd == -1 {
            panic!("could not create kqueue");
        }
        Self(fd)
    }

    pub fn add(&self, event: Event, index: usize) {
        let event = event.into_kevent(libc::EV_ADD, index);
        if !modify_kqueue(self.0, &event) {
            panic!("could not add event");
        }
    }

    pub fn remove(&self, event: Event) {
        let event = event.into_kevent(libc::EV_DELETE, 0);
        if !modify_kqueue(self.0, &event) {
            panic!("could not remove event");
        }
    }

    pub fn wait<'a>(
        &self,
        events: &'a mut KqueueEvents,
        timeout: Option<Duration>,
    ) -> impl 'a + ExactSizeIterator<Item = Result<TriggeredEvent, ()>> {
        let mut timespec = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let timeout = match timeout {
            Some(duration) => {
                timespec.tv_sec = duration.as_secs() as _;
                timespec.tv_nsec = duration.subsec_nanos() as _;
                &timespec as _
            }
            None => std::ptr::null(),
        };

        let len = unsafe {
            libc::kevent(
                self.0,
                [].as_ptr(),
                0,
                events.0.as_mut_ptr(),
                events.0.len() as _,
                timeout,
            )
        };
        if len == -1 {
            panic!("could not wait for events");
        }

        events.0[..len as usize].iter().map(|e| {
            if e.flags & libc::EV_ERROR != 0 {
                Err(())
            } else {
                Ok(TriggeredEvent {
                    index: e.udata as _,
                    data: e.data as _,
                })
            }
        })
    }
}
impl AsRawFd for Kqueue {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}
impl Drop for Kqueue {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

fn run_server(listener: UnixListener) -> Result<(), AnyError> {
    use io::Write;

    const NONE_PROCESS: Option<Process> = None;
    static KQUEUE_FD: AtomicIsize = AtomicIsize::new(-1);

    let kqueue = Kqueue::new();
    kqueue.add(Event::FlushRequests(false), 0);
    kqueue.add(Event::Fd(listener.as_raw_fd()), 1);
    let mut kqueue_events = KqueueEvents::new();

    KQUEUE_FD.store(kqueue.as_raw_fd() as _, Ordering::Relaxed);

    fn flush_requests() {
        print!("flush requests from application\r\n");
        let fd = KQUEUE_FD.load(Ordering::Relaxed) as _;
        let event = Event::FlushRequests(true).into_kevent(libc::EV_ADD, 0);
        if !modify_kqueue(fd, &event) {
            panic!("error trigerring flush events");
        }
    }

    let (request_sender, request_receiver) = mpsc::channel();
    let platform = Platform::new(flush_requests, request_sender);
    let event_sender = ServerApplication::run(platform);

    let mut client_connections: [Option<UnixStream>; MAX_CLIENT_COUNT] = Default::default();
    let mut processes = [NONE_PROCESS; MAX_PROCESS_COUNT];
    let mut buf_pool = BufPool::default();

    let mut timeout = Some(ServerApplication::idle_duration());

    const CLIENTS_START_INDEX: usize = 1 + 1;
    const CLIENTS_LAST_INDEX: usize = CLIENTS_START_INDEX + MAX_CLIENT_COUNT - 1;
    const PROCESSES_START_INDEX: usize = CLIENTS_LAST_INDEX + 1;
    const PROCESSES_LAST_INDEX: usize = PROCESSES_START_INDEX + MAX_PROCESS_COUNT - 1;

    loop {
        let events = kqueue.wait(&mut kqueue_events, timeout);
        if events.len() == 0 {
            eprint!("idle time\r\n");
            timeout = None;
            event_sender.send(ApplicationEvent::Idle)?;
            continue;
        }

        for event in events {
            let (event_index, event_data) = match event {
                Ok(event) => (event.index, event.data),
                Err(()) => return Ok(()),
            };
            eprintln!("new event: {}", event_index);
            match event_index {
                0 => {
                    for request in request_receiver.try_iter() {
                        eprintln!("new request: {:?}", std::mem::discriminant(&request));
                        match request {
                            PlatformRequest::Exit => return Ok(()),
                            PlatformRequest::WriteToClient { handle, buf } => {
                                let index = handle.into_index();
                                if let Some(ref mut connection) = client_connections[index] {
                                    if connection.write_all(buf.as_bytes()).is_err() {
                                        kqueue.remove(Event::Fd(connection.as_raw_fd()));
                                        client_connections[index] = None;
                                        event_sender
                                            .send(ApplicationEvent::ConnectionClose { handle })?;
                                    }
                                }
                            }
                            PlatformRequest::CloseClient { handle } => {
                                let index = handle.into_index();
                                if let Some(connection) = client_connections[index].take() {
                                    kqueue.remove(Event::Fd(connection.as_raw_fd()));
                                }
                                event_sender.send(ApplicationEvent::ConnectionClose { handle })?;
                            }
                            PlatformRequest::SpawnProcess {
                                tag,
                                mut command,
                                buf_len,
                            } => {
                                for (i, p) in processes.iter_mut().enumerate() {
                                    if p.is_some() {
                                        continue;
                                    }

                                    let handle = ProcessHandle(i);
                                    match command.spawn() {
                                        Ok(child) => {
                                            let process = Process::new(child, tag, buf_len);
                                            if let Some(fd) = process.try_as_raw_fd() {
                                                kqueue
                                                    .add(Event::Fd(fd), PROCESSES_START_INDEX + i);
                                            }
                                            *p = Some(process);
                                            event_sender.send(
                                                ApplicationEvent::ProcessSpawned { tag, handle },
                                            )?;
                                        }
                                        Err(_) => {
                                            event_sender.send(ApplicationEvent::ProcessExit {
                                                tag,
                                                success: false,
                                            })?
                                        }
                                    }
                                    break;
                                }
                            }
                            PlatformRequest::WriteToProcess { handle, buf } => {
                                let index = handle.0;
                                if let Some(ref mut process) = processes[index] {
                                    if !process.write(buf.as_bytes()) {
                                        if let Some(fd) = process.try_as_raw_fd() {
                                            kqueue.remove(Event::Fd(fd));
                                        }
                                        let tag = process.tag();
                                        process.kill();
                                        processes[index] = None;
                                        event_sender.send(ApplicationEvent::ProcessExit {
                                            tag,
                                            success: false,
                                        })?;
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
                                        kqueue.remove(Event::Fd(fd));
                                    }
                                    let tag = process.tag();
                                    process.kill();
                                    processes[index] = None;
                                    event_sender.send(ApplicationEvent::ProcessExit {
                                        tag,
                                        success: false,
                                    })?;
                                }
                            }
                        }
                    }
                }
                1 => {
                    eprint!("new connections available event_data: {}\r\n", event_data);
                    for _ in 0..event_data {
                        match listener.accept() {
                            Ok((connection, _)) => {
                                for (i, c) in client_connections.iter_mut().enumerate() {
                                    if c.is_none() {
                                        kqueue.add(
                                            Event::Fd(connection.as_raw_fd()),
                                            CLIENTS_START_INDEX + i,
                                        );
                                        *c = Some(connection);
                                        let handle = ClientHandle::from_index(i).unwrap();
                                        event_sender
                                            .send(ApplicationEvent::ConnectionOpen { handle })?;
                                        break;
                                    }
                                }
                            }
                            Err(error) => panic!("could not accept connection {}", error),
                        }
                    }
                }
                CLIENTS_START_INDEX..=CLIENTS_LAST_INDEX => {
                    let index = event_index - CLIENTS_START_INDEX;
                    if let Some(ref mut connection) = client_connections[index] {
                        let handle = ClientHandle::from_index(index).unwrap();
                        match read_from_connection(connection, &mut buf_pool, event_data as _) {
                            Ok(buf) if !buf.as_bytes().is_empty() => {
                                event_sender
                                    .send(ApplicationEvent::ConnectionOutput { handle, buf })?;
                            }
                            _ => {
                                kqueue.remove(Event::Fd(connection.as_raw_fd()));
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
                            Ok(Some(buf)) => {
                                if buf.as_bytes().is_empty() {
                                    event_sender.send(ApplicationEvent::ProcessExit {
                                        tag,
                                        success: true,
                                    })?;
                                } else {
                                    event_sender
                                        .send(ApplicationEvent::ProcessOutput { tag, buf })?;
                                }
                            }
                            Err(()) => {
                                if let Some(fd) = process.try_as_raw_fd() {
                                    kqueue.remove(Event::Fd(fd));
                                }
                                process.kill();
                                processes[index] = None;
                                event_sender.send(ApplicationEvent::ProcessExit {
                                    tag,
                                    success: false,
                                })?;
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

    let mut client_index = 0;
    match connection.read(std::slice::from_mut(&mut client_index)) {
        Ok(1) => (),
        _ => return,
    }

    let client_handle = ClientHandle::from_index(client_index as _).unwrap();
    let is_pipped = is_pipped();

    let stdout = io::stdout();
    let mut application = ClientApplication::new(client_handle, stdout.lock(), is_pipped);
    let bytes = application.init(args);
    if connection.write(bytes).is_err() {
        return;
    }

    let raw_mode;

    let kqueue = Kqueue::new();
    kqueue.add(Event::Fd(connection.as_raw_fd()), 0);
    kqueue.add(Event::Fd(libc::STDIN_FILENO), 1);
    let mut kqueue_events = KqueueEvents::new();

    if is_pipped {
        raw_mode = None;
    } else {
        raw_mode = Some(RawMode::enter());
        kqueue.add(Event::Resize, 2);

        let size = get_terminal_size();
        let bytes = application.update(Some(size), &[], &[], &[]);
        if connection.write(bytes).is_err() {
            return;
        }
    }

    let mut keys = Vec::new();
    let mut buf = Vec::new();

    'main_loop: loop {
        for event in kqueue.wait(&mut kqueue_events, None) {
            let mut resize = None;
            let mut stdin_bytes = &[][..];
            let mut server_bytes = &[][..];

            keys.clear();

            match event {
                Ok(TriggeredEvent { index: 0, data }) => {
                    buf.resize(data as _, 0);
                    match connection.read(&mut buf) {
                        Ok(0) | Err(_) => break 'main_loop,
                        Ok(len) => server_bytes = &buf[..len],
                    }
                }
                Ok(TriggeredEvent { index: 1, data }) => {
                    buf.resize(data as _, 0);
                    match read(libc::STDIN_FILENO, &mut buf) {
                        Ok(0) | Err(()) => {
                            kqueue.remove(Event::Fd(libc::STDIN_FILENO));
                            continue;
                        }
                        Ok(len) => {
                            let bytes = &buf[..len];
                            if is_pipped {
                                stdin_bytes = bytes;
                            } else {
                                parse_terminal_keys(&bytes, &mut keys);
                            }
                        }
                    }
                }
                Ok(TriggeredEvent { index: 2, .. }) => resize = Some(get_terminal_size()),
                Ok(_) => unreachable!(),
                Err(()) => break 'main_loop,
            }

            let bytes = application.update(resize, &keys, stdin_bytes, server_bytes);
            if connection.write(bytes).is_err() {
                break;
            }
        }
    }

    drop(raw_mode);
}

