use std::{io::Read, process::Child, sync::mpsc, thread};

use crate::{
    client::TargetClient,
    client_event::LocalEvent,
    script::{ScriptEngineRef, ScriptResult, ScriptValue},
};

#[derive(Clone, Copy)]
pub struct TaskHandle(usize);
impl TaskHandle {
    pub fn into_index(self) -> usize {
        self.0
    }
}

pub enum TaskRequest {
    Stop,
    ChildStream(Child),
}

pub enum TaskResult {
    Finished,
    ChildPartialOutput(String),
}
impl TaskResult {
    pub fn to_script_value<'script>(
        &self,
        engine: ScriptEngineRef<'script>,
    ) -> ScriptResult<ScriptValue<'script>> {
        match self {
            TaskResult::Finished => Ok(ScriptValue::Nil),
            TaskResult::ChildPartialOutput(output) => {
                let output = engine.create_string(output.as_bytes())?;
                Ok(ScriptValue::String(output))
            }
        }
    }
}

pub struct TaskManager {
    task_sender: mpsc::Sender<Task>,
    worker: TaskWorker,
    next_handle: TaskHandle,
}

struct Task {
    target_client: TargetClient,
    handle: TaskHandle,
    request: TaskRequest,
}

impl TaskManager {
    pub fn new(event_sender: mpsc::Sender<LocalEvent>) -> Self {
        let (task_sender, task_receiver) = mpsc::channel();
        let worker = TaskWorker::new(task_receiver, event_sender);
        Self {
            task_sender,
            worker,
            next_handle: TaskHandle(0),
        }
    }

    pub fn request(&mut self, target_client: TargetClient, task: TaskRequest) -> TaskHandle {
        let handle = self.next_handle;
        self.next_handle.0 += 1;
        let _ = self.task_sender.send(Task {
            target_client,
            handle,
            request: task,
        });
        handle
    }
}

impl Drop for TaskManager {
    fn drop(&mut self) {
        self.worker.stop(&self.task_sender);
    }
}

struct TaskWorker {
    _join_handle: thread::JoinHandle<()>,
}
impl TaskWorker {
    pub fn new(
        task_receiver: mpsc::Receiver<Task>,
        event_sender: mpsc::Sender<LocalEvent>,
    ) -> Self {
        let join_handle = thread::spawn(move || Self::work(task_receiver, event_sender));
        Self {
            _join_handle: join_handle,
        }
    }

    pub fn stop(&self, task_sender: &mpsc::Sender<Task>) {
        let _ = task_sender.send(Task {
            target_client: TargetClient::Local,
            handle: TaskHandle(0),
            request: TaskRequest::Stop,
        });
    }

    fn work(task_receiver: mpsc::Receiver<Task>, event_sender: mpsc::Sender<LocalEvent>) {
        loop {
            let task = match task_receiver.recv() {
                Ok(task) => task,
                Err(_) => break,
            };

            macro_rules! send_result {
                ($result:expr) => {{
                    let event = LocalEvent::TaskEvent(task.target_client, task.handle, $result);
                    if let Err(_) = event_sender.send(event) {
                        break;
                    }
                }};
            }

            match task.request {
                TaskRequest::Stop => break,
                TaskRequest::ChildStream(child) => {
                    eprintln!("child stream request");
                    if let Some(mut stdout) = child.stdout {
                        let mut buf = Vec::new();
                        let mut buf_len = 0;
                        loop {
                            let target_len = buf_len + 2048;
                            if target_len > buf.len() {
                                buf.resize(target_len, 0);
                            }

                            match stdout.read(&mut buf[buf_len..]) {
                                Ok(0) | Err(_) => break,
                                Ok(len) => buf_len += len,
                            }

                            let last_line_end_index;
                            match buf[..buf_len].iter().rposition(|b| *b == b'\n') {
                                Some(i) => last_line_end_index = i + 1,
                                None => continue,
                            }

                            let output =
                                String::from_utf8_lossy(&buf[..last_line_end_index]).into();
                            buf.copy_within(..last_line_end_index, 0);
                            buf_len -= last_line_end_index;

                            eprintln!("output:\n{}\n---\n", &output);
                            send_result!(TaskResult::ChildPartialOutput(output));
                        }

                        let output = String::from_utf8_lossy(&buf[..buf_len]).into();
                        eprintln!("final output:\n{}\n---\n", &output);
                        send_result!(TaskResult::ChildPartialOutput(output));
                    } else {
                        eprintln!("num tinha stdout");
                    }

                    send_result!(TaskResult::Finished);
                }
            }
        }
    }
}
