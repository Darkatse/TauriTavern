use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tt_contracts::host::HostKind;
use ttsync_contract::path::SyncPath;

pub(crate) fn default_transfer_concurrency(kind: HostKind) -> usize {
    match kind {
        HostKind::Mobile => 2,
        HostKind::Desktop => 4,
    }
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
