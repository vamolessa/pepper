use std::{process::Child, sync::mpsc, thread};

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
    ChildPartialOutput(Option<String>),
}
impl TaskResult {
    pub fn to_script_value<'script>(
        &self,
        engine: ScriptEngineRef<'script>,
    ) -> ScriptResult<ScriptValue<'script>> {
        match self {
            TaskResult::ChildPartialOutput(output) => match output {
                Some(output) => {
                    let output = engine.create_string(output.as_bytes())?;
                    Ok(ScriptValue::String(output))
                }
                None => Ok(ScriptValue::Nil),
            },
        }
    }
}

pub struct TaskManager {
    task_sender: mpsc::Sender<Task>,
    worker: TaskWorker,
    next_handle: TaskHandle,
}

struct Task {
    handle: TaskHandle,
    target_client: TargetClient,
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
            handle,
            target_client,
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
            handle: TaskHandle(0),
            target_client: TargetClient::Local,
            request: TaskRequest::Stop,
        });
    }

    fn work(task_receiver: mpsc::Receiver<Task>, event_sender: mpsc::Sender<LocalEvent>) {
        loop {
            let task = match task_receiver.recv() {
                Ok(task) => task,
                Err(_) => break,
            };

            match task.request {
                TaskRequest::Stop => break,
                TaskRequest::ChildStream(child) => {
                    //
                }
            }
        }
    }
}
