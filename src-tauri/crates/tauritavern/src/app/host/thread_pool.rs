//! rayon's global pool runs the CPU-parallel work of every crate: long token estimates, local
//! embeddings, vector search.

use std::num::NonZeroUsize;

/// Gives rayon's global pool one thread for each CPU of the device.
///
/// By default rayon counts the CPUs online when its pool starts. Android takes idle cores offline,
/// so a pool started then would leave some of them unused for the life of the process.
pub(super) fn configure() {
    let Some(cpus) = device_cpus() else {
        return;
    };
    rayon::ThreadPoolBuilder::new()
        .num_threads(cpus.get())
        .build_global()
        .expect("the host configures rayon's global pool before anything uses it");
}

/// Every CPU the device has, online or not.
#[cfg(unix)]
fn device_cpus() -> Option<NonZeroUsize> {
    // SAFETY: sysconf only reads a system value.
    let cpus = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_CONF) };
    usize::try_from(cpus).ok().and_then(NonZeroUsize::new)
}

/// Every CPU the device has. Windows does not take cores offline.
#[cfg(windows)]
fn device_cpus() -> Option<NonZeroUsize> {
    std::thread::available_parallelism().ok()
}
