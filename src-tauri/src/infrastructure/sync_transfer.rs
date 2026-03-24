use std::time::{SystemTime, UNIX_EPOCH};

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

