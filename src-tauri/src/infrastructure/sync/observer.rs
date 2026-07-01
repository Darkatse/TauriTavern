use std::sync::Arc;

use ttsync_client::{SyncDirection as ClientSyncDirection, SyncObserver, SyncProgress};

use tt_contracts::sync::{SyncJobContext, SyncJobEvent, SyncJobProgress, SyncJobProgressDirection};
use tt_ports::sync::SyncJobEventPublisher;

pub struct SyncJobProgressObserver {
    events: Arc<dyn SyncJobEventPublisher>,
    job: SyncJobContext,
}

impl SyncJobProgressObserver {
    pub fn new(events: Arc<dyn SyncJobEventPublisher>, job: SyncJobContext) -> Self {
        Self { events, job }
    }
}

impl SyncObserver for SyncJobProgressObserver {
    fn on_progress(&self, progress: SyncProgress) {
        self.events.publish_sync_job(SyncJobEvent::progress(
            self.job.clone(),
            SyncJobProgress {
                direction: progress_direction(progress.direction),
                phase: progress.phase,
                files_done: progress.files_done,
                files_total: progress.files_total,
                bytes_done: progress.bytes_done,
                bytes_total: progress.bytes_total,
                current_path: progress.current_path,
            },
        ));
    }
}

fn progress_direction(direction: ClientSyncDirection) -> SyncJobProgressDirection {
    match direction {
        ClientSyncDirection::Pull => SyncJobProgressDirection::Pull,
        ClientSyncDirection::Push => SyncJobProgressDirection::Push,
    }
}
