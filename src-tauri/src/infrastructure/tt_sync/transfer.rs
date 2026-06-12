pub(crate) use crate::infrastructure::sync_transfer::{
    now_ms, resolve_to_local, should_emit_progress,
};

pub(crate) fn tt_sync_transfer_concurrency() -> usize {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        8
    } else {
        16
    }
}
