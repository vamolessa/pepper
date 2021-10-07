use std::{
    io,
    os::unix::{
        io::{AsRawFd, RawFd},
        net::{UnixListener, UnixStream},
    },
    time::Duration,
};

use crate::{
    application::{ApplicationContext, ClientApplication, ServerApplication},
    client::ClientHandle,
    platform::{Key, PlatformEvent, PlatformProcessHandle, PlatformRequest},
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

pub fn main(ctx: ApplicationContext) {
    run(ctx, run_server, run_client);
}

fn errno() -> libc::c_int {
    unsafe { *libc::__error() }
}

enum Event {
    Resize,
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

        let mut len = unsafe {
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
            if errno() == libc::EINTR {
                len = 0;
            } else {
                panic!("could not wait for events");
            }
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

fn run_server(ctx: ApplicationContext, listener: UnixListener) {
    use io::Write;

    const NONE_PROCESS: Option<Process> = None;

    let mut application = match ServerApplication::new(ctx) {
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

    let kqueue = Kqueue::new();
    kqueue.add(Event::Fd(listener.as_raw_fd()), 0);
    let mut kqueue_events = KqueueEvents::new();

    loop {
        let kqueue_events = kqueue.wait(&mut kqueue_events, timeout);
        if kqueue_events.len() == 0 {
            match timeout {
                Some(Duration::ZERO) => timeout = Some(ServerApplication::idle_duration()),
                Some(_) => {
                    events.push(PlatformEvent::Idle);
                    timeout = None;
                }
                None => unreachable!(),
            }
        }

        for event in kqueue_events {
            let (event_index, event_data) = match event {
                Ok(event) => (event.index, event.data),
                Err(()) => return,
            };

            match event_index {
                0 => {
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
                                        events.push(PlatformEvent::ConnectionOpen { handle });
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
                        match read_from_connection(
                            connection,
                            &mut application.platform.buf_pool,
                            event_data as _,
                        ) {
                            Ok(buf) => events.push(PlatformEvent::ConnectionOutput { handle, buf }),
                            Err(()) => {
                                kqueue.remove(Event::Fd(connection.as_raw_fd()));
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
                            Ok(Some(buf)) => events.push(PlatformEvent::ProcessExit { tag }),
                            Err(()) => {
                                if let Some(fd) = process.try_as_raw_fd() {
                                    kqueue.remove(Event::Fd(fd));
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
                                kqueue.remove(Event::Fd(connection.as_raw_fd()));
                                client_connections[index] = None;
                                events.push(PlatformEvent::ConnectionClose { handle });
                            }
                        }
                        application.platform.buf_pool.release(buf);
                    }
                    PlatformRequest::CloseClient { handle } => {
                        let index = handle.into_index();
                        if let Some(connection) = client_connections[index].take() {
                            kqueue.remove(Event::Fd(connection.as_raw_fd()));
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

                            let handle = PlatformProcessHandle(i as _);
                            if let Ok(child) = command.spawn() {
                                let process = Process::new(child, tag, buf_len);
                                if let Some(fd) = process.try_as_raw_fd() {
                                    kqueue.add(Event::Fd(fd), PROCESSES_START_INDEX + i);
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
                                    kqueue.remove(Event::Fd(fd));
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
                                kqueue.remove(Event::Fd(fd));
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

    let kqueue = Kqueue::new();
    kqueue.add(Event::Fd(connection.as_raw_fd()), 1);
    if is_pipped(libc::STDIN_FILENO) {
        kqueue.add(Event::Fd(libc::STDIN_FILENO), 3);
    }

    let mut kqueue_events = KqueueEvents::new();

    if let Some(terminal) = &terminal {
        terminal.enter_raw_mode();
        kqueue.add(Event::Fd(terminal.as_raw_fd()), 0);

        kqueue.add(Event::Resize, 2);

        let size = terminal.get_size();
        let (_, bytes) = application.update(Some(size), &[Key::None], None, &[]);
        if connection.write_all(bytes).is_err() {
            return;
        }
    }

    if is_pipped(libc::STDOUT_FILENO) {
        let (_, bytes) = application.update(None, &[], Some(&[]), &[]);
        if connection.write_all(bytes).is_err() {
            return;
        }
    }

    let mut keys = Vec::new();
    let mut buf = Vec::new();

    'main_loop: loop {
        for event in kqueue.wait(&mut kqueue_events, None) {
            let mut resize = None;
            let mut stdin_bytes = None;
            let mut server_bytes = &[][..];

            keys.clear();

            match event {
                Ok(TriggeredEvent { index: 0, data }) => {
                    if let Some(terminal) = &terminal {
                        buf.resize(data as _, 0);
                        match read(terminal.as_raw_fd(), &mut buf) {
                            Ok(0) | Err(()) => break 'main_loop,
                            Ok(len) => terminal.parse_keys(&buf[..len], &mut keys),
                        }
                    }
                }
                Ok(TriggeredEvent { index: 1, data }) => {
                    buf.resize(data as _, 0);
                    match connection.read(&mut buf) {
                        Ok(0) | Err(_) => break 'main_loop,
                        Ok(len) => server_bytes = &buf[..len],
                    }
                }
                Ok(TriggeredEvent { index: 2, .. }) => {
                    resize = terminal.as_ref().map(Terminal::get_size);
                }
                Ok(TriggeredEvent { index: 3, data }) => {
                    buf.resize(data as _, 0);
                    match read(libc::STDIN_FILENO, &mut buf) {
                        Ok(0) | Err(()) => {
                            kqueue.remove(Event::Fd(libc::STDIN_FILENO));
                            stdin_bytes = Some(&[][..]);
                        }
                        Ok(len) => stdin_bytes = Some(&buf[..len]),
                    }
                }
                Ok(_) => unreachable!(),
                Err(()) => break 'main_loop,
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
