use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::sync::{
    SyncEndpointRef, SyncExecutionKind, SyncIntent, SyncJob, SyncJobOutcome, SyncJobReport,
    SyncJobRequest,
};

#[async_trait]
pub trait SyncJobExecutor: Send + Sync {
    async fn execute(&self, job: SyncJob) -> Result<SyncJobOutcome, DomainError>;
}

pub struct SyncJobCoordinator {
    gate: Arc<Semaphore>,
    executor: Arc<dyn SyncJobExecutor>,
    active: Arc<Mutex<Option<ActiveSyncJob>>>,
}

impl SyncJobCoordinator {
    pub fn new(executor: Arc<dyn SyncJobExecutor>) -> Self {
        Self {
            gate: Arc::new(Semaphore::new(1)),
            executor,
            active: Arc::new(Mutex::new(None)),
        }
    }

    pub fn try_start(&self, request: SyncJobRequest) -> Result<StartedSyncJob, SyncJobReport> {
        let job = build_job(request);
        let (permit, active_guard) = if job.execution == SyncExecutionKind::RequestRemotePull {
            (None, None)
        } else {
            let permit = match self.gate.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => return Err(SyncJobReport::failed(job, "Sync job already running")),
            };

            {
                let mut active = self.active.lock().expect("sync active job lock poisoned");
                *active = Some(ActiveSyncJob { id: job.id.clone() });
            }

            (
                Some(permit),
                Some(ActiveJobGuard {
                    id: job.id.clone(),
                    active: self.active.clone(),
                }),
            )
        };

        Ok(StartedSyncJob {
            job,
            executor: self.executor.clone(),
            _permit: permit,
            _active_guard: active_guard,
        })
    }

    pub async fn run(&self, request: SyncJobRequest) -> SyncJobReport {
        match self.try_start(request) {
            Ok(started) => started.execute().await.finish(),
            Err(report) => report,
        }
    }
}

pub struct StartedSyncJob {
    job: SyncJob,
    executor: Arc<dyn SyncJobExecutor>,
    _permit: Option<OwnedSemaphorePermit>,
    _active_guard: Option<ActiveJobGuard>,
}

impl StartedSyncJob {
    pub async fn execute(self) -> ExecutedSyncJob {
        let Self {
            job,
            executor,
            _permit,
            _active_guard,
        } = self;
        let result = executor.execute(job.clone()).await;
        ExecutedSyncJob {
            job,
            result,
            _permit,
            _active_guard,
        }
    }
}

pub struct ExecutedSyncJob {
    job: SyncJob,
    result: Result<SyncJobOutcome, DomainError>,
    _permit: Option<OwnedSemaphorePermit>,
    _active_guard: Option<ActiveJobGuard>,
}

impl ExecutedSyncJob {
    pub fn outcome(&self) -> Option<&SyncJobOutcome> {
        self.result.as_ref().ok()
    }

    pub fn finish(self) -> SyncJobReport {
        match self.result {
            Ok(outcome) => SyncJobReport::from_outcome(self.job, outcome),
            Err(error) => SyncJobReport::failed(self.job, error.to_string()),
        }
    }

    pub fn finish_or_error(self) -> Result<SyncJobReport, DomainError> {
        match self.result {
            Ok(outcome) => Ok(SyncJobReport::from_outcome(self.job, outcome)),
            Err(error) => Err(error),
        }
    }

    pub fn finish_with_error(self, error: DomainError) -> SyncJobReport {
        SyncJobReport::failed(self.job, error.to_string())
    }
}

struct ActiveSyncJob {
    id: String,
}

struct ActiveJobGuard {
    id: String,
    active: Arc<Mutex<Option<ActiveSyncJob>>>,
}

impl Drop for ActiveJobGuard {
    fn drop(&mut self) {
        let mut active = self.active.lock().expect("sync active job lock poisoned");
        if active.as_ref().is_some_and(|job| job.id == self.id) {
            *active = None;
        }
    }
}

fn build_job(request: SyncJobRequest) -> SyncJob {
    let execution = resolve_execution(&request.endpoint, request.intent);
    SyncJob {
        id: Uuid::new_v4().to_string(),
        endpoint: request.endpoint,
        intent: request.intent,
        execution,
        origin: request.origin,
        policy: request.policy,
    }
}

fn resolve_execution(endpoint: &SyncEndpointRef, intent: SyncIntent) -> SyncExecutionKind {
    match (endpoint, intent) {
        (SyncEndpointRef::LanPeer { .. }, SyncIntent::PullToLocal) => SyncExecutionKind::Pull,
        (SyncEndpointRef::LanPeer { .. }, SyncIntent::ReplicateLocalToRemote) => {
            SyncExecutionKind::RequestRemotePull
        }
        (SyncEndpointRef::RemoteServer { .. }, SyncIntent::PullToLocal) => SyncExecutionKind::Pull,
        (SyncEndpointRef::RemoteServer { .. }, SyncIntent::ReplicateLocalToRemote) => {
            SyncExecutionKind::DirectPush
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::models::sync::{
        ResolvedSyncPolicy, SyncJobReportResult, SyncJobSummary, SyncOperationOptions, SyncOrigin,
    };
    use ttsync_contract::peer::DeviceId;
    use ttsync_contract::sync::SyncMode;

    struct NoopExecutor;

    #[async_trait]
    impl SyncJobExecutor for NoopExecutor {
        async fn execute(&self, job: SyncJob) -> Result<SyncJobOutcome, DomainError> {
            if job.execution == SyncExecutionKind::RequestRemotePull {
                return Ok(SyncJobOutcome::RemoteRequestAccepted);
            }

            Ok(SyncJobOutcome::Completed {
                summary: SyncJobSummary::new(0, 0, 0),
            })
        }
    }

    fn request(intent: SyncIntent) -> SyncJobRequest {
        SyncJobRequest {
            endpoint: SyncEndpointRef::LanPeer {
                device_id: DeviceId::new("11111111-1111-4111-8111-111111111111".to_string())
                    .unwrap(),
            },
            intent,
            origin: SyncOrigin::Manual,
            policy: ResolvedSyncPolicy::Transfer {
                mode: SyncMode::Incremental,
                options: SyncOperationOptions::default(),
            },
        }
    }

    fn remote_pull_request() -> SyncJobRequest {
        SyncJobRequest {
            policy: ResolvedSyncPolicy::RemotePullRequest {
                options: SyncOperationOptions::default(),
            },
            ..request(SyncIntent::ReplicateLocalToRemote)
        }
    }

    #[tokio::test]
    async fn resolves_lan_replicate_as_remote_pull_request() {
        let coordinator = SyncJobCoordinator::new(Arc::new(NoopExecutor));
        let report = coordinator.run(remote_pull_request()).await;

        assert_eq!(report.job.execution, SyncExecutionKind::RequestRemotePull);
        assert!(matches!(
            report.result,
            SyncJobReportResult::RemoteRequestAccepted
        ));
    }

    #[test]
    fn busy_report_mentions_running_job() {
        let coordinator = SyncJobCoordinator::new(Arc::new(NoopExecutor));
        let _started = coordinator
            .try_start(request(SyncIntent::PullToLocal))
            .expect("first job should start");

        let report = match coordinator.try_start(request(SyncIntent::PullToLocal)) {
            Ok(_) => panic!("second job should be rejected"),
            Err(report) => report,
        };

        assert!(
            report
                .failure_message()
                .unwrap()
                .contains("already running")
        );
    }

    #[tokio::test]
    async fn remote_pull_request_does_not_wait_for_transfer_gate() {
        let coordinator = SyncJobCoordinator::new(Arc::new(NoopExecutor));
        let _started = coordinator
            .try_start(request(SyncIntent::PullToLocal))
            .expect("transfer job should start");

        let report = coordinator.run(remote_pull_request()).await;

        assert!(matches!(
            report.result,
            SyncJobReportResult::RemoteRequestAccepted
        ));
    }

    #[test]
    fn active_guard_only_clears_its_own_job() {
        let active = Arc::new(Mutex::new(Some(ActiveSyncJob {
            id: "old".to_string(),
        })));
        let guard = ActiveJobGuard {
            id: "old".to_string(),
            active: active.clone(),
        };

        {
            let mut active_job = active.lock().unwrap();
            *active_job = Some(ActiveSyncJob {
                id: "new".to_string(),
            });
        }

        drop(guard);

        assert_eq!(active.lock().unwrap().as_ref().unwrap().id, "new");
    }
}
