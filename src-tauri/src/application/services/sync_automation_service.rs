use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::sync::{Mutex, Notify};
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::sync::SyncMode;

use crate::application::services::sync_job_coordinator::SyncJobCoordinator;
use crate::domain::errors::DomainError;
use crate::domain::models::lan_sync::LanServerSettings;
use crate::domain::models::sync::{
    ResolvedSyncPolicy, SyncEndpointRef, SyncIntent, SyncJobReportResult, SyncJobRequest,
    SyncOperationOptions, SyncOrigin,
};
use crate::domain::models::sync_automation::{
    SYNC_AUTOMATION_COLD_START_DELAY_SECS, ScheduledSyncRule, SyncAutomationConfig,
    SyncAutomationStatus, SyncAutomationTarget, SyncAutomationToastEvent, SyncAutomationToastLevel,
    validate_sync_automation_config,
};

#[async_trait]
pub trait SyncAutomationRuleRepository: Send + Sync {
    async fn load_or_create_rule(&self) -> Result<LoadedScheduledSyncRule, DomainError>;
    async fn save_rule(&self, rule: &ScheduledSyncRule) -> Result<(), DomainError>;
}

#[async_trait]
pub trait SyncAutomationLanSettingsRepository: Send + Sync {
    async fn load_or_create_server_settings(&self) -> Result<LoadedLanServerSettings, DomainError>;
    async fn save_server_settings(&self, settings: &LanServerSettings) -> Result<(), DomainError>;
    async fn load_manual_default_sync_mode(&self) -> Result<SyncMode, DomainError>;
}

#[async_trait]
pub trait SyncAutomationEndpointCatalog: Send + Sync {
    async fn validate_target(
        &self,
        target: &SyncAutomationTarget,
        mode: SyncMode,
    ) -> Result<(), DomainError>;
}

#[async_trait]
pub trait SyncAutomationLanServerControl: Send + Sync {
    fn validate_allowed(&self) -> Result<(), DomainError>;
    async fn start(&self) -> Result<(), DomainError>;
    async fn ensure_running(&self) -> Result<(), DomainError>;
}

pub trait SyncAutomationEventPublisher: Send + Sync {
    fn publish_status(&self, status: SyncAutomationStatus);
    fn publish_toast(&self, event: SyncAutomationToastEvent);
}

pub struct LoadedScheduledSyncRule {
    pub rule: ScheduledSyncRule,
    pub legacy_lan_server_auto_start: Option<bool>,
    pub legacy_missing_sync_mode: bool,
    pub rewrite_canonical: bool,
}

pub struct LoadedLanServerSettings {
    pub settings: LanServerSettings,
    pub created: bool,
}

impl LoadedScheduledSyncRule {
    pub fn new(rule: ScheduledSyncRule) -> Self {
        Self {
            rule,
            legacy_lan_server_auto_start: None,
            legacy_missing_sync_mode: false,
            rewrite_canonical: false,
        }
    }
}

pub struct SyncAutomationService {
    events: Arc<dyn SyncAutomationEventPublisher>,
    rule_repository: Arc<dyn SyncAutomationRuleRepository>,
    lan_settings_repository: Arc<dyn SyncAutomationLanSettingsRepository>,
    endpoint_catalog: Arc<dyn SyncAutomationEndpointCatalog>,
    lan_server: Arc<dyn SyncAutomationLanServerControl>,
    coordinator: Arc<SyncJobCoordinator>,
    status: Mutex<SyncAutomationStatus>,
    notify: Notify,
}

impl SyncAutomationService {
    pub fn new(
        events: Arc<dyn SyncAutomationEventPublisher>,
        rule_repository: Arc<dyn SyncAutomationRuleRepository>,
        lan_settings_repository: Arc<dyn SyncAutomationLanSettingsRepository>,
        endpoint_catalog: Arc<dyn SyncAutomationEndpointCatalog>,
        lan_server: Arc<dyn SyncAutomationLanServerControl>,
        coordinator: Arc<SyncJobCoordinator>,
    ) -> Self {
        Self {
            events,
            rule_repository,
            lan_settings_repository,
            endpoint_catalog,
            lan_server,
            coordinator,
            status: Mutex::new(SyncAutomationStatus::default()),
            notify: Notify::new(),
        }
    }

    pub async fn run(self: Arc<Self>, cancel: CancellationToken) {
        self.start_lan_server_if_enabled().await;
        self.scheduler_loop(cancel).await;
    }

    pub async fn get_config(&self) -> Result<SyncAutomationConfig, DomainError> {
        let (rule, settings) = self.load_rule_and_settings().await?;
        Ok(SyncAutomationConfig::from_parts(settings.auto_start, rule))
    }

    pub async fn update_config(
        &self,
        config: SyncAutomationConfig,
    ) -> Result<SyncAutomationConfig, DomainError> {
        validate_sync_automation_config(&config)?;

        if config.lan_server_auto_start {
            self.lan_server.validate_allowed()?;
        }

        let rule = config.clone().into_rule();
        self.validate_rule_target(&rule).await?;

        let mut settings = self
            .lan_settings_repository
            .load_or_create_server_settings()
            .await?
            .settings;
        settings.auto_start = config.lan_server_auto_start;

        self.lan_settings_repository
            .save_server_settings(&settings)
            .await?;
        self.rule_repository.save_rule(&rule).await?;
        self.notify.notify_waiters();

        Ok(SyncAutomationConfig::from_parts(settings.auto_start, rule))
    }

    pub async fn get_status(&self) -> SyncAutomationStatus {
        self.status.lock().await.clone()
    }

    async fn load_rule_and_settings(
        &self,
    ) -> Result<(ScheduledSyncRule, LanServerSettings), DomainError> {
        let loaded_settings = self
            .lan_settings_repository
            .load_or_create_server_settings()
            .await?;
        let mut settings = loaded_settings.settings;
        let mut loaded = self.rule_repository.load_or_create_rule().await?;

        if let Some(auto_start) = loaded.legacy_lan_server_auto_start
            && loaded_settings.created
        {
            settings.auto_start = auto_start;
            self.lan_settings_repository
                .save_server_settings(&settings)
                .await?;
        }

        if loaded.legacy_missing_sync_mode {
            loaded.rule.sync_mode = self
                .lan_settings_repository
                .load_manual_default_sync_mode()
                .await?;
        }

        if loaded.rewrite_canonical {
            self.rule_repository.save_rule(&loaded.rule).await?;
        }

        Ok((loaded.rule, settings))
    }

    async fn scheduler_loop(self: Arc<Self>, cancel: CancellationToken) {
        let mut next_run_at_ms = now_ms() + SYNC_AUTOMATION_COLD_START_DELAY_SECS * 1000;

        loop {
            let rule = match self.load_rule_and_settings().await {
                Ok((rule, _settings)) => rule,
                Err(error) => {
                    self.record_error(error.to_string()).await;
                    if wait_or_cancel(&cancel, Duration::from_secs(60)).await {
                        break;
                    }
                    continue;
                }
            };

            if !rule.enabled || rule.target.is_none() {
                self.set_next_run(None).await;
                tokio::select! {
                    _ = self.notify.notified() => {
                        let interval = self.load_rule_and_settings()
                            .await
                            .map(|(rule, _)| rule.interval_minutes)
                            .unwrap_or(rule.interval_minutes);
                        next_run_at_ms = now_ms() + interval_ms(interval);
                    }
                    _ = cancel.cancelled() => break,
                }
                continue;
            }

            self.set_next_run(Some(next_run_at_ms)).await;
            let wait_ms = next_run_at_ms.saturating_sub(now_ms());

            tokio::select! {
                _ = sleep(Duration::from_millis(wait_ms)) => {}
                _ = self.notify.notified() => {
                    let interval = self.load_rule_and_settings()
                        .await
                        .map(|(rule, _)| rule.interval_minutes)
                        .unwrap_or(rule.interval_minutes);
                    next_run_at_ms = now_ms() + interval_ms(interval);
                    continue;
                }
                _ = cancel.cancelled() => break,
            }

            let rule = match self.load_rule_and_settings().await {
                Ok((rule, _settings)) => rule,
                Err(error) => {
                    self.record_error(error.to_string()).await;
                    next_run_at_ms = now_ms() + 60_000;
                    continue;
                }
            };

            let success = if rule.enabled && rule.target.is_some() {
                self.run_scheduled_upload(rule.clone()).await
            } else {
                None
            };
            let next_rule = self
                .load_rule_and_settings()
                .await
                .map(|(rule, _settings)| rule)
                .unwrap_or(rule);
            next_run_at_ms = now_ms() + interval_ms(next_rule.interval_minutes);
            let scheduled_next_run_at_ms =
                (next_rule.enabled && next_rule.target.is_some()).then_some(next_run_at_ms);
            self.set_next_run(scheduled_next_run_at_ms).await;

            if let Some(success) = success {
                match scheduled_next_run_at_ms {
                    Some(next_run_at_ms) => {
                        self.emit_toast_with_next_run(
                            SyncAutomationToastLevel::Info,
                            success.message(),
                            next_run_at_ms,
                        )
                        .await;
                    }
                    None => {
                        self.emit_toast(SyncAutomationToastLevel::Info, success.message())
                            .await;
                    }
                }
            }
        }
    }

    async fn start_lan_server_if_enabled(&self) {
        let settings = match self.load_rule_and_settings().await {
            Ok((_rule, settings)) => settings,
            Err(error) => {
                self.record_error(error.to_string()).await;
                return;
            }
        };

        if !settings.auto_start {
            return;
        }

        if let Err(error) = self.lan_server.validate_allowed() {
            self.record_error(error.to_string()).await;
            self.emit_toast(
                SyncAutomationToastLevel::Warning,
                "LAN Sync auto-start failed.",
            )
            .await;
            return;
        }

        if let Err(error) = self.lan_server.start().await {
            self.record_error(error.to_string()).await;
            self.emit_toast(
                SyncAutomationToastLevel::Warning,
                "LAN Sync auto-start failed.",
            )
            .await;
        }
    }

    async fn run_scheduled_upload(&self, rule: ScheduledSyncRule) -> Option<AutomationSuccess> {
        let started_at_ms = now_ms();
        self.update_status(|status| {
            status.running = true;
            status.next_run_at_ms = None;
            status.last_attempt_at_ms = Some(started_at_ms);
            status.last_error = None;
            status.last_error_at_ms = None;
        })
        .await;

        let result = self.run_upload_job(&rule).await;
        match result {
            Ok(success) => {
                let completed_at_ms = now_ms();
                self.update_status(|status| {
                    status.running = false;
                    match success {
                        AutomationSuccess::Completed => {
                            status.last_success_at_ms = Some(completed_at_ms);
                        }
                        AutomationSuccess::RemoteRequestAccepted => {
                            status.last_request_accepted_at_ms = Some(completed_at_ms);
                        }
                    }
                    status.last_error = None;
                    status.last_error_at_ms = None;
                })
                .await;
                Some(success)
            }
            Err(error) => {
                let message = error.to_string();
                let failed_at_ms = now_ms();
                self.update_status(|status| {
                    status.running = false;
                    status.last_error_at_ms = Some(failed_at_ms);
                    status.last_error = Some(message.clone());
                })
                .await;
                self.emit_toast_with_detail(
                    SyncAutomationToastLevel::Warning,
                    "Auto sync upload failed.",
                    Some(message),
                )
                .await;
                None
            }
        }
    }

    async fn run_upload_job(
        &self,
        rule: &ScheduledSyncRule,
    ) -> Result<AutomationSuccess, DomainError> {
        self.validate_rule_target(rule).await?;
        let target = rule
            .target
            .as_ref()
            .ok_or_else(|| DomainError::InvalidData("Auto sync target is required".to_string()))?;
        let options = SyncOperationOptions {
            selection: rule.selection.clone(),
            require_bundle_zstd: rule.require_bundle_zstd,
        };

        let request = match target {
            SyncAutomationTarget::Lan { device_id } => {
                self.lan_server.ensure_running().await?;
                let device_id = parse_device_id(device_id)?;
                SyncJobRequest {
                    endpoint: SyncEndpointRef::LanPeer { device_id },
                    intent: SyncIntent::ReplicateLocalToRemote,
                    origin: SyncOrigin::Scheduled,
                    policy: ResolvedSyncPolicy::RemotePullRequest { options },
                }
            }
            SyncAutomationTarget::Tt { server_device_id } => {
                let server_device_id = parse_device_id(server_device_id)?;
                SyncJobRequest {
                    endpoint: SyncEndpointRef::RemoteServer { server_device_id },
                    intent: SyncIntent::ReplicateLocalToRemote,
                    origin: SyncOrigin::Scheduled,
                    policy: ResolvedSyncPolicy::Transfer {
                        mode: rule.sync_mode,
                        options,
                    },
                }
            }
        };

        let report = self.coordinator.run(request).await;
        match report.result {
            SyncJobReportResult::Completed { .. } => Ok(AutomationSuccess::Completed),
            SyncJobReportResult::RemoteRequestAccepted => {
                Ok(AutomationSuccess::RemoteRequestAccepted)
            }
            SyncJobReportResult::Failed { message, .. } => Err(DomainError::InvalidData(message)),
        }
    }

    async fn validate_rule_target(&self, rule: &ScheduledSyncRule) -> Result<(), DomainError> {
        if !rule.enabled {
            return Ok(());
        }

        let Some(target) = &rule.target else {
            return Ok(());
        };
        self.endpoint_catalog
            .validate_target(target, rule.sync_mode)
            .await
    }

    async fn set_next_run(&self, next_run_at_ms: Option<u64>) {
        self.update_status(|status| {
            status.next_run_at_ms = next_run_at_ms;
        })
        .await;
    }

    async fn record_error(&self, message: String) {
        let failed_at_ms = now_ms();
        self.update_status(|status| {
            status.running = false;
            status.last_error_at_ms = Some(failed_at_ms);
            status.last_error = Some(message);
        })
        .await;
    }

    async fn update_status(&self, update: impl FnOnce(&mut SyncAutomationStatus)) {
        let snapshot = {
            let mut status = self.status.lock().await;
            update(&mut status);
            status.clone()
        };
        self.events.publish_status(snapshot);
    }

    async fn emit_toast(&self, level: SyncAutomationToastLevel, message: impl Into<String>) {
        self.emit_toast_with_detail(level, message, None).await;
    }

    async fn emit_toast_with_detail(
        &self,
        level: SyncAutomationToastLevel,
        message: impl Into<String>,
        detail: Option<String>,
    ) {
        self.emit_toast_event(level, message, detail, None).await;
    }

    async fn emit_toast_with_next_run(
        &self,
        level: SyncAutomationToastLevel,
        message: impl Into<String>,
        next_run_at_ms: u64,
    ) {
        self.emit_toast_event(level, message, None, Some(next_run_at_ms))
            .await;
    }

    async fn emit_toast_event(
        &self,
        level: SyncAutomationToastLevel,
        message: impl Into<String>,
        detail: Option<String>,
        next_run_at_ms: Option<u64>,
    ) {
        let payload = SyncAutomationToastEvent {
            level,
            message: message.into(),
            detail,
            next_run_at_ms,
        };
        self.events.publish_toast(payload);
    }
}

#[derive(Debug, Clone, Copy)]
enum AutomationSuccess {
    Completed,
    RemoteRequestAccepted,
}

impl AutomationSuccess {
    fn message(self) -> &'static str {
        match self {
            Self::Completed => "Auto sync upload has completed as scheduled.",
            Self::RemoteRequestAccepted => {
                "Auto sync upload request has been accepted as scheduled."
            }
        }
    }
}

async fn wait_or_cancel(cancel: &CancellationToken, duration: Duration) -> bool {
    tokio::select! {
        _ = sleep(duration) => false,
        _ = cancel.cancelled() => true,
    }
}

fn parse_device_id(value: &str) -> Result<DeviceId, DomainError> {
    DeviceId::new(value.to_string()).map_err(|error| DomainError::InvalidData(error.to_string()))
}

fn interval_ms(interval_minutes: u16) -> u64 {
    u64::from(interval_minutes) * 60 * 1000
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex as StdMutex;

    use crate::application::services::data_change_reconciler::DataChangeReconciler;
    use crate::application::services::sync_job_coordinator::{
        SyncJobEventPublisher, SyncJobExecutor,
    };
    use crate::domain::models::sync::{
        LocalAppliedChangeSummary, SyncExecutionFailure, SyncExecutionKind, SyncExecutionReport,
        SyncJob, SyncJobEvent, SyncJobSummary,
    };

    #[derive(Default)]
    struct RecordingEvents {
        statuses: StdMutex<Vec<SyncAutomationStatus>>,
        toasts: StdMutex<Vec<SyncAutomationToastEvent>>,
    }

    impl SyncAutomationEventPublisher for RecordingEvents {
        fn publish_status(&self, status: SyncAutomationStatus) {
            self.statuses.lock().unwrap().push(status);
        }

        fn publish_toast(&self, event: SyncAutomationToastEvent) {
            self.toasts.lock().unwrap().push(event);
        }
    }

    struct NoopJobEvents;

    impl SyncJobEventPublisher for NoopJobEvents {
        fn publish_sync_job(&self, _event: SyncJobEvent) {}
    }

    struct NoopRuleRepository;

    #[async_trait]
    impl SyncAutomationRuleRepository for NoopRuleRepository {
        async fn load_or_create_rule(&self) -> Result<LoadedScheduledSyncRule, DomainError> {
            Ok(LoadedScheduledSyncRule::new(ScheduledSyncRule::default()))
        }

        async fn save_rule(&self, _rule: &ScheduledSyncRule) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct NoopLanSettingsRepository;

    #[async_trait]
    impl SyncAutomationLanSettingsRepository for NoopLanSettingsRepository {
        async fn load_or_create_server_settings(
            &self,
        ) -> Result<LoadedLanServerSettings, DomainError> {
            Ok(LoadedLanServerSettings {
                settings: LanServerSettings {
                    port: 55555,
                    auto_start: false,
                },
                created: false,
            })
        }

        async fn save_server_settings(
            &self,
            _settings: &LanServerSettings,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        async fn load_manual_default_sync_mode(&self) -> Result<SyncMode, DomainError> {
            Ok(SyncMode::Incremental)
        }
    }

    struct MigratingRuleRepository {
        saved_rules: StdMutex<Vec<ScheduledSyncRule>>,
    }

    impl MigratingRuleRepository {
        fn new() -> Self {
            Self {
                saved_rules: StdMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl SyncAutomationRuleRepository for MigratingRuleRepository {
        async fn load_or_create_rule(&self) -> Result<LoadedScheduledSyncRule, DomainError> {
            Ok(LoadedScheduledSyncRule {
                rule: ScheduledSyncRule {
                    interval_minutes: 15,
                    ..ScheduledSyncRule::default()
                },
                legacy_lan_server_auto_start: Some(true),
                legacy_missing_sync_mode: true,
                rewrite_canonical: true,
            })
        }

        async fn save_rule(&self, rule: &ScheduledSyncRule) -> Result<(), DomainError> {
            self.saved_rules.lock().unwrap().push(rule.clone());
            Ok(())
        }
    }

    struct MigratingLanSettingsRepository {
        saved_settings: StdMutex<Vec<LanServerSettings>>,
        created: bool,
    }

    impl MigratingLanSettingsRepository {
        fn new(created: bool) -> Self {
            Self {
                saved_settings: StdMutex::new(Vec::new()),
                created,
            }
        }
    }

    #[async_trait]
    impl SyncAutomationLanSettingsRepository for MigratingLanSettingsRepository {
        async fn load_or_create_server_settings(
            &self,
        ) -> Result<LoadedLanServerSettings, DomainError> {
            Ok(LoadedLanServerSettings {
                settings: LanServerSettings {
                    port: 55555,
                    auto_start: false,
                },
                created: self.created,
            })
        }

        async fn save_server_settings(
            &self,
            settings: &LanServerSettings,
        ) -> Result<(), DomainError> {
            self.saved_settings.lock().unwrap().push(settings.clone());
            Ok(())
        }

        async fn load_manual_default_sync_mode(&self) -> Result<SyncMode, DomainError> {
            Ok(SyncMode::Mirror)
        }
    }

    struct AllowEndpointCatalog;

    #[async_trait]
    impl SyncAutomationEndpointCatalog for AllowEndpointCatalog {
        async fn validate_target(
            &self,
            _target: &SyncAutomationTarget,
            _mode: SyncMode,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct ReadyLanServer;

    #[async_trait]
    impl SyncAutomationLanServerControl for ReadyLanServer {
        fn validate_allowed(&self) -> Result<(), DomainError> {
            Ok(())
        }

        async fn start(&self) -> Result<(), DomainError> {
            Ok(())
        }

        async fn ensure_running(&self) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct OutcomeExecutor;

    #[async_trait]
    impl SyncJobExecutor for OutcomeExecutor {
        async fn execute(&self, job: SyncJob) -> Result<SyncExecutionReport, SyncExecutionFailure> {
            if job.execution == SyncExecutionKind::RequestRemotePull {
                return Ok(SyncExecutionReport::remote_request_accepted());
            }

            Ok(SyncExecutionReport::completed(
                SyncJobSummary::new(1, 64, 0),
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

    fn service_with_repositories(
        rule_repository: Arc<dyn SyncAutomationRuleRepository>,
        lan_settings_repository: Arc<dyn SyncAutomationLanSettingsRepository>,
    ) -> SyncAutomationService {
        SyncAutomationService::new(
            Arc::new(RecordingEvents::default()),
            rule_repository,
            lan_settings_repository,
            Arc::new(AllowEndpointCatalog),
            Arc::new(ReadyLanServer),
            Arc::new(SyncJobCoordinator::new(
                Arc::new(OutcomeExecutor),
                Arc::new(NoopReconciler),
                Arc::new(NoopJobEvents),
            )),
        )
    }

    fn service() -> SyncAutomationService {
        service_with_repositories(
            Arc::new(NoopRuleRepository),
            Arc::new(NoopLanSettingsRepository),
        )
    }

    #[tokio::test]
    async fn legacy_rule_migration_moves_auto_start_and_manual_mode() {
        let rule_repository = Arc::new(MigratingRuleRepository::new());
        let lan_settings_repository = Arc::new(MigratingLanSettingsRepository::new(true));
        let service =
            service_with_repositories(rule_repository.clone(), lan_settings_repository.clone());

        let config = service.get_config().await.expect("migrate config");

        assert!(config.lan_server_auto_start);
        assert_eq!(config.sync_mode, SyncMode::Mirror);
        assert_eq!(
            lan_settings_repository
                .saved_settings
                .lock()
                .unwrap()
                .last()
                .map(|settings| settings.auto_start),
            Some(true),
        );
        assert_eq!(
            rule_repository
                .saved_rules
                .lock()
                .unwrap()
                .last()
                .map(|rule| rule.sync_mode),
            Some(SyncMode::Mirror),
        );
    }

    #[tokio::test]
    async fn legacy_rule_migration_keeps_existing_lan_auto_start() {
        let rule_repository = Arc::new(MigratingRuleRepository::new());
        let lan_settings_repository = Arc::new(MigratingLanSettingsRepository::new(false));
        let service =
            service_with_repositories(rule_repository.clone(), lan_settings_repository.clone());

        let config = service.get_config().await.expect("migrate config");

        assert!(!config.lan_server_auto_start);
        assert!(
            lan_settings_repository
                .saved_settings
                .lock()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            rule_repository
                .saved_rules
                .lock()
                .unwrap()
                .last()
                .map(|rule| rule.sync_mode),
            Some(SyncMode::Mirror),
        );
    }

    #[tokio::test]
    async fn lan_request_acceptance_does_not_count_as_success() {
        let service = service();
        let rule = ScheduledSyncRule {
            enabled: true,
            target: Some(SyncAutomationTarget::Lan {
                device_id: "11111111-1111-4111-8111-111111111111".to_string(),
            }),
            ..ScheduledSyncRule::default()
        };

        let result = service.run_scheduled_upload(rule).await;
        let status = service.get_status().await;

        assert!(matches!(
            result,
            Some(AutomationSuccess::RemoteRequestAccepted)
        ));
        assert!(status.last_request_accepted_at_ms.is_some());
        assert!(status.last_success_at_ms.is_none());
        assert!(status.last_error.is_none());
    }

    #[tokio::test]
    async fn completed_transfer_updates_last_success() {
        let service = service();
        let rule = ScheduledSyncRule {
            enabled: true,
            target: Some(SyncAutomationTarget::Tt {
                server_device_id: "22222222-2222-4222-8222-222222222222".to_string(),
            }),
            ..ScheduledSyncRule::default()
        };

        let result = service.run_scheduled_upload(rule).await;
        let status = service.get_status().await;

        assert!(matches!(result, Some(AutomationSuccess::Completed)));
        assert!(status.last_success_at_ms.is_some());
        assert!(status.last_request_accepted_at_ms.is_none());
        assert!(status.last_error.is_none());
    }
}
