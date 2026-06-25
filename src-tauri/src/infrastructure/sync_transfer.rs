use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ttsync_contract::path::SyncPath;

use crate::domain::models::sync::LocalAppliedChangeSummary;

pub(crate) fn default_transfer_concurrency() -> usize {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        2
    } else {
        4
    }
}

pub(crate) fn should_emit_progress(files_done: usize, files_total: usize) -> bool {
    files_done == files_total || files_done == 1 || files_done % 10 == 0
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

pub(crate) fn resolve_to_local(sync_root: &Path, sync_path: &SyncPath) -> PathBuf {
    let mut full_path = PathBuf::from(sync_root);
    for part in sync_path.as_str().split('/') {
        full_path.push(part);
    }
    full_path
}

#[derive(Default)]
pub(crate) struct LocalChangeTracker {
    files_written: AtomicUsize,
    bytes_written: AtomicU64,
    files_deleted: AtomicUsize,
}

impl LocalChangeTracker {
    pub(crate) fn record_write(&self, size_bytes: u64) {
        self.files_written.fetch_add(1, Ordering::Relaxed);
        self.bytes_written.fetch_add(size_bytes, Ordering::Relaxed);
    }

    pub(crate) fn record_delete(&self) {
        self.files_deleted.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn summary(&self) -> LocalAppliedChangeSummary {
        LocalAppliedChangeSummary {
            files_written: self.files_written.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            files_deleted: self.files_deleted.load(Ordering::Relaxed),
        }
    }
}
