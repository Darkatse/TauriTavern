use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use crate::application::services::data_change_reconciler::DataChangeReconciler;
use crate::domain::errors::DomainError;
use crate::domain::models::sync::{
    SyncEndpointRef, SyncExecutionFailure, SyncExecutionKind, SyncExecutionReport, SyncIntent,
    SyncJob, SyncJobOutcome, SyncJobReport, SyncJobRequest,
};

#[async_trait]
pub trait SyncJobExecutor: Send + Sync {
    async fn execute(&self, job: SyncJob) -> Result<SyncExecutionReport, SyncExecutionFailure>;
}

pub struct SyncJobCoordinator {
    gate: Arc<Semaphore>,
    executor: Arc<dyn SyncJobExecutor>,
    reconciler: Arc<dyn DataChangeReconciler>,
    active: Arc<Mutex<Option<ActiveSyncJob>>>,
}

impl SyncJobCoordinator {
    pub fn new(
        executor: Arc<dyn SyncJobExecutor>,
        reconciler: Arc<dyn DataChangeReconciler>,
    ) -> Self {
        Self {
            gate: Arc::new(Semaphore::new(1)),
            executor,
            reconciler,
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
                Err(_) => {
                    return Err(SyncJobReport::failed_without_local_mutation(
                        job,
                        "Sync job already running",
                    ));
                }
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
            reconciler: self.reconciler.clone(),
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
    reconciler: Arc<dyn DataChangeReconciler>,
    _permit: Option<OwnedSemaphorePermit>,
    _active_guard: Option<ActiveJobGuard>,
}

impl StartedSyncJob {
    pub async fn execute(self) -> ExecutedSyncJob {
        let Self {
            job,
            executor,
            reconciler,
            _permit,
            _active_guard,
        } = self;
        let result =
            finalize_execution(&job, executor.execute(job.clone()).await, &*reconciler).await;
        ExecutedSyncJob {
            result,
            _permit,
            _active_guard,
        }
    }
}

pub struct ExecutedSyncJob {
    result: SyncJobReportResultWithError,
    _permit: Option<OwnedSemaphorePermit>,
    _active_guard: Option<ActiveJobGuard>,
}

impl ExecutedSyncJob {
    pub fn finish(self) -> SyncJobReport {
        self.result.report
    }

    pub fn finish_or_error(self) -> Result<SyncJobReport, DomainError> {
        match self.result.error {
            Some(error) => Err(error),
            None => Ok(self.result.report),
        }
    }
}

struct SyncJobReportResultWithError {
    report: SyncJobReport,
    error: Option<DomainError>,
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

async fn finalize_execution(
    job: &SyncJob,
    execution: Result<SyncExecutionReport, SyncExecutionFailure>,
    reconciler: &dyn DataChangeReconciler,
) -> SyncJobReportResultWithError {
    match execution {
        Ok(report) => finalize_success(job, report, reconciler).await,
        Err(failure) => finalize_failure(job, failure, reconciler).await,
    }
}

async fn finalize_success(
    job: &SyncJob,
    report: SyncExecutionReport,
    reconciler: &dyn DataChangeReconciler,
) -> SyncJobReportResultWithError {
    if report.local_applied.changed() {
        if let Err(error) = reconciler.reconcile(reconcile_reason(job)).await {
            tracing::warn!(
                job_id = job.id,
                error = %error,
                "Sync completed but data reconciliation failed"
            );
            let message = match report.outcome {
                SyncJobOutcome::Completed { .. } => {
                    format!("Sync completed but failed to refresh runtime caches: {error}")
                }
                SyncJobOutcome::RemoteRequestAccepted => error.to_string(),
            };
            return SyncJobReportResultWithError {
                report: SyncJobReport::failed_after_partial_local_mutation(
                    job.clone(),
                    message,
                    report.local_applied,
                    Some(error.to_string()),
                ),
                error: Some(error),
            };
        }
    }

    SyncJobReportResultWithError {
        report: SyncJobReport::from_outcome(job.clone(), report.outcome),
        error: None,
    }
}

async fn finalize_failure(
    job: &SyncJob,
    failure: SyncExecutionFailure,
    reconciler: &dyn DataChangeReconciler,
) -> SyncJobReportResultWithError {
    let SyncExecutionFailure {
        error,
        local_applied,
    } = failure;
    let message = error.to_string();

    let report = if local_applied.changed() {
        let reconcile_error =
            reconciler
                .reconcile(reconcile_reason(job))
                .await
                .err()
                .map(|error| {
                    tracing::warn!(
                        job_id = job.id,
                        error = %error,
                        "Sync failed after local data changed and reconciliation also failed"
                    );
                    error.to_string()
                });

        SyncJobReport::failed_after_partial_local_mutation(
            job.clone(),
            message,
            local_applied,
            reconcile_error,
        )
    } else {
        SyncJobReport::failed_without_local_mutation(job.clone(), message)
    };

    SyncJobReportResultWithError {
        report,
        error: Some(error),
    }
}

fn reconcile_reason(job: &SyncJob) -> &'static str {
    match &job.endpoint {
        SyncEndpointRef::LanPeer { .. } => "lan_sync",
        SyncEndpointRef::RemoteServer { .. } => "tt_sync_pull",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::models::sync::{
        LocalAppliedChangeSummary, ResolvedSyncPolicy, SyncJobReportResult, SyncJobSummary,
        SyncOperationOptions, SyncOrigin,
    };
    use ttsync_contract::peer::DeviceId;
    use ttsync_contract::sync::SyncMode;

    struct NoopExecutor;

    #[async_trait]
    impl SyncJobExecutor for NoopExecutor {
        async fn execute(&self, job: SyncJob) -> Result<SyncExecutionReport, SyncExecutionFailure> {
            if job.execution == SyncExecutionKind::RequestRemotePull {
                return Ok(SyncExecutionReport::remote_request_accepted());
            }

            Ok(SyncExecutionReport::completed(
                SyncJobSummary::new(0, 0, 0),
                LocalAppliedChangeSummary::default(),
            ))
        }
    }

    struct NoopReconciler;

    #[async_trait]
    impl DataChangeReconciler for NoopReconciler {
        async fn reconcile(&self, _reason: &str) -> Result<(), DomainError> {
            Ok(())
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
        let coordinator = SyncJobCoordinator::new(Arc::new(NoopExecutor), Arc::new(NoopReconciler));
        let report = coordinator.run(remote_pull_request()).await;

        assert_eq!(report.job.execution, SyncExecutionKind::RequestRemotePull);
        assert!(matches!(
            report.result,
            SyncJobReportResult::RemoteRequestAccepted
        ));
    }

    #[test]
    fn busy_report_mentions_running_job() {
        let coordinator = SyncJobCoordinator::new(Arc::new(NoopExecutor), Arc::new(NoopReconciler));
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
        let coordinator = SyncJobCoordinator::new(Arc::new(NoopExecutor), Arc::new(NoopReconciler));
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

    struct PartialFailureExecutor;

    #[async_trait]
    impl SyncJobExecutor for PartialFailureExecutor {
        async fn execute(
            &self,
            _job: SyncJob,
        ) -> Result<SyncExecutionReport, SyncExecutionFailure> {
            let local_applied = LocalAppliedChangeSummary {
                files_written: 1,
                bytes_written: 7,
                files_deleted: 0,
                target_changed: false,
            };
            Err(SyncExecutionFailure::new(
                DomainError::InternalError("download failed".to_string()),
                local_applied,
            ))
        }
    }

    struct FailingReconciler;

    #[async_trait]
    impl DataChangeReconciler for FailingReconciler {
        async fn reconcile(&self, _reason: &str) -> Result<(), DomainError> {
            Err(DomainError::InternalError("cache stale".to_string()))
        }
    }

    #[tokio::test]
    async fn partial_failure_reports_reconcile_error() {
        let coordinator = SyncJobCoordinator::new(
            Arc::new(PartialFailureExecutor),
            Arc::new(FailingReconciler),
        );

        let report = coordinator.run(request(SyncIntent::PullToLocal)).await;

        match report.result {
            SyncJobReportResult::Failed {
                local_applied,
                reconcile_error,
                ..
            } => {
                assert_eq!(local_applied.files_written, 1);
                assert_eq!(local_applied.bytes_written, 7);
                assert_eq!(
                    reconcile_error.as_deref(),
                    Some("Internal error: cache stale")
                );
            }
            other => panic!("unexpected report: {other:?}"),
        }
    }

    struct CompletedWithMutationExecutor;

    #[async_trait]
    impl SyncJobExecutor for CompletedWithMutationExecutor {
        async fn execute(
            &self,
            _job: SyncJob,
        ) -> Result<SyncExecutionReport, SyncExecutionFailure> {
            let local_applied = LocalAppliedChangeSummary {
                files_written: 0,
                bytes_written: 0,
                files_deleted: 1,
                target_changed: false,
            };
            Ok(SyncExecutionReport::completed(
                SyncJobSummary::new(0, 0, 1),
                local_applied,
            ))
        }
    }

    #[tokio::test]
    async fn completed_with_reconcile_failure_is_failed() {
        let coordinator = SyncJobCoordinator::new(
            Arc::new(CompletedWithMutationExecutor),
            Arc::new(FailingReconciler),
        );

        let report = coordinator.run(request(SyncIntent::PullToLocal)).await;

        assert!(report.failure_message().is_some());
        assert!(report.completed_summary().is_none());
        match report.result {
            SyncJobReportResult::Failed {
                local_applied,
                reconcile_error,
                ..
            } => {
                assert_eq!(local_applied.files_deleted, 1);
                assert_eq!(
                    reconcile_error.as_deref(),
                    Some("Internal error: cache stale")
                );
            }
            other => panic!("unexpected report: {other:?}"),
        }
    }
}
