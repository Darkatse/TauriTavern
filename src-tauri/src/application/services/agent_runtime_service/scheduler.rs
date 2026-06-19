use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::watch;

use super::AgentRuntimeService;
use super::guidance::AgentGuidanceMailbox;
use crate::application::errors::ApplicationError;
use crate::domain::models::agent::{
    AgentDelegationContinuation, AgentInvocationStatus, AgentTaskRecord, AgentTaskStatus,
};

pub(super) struct ActiveRunHandle {
    pub(super) cancel_sender: watch::Sender<bool>,
    pub(super) scheduler: Arc<AgentTaskScheduler>,
    pub(super) guidance_mailbox: Arc<AgentGuidanceMailbox>,
}

impl ActiveRunHandle {
    pub(super) fn new(
        service: &Arc<AgentRuntimeService>,
        run_id: String,
        cancel_sender: watch::Sender<bool>,
    ) -> Self {
        Self {
            cancel_sender,
            scheduler: Arc::new(AgentTaskScheduler::new(service, run_id)),
            guidance_mailbox: Arc::new(AgentGuidanceMailbox::new()),
        }
    }
}

struct AgentTaskWorker {
    cancel_sender: watch::Sender<bool>,
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
        let service = self.service()?;
        let (cancel_sender, mut cancel_receiver) = watch::channel(false);
        {
            let mut workers = self
                .workers
                .lock()
                .expect("agent task scheduler mutex poisoned");
            if workers.contains_key(&task_id) {
                return Err(ApplicationError::InternalError(format!(
                    "agent.task_already_scheduled: task `{task_id}` is already scheduled"
                )));
            }
            workers.insert(task_id.clone(), AgentTaskWorker { cancel_sender });
        }
        self.notify_change();

        let scheduler = Arc::clone(self);
        let run_id = self.run_id.clone();
        tokio::spawn(async move {
            let result = service
                .run_child_task_to_terminal(
                    run_id.as_str(),
                    task_id.as_str(),
                    child_invocation_id.as_str(),
                    &mut cancel_receiver,
                )
                .await;
            if let Err(error) = result {
                tracing::error!(
                    "Agent child task worker failed to record terminal state for task {}: {}",
                    task_id,
                    error
                );
            }
            scheduler.finish_worker(task_id.as_str());
        });

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
        let tasks = tasks
            .into_iter()
            .filter(|task| task.parent_invocation_id == parent_invocation_id)
            .filter(|task| task.continuation == AgentDelegationContinuation::ReturnToParent)
            .filter(task_is_unfinished)
            .collect::<Vec<_>>();
        self.cancel_unfinished_tasks(
            &service,
            tasks,
            "cancelled because the parent Agent finished the run",
        )
        .await
    }

    pub(super) async fn cancel_all_unfinished(&self) -> Result<(), ApplicationError> {
        let service = self.service()?;
        let tasks = service
            .invocation_repository
            .list_tasks(&self.run_id)
            .await?;
        let tasks = tasks
            .into_iter()
            .filter(|task| task.continuation == AgentDelegationContinuation::ReturnToParent)
            .filter(task_is_unfinished)
            .collect::<Vec<_>>();
        self.cancel_unfinished_tasks(&service, tasks, "cancelled because the Agent run stopped")
            .await
    }

    async fn cancel_unfinished_tasks(
        &self,
        service: &AgentRuntimeService,
        tasks: Vec<AgentTaskRecord>,
        reason: &str,
    ) -> Result<(), ApplicationError> {
        for task in tasks {
            let transition = service
                .transition_child_task_with_change(
                    &self.run_id,
                    task.id.as_str(),
                    AgentTaskStatus::Cancelled,
                    None,
                    Some(reason.to_string()),
                )
                .await?;
            if !transition.changed {
                continue;
            }
            self.cancel_task_worker(task.id.as_str());
            service
                .finish_child_invocation(
                    &self.run_id,
                    task.child_invocation_id.as_str(),
                    AgentInvocationStatus::Cancelled,
                )
                .await?;
        }
        self.notify_change();
        Ok(())
    }

    pub(super) fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn cancel_task_worker(&self, task_id: &str) {
        let mut workers = self
            .workers
            .lock()
            .expect("agent task scheduler mutex poisoned");
        if let Some(worker) = workers.remove(task_id) {
            let _ = worker.cancel_sender.send(true);
        }
    }

    fn finish_worker(&self, task_id: &str) {
        self.workers
            .lock()
            .expect("agent task scheduler mutex poisoned")
            .remove(task_id);
        self.notify_change();
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

    pub(super) async fn cancel_unfinished_child_tasks(
        &self,
        run_id: &str,
    ) -> Result<(), ApplicationError> {
        let Some(handle) = self.active_runs.read().await.get(run_id).cloned() else {
            return Ok(());
        };
        handle.scheduler.cancel_all_unfinished().await
    }
}
