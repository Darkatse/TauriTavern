use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, Notify};
use tokio::time::{Duration, sleep};
use ttsync_contract::sync::SyncMode;

use crate::application::services::lan_sync_service::LanSyncService;
use crate::application::services::tt_sync_service::TtSyncService;
use crate::domain::errors::DomainError;
use crate::domain::models::sync::SyncOperationOptions;
use crate::domain::models::sync_automation::{
    SYNC_AUTOMATION_COLD_START_DELAY_SECS, SyncAutomationConfig, SyncAutomationStatus,
    SyncAutomationTarget, SyncAutomationToastEvent, SyncAutomationToastLevel,
};
use crate::infrastructure::sync_automation_store::SyncAutomationStore;

pub trait SyncAutomationEventPublisher: Send + Sync {
    fn publish_status(&self, status: SyncAutomationStatus);
    fn publish_toast(&self, event: SyncAutomationToastEvent);
}

pub struct SyncAutomationService {
    events: Arc<dyn SyncAutomationEventPublisher>,
    store: SyncAutomationStore,
    lan_sync_service: Arc<LanSyncService>,
    tt_sync_service: Arc<TtSyncService>,
    lan_sync_allowed: bool,
    status: Mutex<SyncAutomationStatus>,
    notify: Notify,
    started: AtomicBool,
}

impl SyncAutomationService {
    pub fn new(
        events: Arc<dyn SyncAutomationEventPublisher>,
        default_user_dir: PathBuf,
        lan_sync_service: Arc<LanSyncService>,
        tt_sync_service: Arc<TtSyncService>,
        lan_sync_allowed: bool,
    ) -> Self {
        Self {
            events,
            store: SyncAutomationStore::new(default_user_dir),
            lan_sync_service,
            tt_sync_service,
            lan_sync_allowed,
            status: Mutex::new(SyncAutomationStatus::default()),
            notify: Notify::new(),
            started: AtomicBool::new(false),
        }
    }

    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }

        let service = self.clone();
        tokio::spawn(async move {
            service.start_lan_server_if_enabled().await;
            service.scheduler_loop().await;
        });
    }

    pub async fn get_config(&self) -> Result<SyncAutomationConfig, DomainError> {
        self.store.load_or_create_config().await
    }

    pub async fn update_config(
        &self,
        config: SyncAutomationConfig,
    ) -> Result<SyncAutomationConfig, DomainError> {
        self.validate_config_targets(&config).await?;
        self.store.save_config(&config).await?;
        self.notify.notify_waiters();
        Ok(config)
    }

    pub async fn get_status(&self) -> SyncAutomationStatus {
        self.status.lock().await.clone()
    }

    async fn scheduler_loop(self: Arc<Self>) {
        let mut next_run_at_ms = now_ms() + SYNC_AUTOMATION_COLD_START_DELAY_SECS * 1000;

        loop {
            let config = match self.store.load_or_create_config().await {
                Ok(config) => config,
                Err(error) => {
                    self.record_error(error.to_string()).await;
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };

            if !config.auto_sync_enabled || config.target.is_none() {
                self.set_next_run(None).await;
                self.notify.notified().await;
                match self.store.load_or_create_config().await {
                    Ok(next_config) => {
                        next_run_at_ms = now_ms() + interval_ms(next_config.interval_minutes);
                    }
                    Err(error) => {
                        self.record_error(error.to_string()).await;
                        next_run_at_ms = now_ms() + 60_000;
                    }
                }
                continue;
            }

            self.set_next_run(Some(next_run_at_ms)).await;
            let wait_ms = next_run_at_ms.saturating_sub(now_ms());
            let wait = sleep(Duration::from_millis(wait_ms));
            tokio::pin!(wait);

            tokio::select! {
                _ = &mut wait => {}
                _ = self.notify.notified() => {
                    match self.store.load_or_create_config().await {
                        Ok(next_config) => {
                            next_run_at_ms = now_ms() + interval_ms(next_config.interval_minutes);
                        }
                        Err(error) => {
                            self.record_error(error.to_string()).await;
                            next_run_at_ms = now_ms() + 60_000;
                        }
                    }
                    continue;
                }
            }

            let config = match self.store.load_or_create_config().await {
                Ok(config) => config,
                Err(error) => {
                    self.record_error(error.to_string()).await;
                    next_run_at_ms = now_ms() + 60_000;
                    continue;
                }
            };

            let success_message = if config.auto_sync_enabled && config.target.is_some() {
                self.run_auto_upload(config.clone()).await
            } else {
                None
            };
            let next_config = self.store.load_or_create_config().await.unwrap_or(config);
            next_run_at_ms = now_ms() + interval_ms(next_config.interval_minutes);
            let scheduled_next_run_at_ms = (next_config.auto_sync_enabled
                && next_config.target.is_some())
            .then_some(next_run_at_ms);
            self.set_next_run(scheduled_next_run_at_ms).await;

            if let Some(message) = success_message {
                match scheduled_next_run_at_ms {
                    Some(next_run_at_ms) => {
                        self.emit_toast_with_next_run(
                            SyncAutomationToastLevel::Info,
                            message,
                            next_run_at_ms,
                        )
                        .await;
                    }
                    None => {
                        self.emit_toast(SyncAutomationToastLevel::Info, message)
                            .await;
                    }
                }
            }
        }
    }

    async fn start_lan_server_if_enabled(&self) {
        let config = match self.store.load_or_create_config().await {
            Ok(config) => config,
            Err(error) => {
                self.record_error(error.to_string()).await;
                return;
            }
        };

        if !config.lan_server_auto_start {
            return;
        }

        if !self.lan_sync_allowed {
            self.record_error("LAN Sync is not allowed by the current platform policy".to_string())
                .await;
            self.emit_toast(
                SyncAutomationToastLevel::Warning,
                "LAN Sync auto-start failed.",
            )
            .await;
            return;
        }

        if let Err(error) = self.lan_sync_service.start_server().await {
            self.record_error(error.to_string()).await;
            self.emit_toast(
                SyncAutomationToastLevel::Warning,
                "LAN Sync auto-start failed.",
            )
            .await;
        }
    }

    async fn run_auto_upload(&self, config: SyncAutomationConfig) -> Option<String> {
        let started_at_ms = now_ms();
        self.update_status(|status| {
            status.running = true;
            status.next_run_at_ms = None;
            status.last_attempt_at_ms = Some(started_at_ms);
            status.last_error = None;
        })
        .await;

        let result = self.run_upload(&config).await;
        match result {
            Ok(message) => {
                let completed_at_ms = now_ms();
                self.update_status(|status| {
                    status.running = false;
                    status.last_success_at_ms = Some(completed_at_ms);
                    status.last_error = None;
                })
                .await;
                Some(message)
            }
            Err(error) => {
                let message = error.to_string();
                self.update_status(|status| {
                    status.running = false;
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

    async fn run_upload(&self, config: &SyncAutomationConfig) -> Result<String, DomainError> {
        let target = config
            .target
            .as_ref()
            .ok_or_else(|| DomainError::InvalidData("Auto sync target is required".to_string()))?;
        let options = Some(SyncOperationOptions {
            selection: config.selection.clone(),
            require_bundle_zstd: true,
        });

        match target {
            SyncAutomationTarget::Lan { device_id } => {
                if !self.lan_sync_allowed {
                    return Err(DomainError::InvalidData(
                        "LAN Sync is not allowed by the current platform policy".to_string(),
                    ));
                }

                let status = self.lan_sync_service.get_status().await?;
                if !status.running {
                    return Err(DomainError::InvalidData(
                        "LAN Sync server is not running. Start the sync port before using LAN auto upload.".to_string(),
                    ));
                }

                self.lan_sync_service
                    .push_to_device_for_automation(device_id, options)
                    .await?;
                Ok("Auto sync upload has started as scheduled.".to_string())
            }
            SyncAutomationTarget::Tt { server_device_id } => {
                let mode = self.lan_sync_service.effective_sync_mode().await?;
                self.tt_sync_service
                    .push_for_automation(server_device_id, mode, options)
                    .await?;
                Ok("Auto sync upload has completed as scheduled.".to_string())
            }
        }
    }

    async fn validate_config_targets(
        &self,
        config: &SyncAutomationConfig,
    ) -> Result<(), DomainError> {
        crate::infrastructure::sync_automation_store::validate_config(config)?;

        if config.lan_server_auto_start && !self.lan_sync_allowed {
            return Err(DomainError::InvalidData(
                "LAN Sync is not allowed by the current platform policy".to_string(),
            ));
        }

        if !config.auto_sync_enabled {
            return Ok(());
        }

        match config
            .target
            .as_ref()
            .ok_or_else(|| DomainError::InvalidData("Auto sync target is required".to_string()))?
        {
            SyncAutomationTarget::Lan { device_id } => {
                if !self.lan_sync_allowed {
                    return Err(DomainError::InvalidData(
                        "LAN Sync is not allowed by the current platform policy".to_string(),
                    ));
                }

                let devices = self.lan_sync_service.list_paired_devices().await?;
                let device = devices
                    .iter()
                    .find(|device| device.device_id == *device_id)
                    .ok_or_else(|| {
                        DomainError::NotFound(format!("LAN Sync device not found: {device_id}"))
                    })?;
                if device.last_known_address.is_none() {
                    return Err(DomainError::InvalidData(
                        "LAN auto upload requires a paired LAN Sync device with an address"
                            .to_string(),
                    ));
                }
            }
            SyncAutomationTarget::Tt { server_device_id } => {
                let servers = self.tt_sync_service.list_servers().await?;
                let server = servers
                    .iter()
                    .find(|server| server.server_device_id.as_str() == server_device_id.as_str())
                    .ok_or_else(|| {
                        DomainError::NotFound(format!(
                            "TT-Sync server not found: {server_device_id}"
                        ))
                    })?;
                if !server.permissions.write {
                    return Err(DomainError::AuthenticationError(
                        "TT-Sync server does not grant write permission".to_string(),
                    ));
                }
                let mode = self.lan_sync_service.effective_sync_mode().await?;
                if mode == SyncMode::Mirror && !server.permissions.mirror_delete {
                    return Err(DomainError::AuthenticationError(
                        "TT-Sync server does not grant mirror_delete permission".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    async fn set_next_run(&self, next_run_at_ms: Option<u64>) {
        self.update_status(|status| {
            status.next_run_at_ms = next_run_at_ms;
        })
        .await;
    }

    async fn record_error(&self, message: String) {
        self.update_status(|status| {
            status.running = false;
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

fn interval_ms(interval_minutes: u16) -> u64 {
    u64::from(interval_minutes) * 60 * 1000
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
