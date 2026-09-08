use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::AgentRuntimeService;
use super::checkpoint::RunCheckpoint;
use super::continuation::InvocationFrame;
use super::guidance::AgentGuidanceMailbox;
use super::model_stream_projection::AgentRunLiveProjection;
use crate::errors::ApplicationError;
use tt_domain::models::agent::{AgentDelegationContinuation, AgentTaskRecord, AgentTaskStatus};

pub(super) struct ActiveRunHandle {
    pub(super) cancel_sender: watch::Sender<bool>,
    pub(super) scheduler: Arc<AgentTaskScheduler>,
    pub(super) guidance_mailbox: Arc<AgentGuidanceMailbox>,
    pub(super) stream_override: Option<bool>,
    pub(super) host_presentation: bool,
    pub(super) pending_checkpoint: tokio::sync::Mutex<Option<RunCheckpoint>>,
    pub(super) live_projection: watch::Sender<AgentRunLiveProjection>,
}

impl ActiveRunHandle {
    pub(super) fn new(
        service: &Arc<AgentRuntimeService>,
        run_id: String,
        cancel_sender: watch::Sender<bool>,
        stream_override: Option<bool>,
        host_presentation: bool,
    ) -> Self {
        let (live_projection, _) = watch::channel(AgentRunLiveProjection::default());
        Self {
            cancel_sender,
            scheduler: Arc::new(AgentTaskScheduler::new(service, run_id)),
            guidance_mailbox: Arc::new(AgentGuidanceMailbox::new()),
            stream_override,
            host_presentation,
            pending_checkpoint: tokio::sync::Mutex::new(None),
            live_projection,
        }
    }

    pub(super) fn stream_enabled(&self, profile_default: bool) -> bool {
        self.stream_override.unwrap_or(profile_default)
    }
}

struct AgentTaskWorker {
    cancel_sender: watch::Sender<bool>,
    join: JoinHandle<Result<Option<InvocationFrame>, ApplicationError>>,
}

pub(super) struct AgentTaskScheduler {
    run_id: String,
    service: Weak<AgentRuntimeService>,
    workers: Mutex<HashMap<String, AgentTaskWorker>>,
    change_seq: AtomicU64,
    changes: watch::Sender<u64>,
}

impl AgentTaskScheduler {
    fn new(service: &Arc<AgentRuntimeService>, run_id: String) -> Self {
        let (changes, _) = watch::channel(0_u64);
        Self {
            run_id,
            service: Arc::downgrade(service),
            workers: Mutex::new(HashMap::new()),
            change_seq: AtomicU64::new(0),
            changes,
        }
    }

    pub(super) fn submit(
        self: &Arc<Self>,
        task_id: String,
        child_invocation_id: String,
    ) -> Result<(), ApplicationError> {
        self.submit_worker(task_id, child_invocation_id, None)
    }

    pub(super) fn submit_resumed(
        self: &Arc<Self>,
        frame: InvocationFrame,
    ) -> Result<(), ApplicationError> {
        let task_id = frame.prepared.delegation_task_id.clone().ok_or_else(|| {
            ApplicationError::InternalError(format!(
                "agent.child_task_id_missing: resumed invocation `{}` has no task",
                frame.prepared.invocation.id
            ))
        })?;
        self.submit_worker(task_id, frame.prepared.invocation.id.clone(), Some(frame))
    }

    fn submit_worker(
        self: &Arc<Self>,
        task_id: String,
        child_invocation_id: String,
        frame: Option<InvocationFrame>,
    ) -> Result<(), ApplicationError> {
        let service = self.service()?;
        let (cancel_sender, mut cancel_receiver) = watch::channel(false);
        let mut workers = self
            .workers
            .lock()
            .expect("agent task scheduler mutex poisoned");
        if workers.contains_key(&task_id) {
            return Err(ApplicationError::InternalError(format!(
                "agent.task_already_scheduled: task `{task_id}` is already scheduled"
            )));
        }
        let scheduler = Arc::clone(self);
        let run_id = self.run_id.clone();
        let worker_task_id = task_id.clone();
        let join = tokio::spawn(async move {
            let result = service
                .run_child_task_to_terminal(
                    run_id.as_str(),
                    worker_task_id.as_str(),
                    child_invocation_id.as_str(),
                    frame,
                    &mut cancel_receiver,
                )
                .await;
            if let Err(error) = &result {
                tracing::error!(
                    target: tt_contracts::observability::USER_VISIBLE_ERROR,
                    "Agent child task worker failed for task {}: {}",
                    worker_task_id,
                    error
                );
            }
            scheduler.notify_change();
            result
        });
        workers.insert(
            task_id,
            AgentTaskWorker {
                cancel_sender,
                join,
            },
        );
        self.notify_change();

        Ok(())
    }

    pub(super) async fn cancel_unfinished_for_parent(
        &self,
        parent_invocation_id: &str,
    ) -> Result<(), ApplicationError> {
        let service = self.service()?;
        let tasks = service
            .invocation_repository
            .list_tasks(&self.run_id)
            .await?;
        for task in tasks
            .into_iter()
            .filter(|task| task.parent_invocation_id == parent_invocation_id)
            .filter(|task| task.continuation == AgentDelegationContinuation::ReturnToParent)
            .filter(task_is_unfinished)
        {
            self.cancel_task_worker(&task.id);
        }
        Ok(())
    }

    pub(super) async fn cancel_all_unfinished(&self) -> Result<(), ApplicationError> {
        for worker in self
            .workers
            .lock()
            .expect("agent task scheduler mutex poisoned")
            .values()
        {
            if !worker.join.is_finished() {
                let _ = worker.cancel_sender.send(true);
            }
        }
        Ok(())
    }

    pub(super) async fn stop_and_join(&self) -> Result<Vec<InvocationFrame>, ApplicationError> {
        let workers = std::mem::take(
            &mut *self
                .workers
                .lock()
                .expect("agent task scheduler mutex poisoned"),
        );
        for worker in workers.values() {
            if !worker.join.is_finished() {
                let _ = worker.cancel_sender.send(true);
            }
        }
        let mut frames = Vec::new();
        let mut first_error = None;
        // Every worker must exit before the run can publish a consistent checkpoint.
        for (task_id, worker) in workers {
            let result = worker
                .join
                .await
                .map_err(|error| {
                    ApplicationError::InternalError(format!(
                        "agent.child_worker_failed: task `{task_id}` did not exit normally: {error}"
                    ))
                })
                .flatten();
            match result {
                Ok(Some(frame)) => frames.push(frame),
                Ok(None) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(frames),
        }
    }

    pub(super) fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn cancel_task_worker(&self, task_id: &str) {
        let workers = self
            .workers
            .lock()
            .expect("agent task scheduler mutex poisoned");
        if let Some(worker) = workers.get(task_id)
            && !worker.join.is_finished()
        {
            let _ = worker.cancel_sender.send(true);
        }
    }

    fn notify_change(&self) {
        let next = self.change_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.changes.send(next);
    }

    fn service(&self) -> Result<Arc<AgentRuntimeService>, ApplicationError> {
        self.service.upgrade().ok_or_else(|| {
            ApplicationError::InternalError(format!(
                "agent.runtime_dropped: active run `{}` no longer has a runtime service",
                self.run_id
            ))
        })
    }
}

fn task_is_unfinished(task: &AgentTaskRecord) -> bool {
    matches!(
        task.status,
        AgentTaskStatus::Queued | AgentTaskStatus::Running
    )
}

impl AgentRuntimeService {
    pub(super) async fn active_run_handle(
        &self,
        run_id: &str,
    ) -> Result<Arc<ActiveRunHandle>, ApplicationError> {
        self.active_runs
            .read()
            .await
            .get(run_id)
            .cloned()
            .ok_or_else(|| {
                ApplicationError::InternalError(format!(
                    "agent.active_run_missing: active run handle for `{run_id}` is missing"
                ))
            })
    }
}
