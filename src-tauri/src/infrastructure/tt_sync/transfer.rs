use std::path::{Path, PathBuf};

use ttsync_contract::path::SyncPath;

pub(crate) use crate::infrastructure::sync_transfer::{now_ms, should_emit_progress};

pub(crate) fn tt_sync_transfer_concurrency() -> usize {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        8
    } else {
        16
    }
}

pub(crate) fn resolve_to_local(sync_root: &Path, sync_path: &SyncPath) -> PathBuf {
    let mut full_path = PathBuf::from(sync_root);
    for part in sync_path.as_str().split('/') {
        full_path.push(part);
    }
    full_path
}
