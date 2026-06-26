use std::sync::Arc;

use ttsync_client::{SyncDirection as ClientSyncDirection, SyncObserver, SyncProgress};
use ttsync_contract::sync::SyncPhase;

use crate::application::services::lan_sync_service::LanSyncEventPublisher;
use crate::domain::models::lan_sync::{LanSyncSyncPhase, LanSyncSyncProgressEvent};
use crate::domain::models::sync::SyncOrigin;
use crate::domain::models::tt_sync::{TtSyncDirection, TtSyncProgressEvent};
use crate::infrastructure::tt_sync::runtime::TtSyncRuntime;

pub struct LanSyncProgressObserver {
    events: Arc<dyn LanSyncEventPublisher>,
}

impl LanSyncProgressObserver {
    pub fn new(events: Arc<dyn LanSyncEventPublisher>) -> Self {
        Self { events }
    }
}

impl SyncObserver for LanSyncProgressObserver {
    fn on_progress(&self, progress: SyncProgress) {
        let Some(phase) = lan_phase(progress.phase) else {
            tracing::warn!(
                "LAN Sync received unsupported progress phase: {:?}",
                progress.phase
            );
            return;
        };

        self.events.publish_progress(LanSyncSyncProgressEvent {
            phase,
            files_done: progress.files_done,
            files_total: progress.files_total,
            bytes_done: progress.bytes_done,
            bytes_total: progress.bytes_total,
            current_path: progress.current_path,
        });
    }
}

fn lan_phase(phase: SyncPhase) -> Option<LanSyncSyncPhase> {
    match phase {
        SyncPhase::Scanning => Some(LanSyncSyncPhase::Scanning),
        SyncPhase::Diffing => Some(LanSyncSyncPhase::Diffing),
        SyncPhase::Downloading => Some(LanSyncSyncPhase::Downloading),
        SyncPhase::Deleting => Some(LanSyncSyncPhase::Deleting),
        SyncPhase::Uploading => None,
    }
}

pub struct TtSyncProgressObserver {
    runtime: Arc<TtSyncRuntime>,
    origin: SyncOrigin,
}

impl TtSyncProgressObserver {
    pub fn new(runtime: Arc<TtSyncRuntime>, origin: SyncOrigin) -> Self {
        Self { runtime, origin }
    }
}

impl SyncObserver for TtSyncProgressObserver {
    fn on_progress(&self, progress: SyncProgress) {
        self.runtime.emit_progress(
            TtSyncProgressEvent {
                direction: tt_direction(progress.direction),
                phase: progress.phase,
                files_done: progress.files_done,
                files_total: progress.files_total,
                bytes_done: progress.bytes_done,
                bytes_total: progress.bytes_total,
                current_path: progress.current_path,
            },
            &self.origin,
        );
    }
}

fn tt_direction(direction: ClientSyncDirection) -> TtSyncDirection {
    match direction {
        ClientSyncDirection::Pull => TtSyncDirection::Pull,
        ClientSyncDirection::Push => TtSyncDirection::Push,
    }
}
