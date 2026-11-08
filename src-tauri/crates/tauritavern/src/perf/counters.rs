//! Process work counters that depend on the code and data, not on machine speed. Each platform reports what it
//! counts exactly: Linux and Windows the bytes and calls of reads and writes, macOS retired instructions. Linux
//! instructions come from callgrind, which splits its dumps at `tt_perf_step_boundary`.

use std::collections::BTreeMap;

pub type Counters = BTreeMap<&'static str, u64>;

/// The counters at one moment, and what reading them added to them.
pub struct Snapshot {
    counters: Counters,
    cost: Counters,
}

/// Callgrind runs with `--dump-before=tt_perf_step_boundary`, so each call closes one dump.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn tt_perf_step_boundary() {
    std::hint::black_box(());
}

#[cfg(target_os = "linux")]
pub fn snapshot() -> Result<Snapshot, String> {
    use std::io::Read;
    // The kernel renders the values before it counts the read that returns them; one read keeps that cost exact.
    let mut text = [0u8; 512];
    let length = std::fs::File::open("/proc/self/io")
        .and_then(|mut file| file.read(&mut text))
        .map_err(|error| format!("/proc/self/io: {error}"))?;
    if length == text.len() {
        return Err("/proc/self/io does not fit one read".to_string());
    }
    let io =
        std::str::from_utf8(&text[..length]).map_err(|error| format!("/proc/self/io: {error}"))?;
    let field = |name: &str| {
        io.lines()
            .find_map(|line| {
                line.strip_prefix(name)?
                    .strip_prefix(": ")?
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .ok_or_else(|| format!("/proc/self/io has no {name}"))
    };
    Ok(Snapshot {
        counters: Counters::from([
            ("readBytes", field("rchar")?),
            ("readOps", field("syscr")?),
            ("writeBytes", field("wchar")?),
            ("writeOps", field("syscw")?),
        ]),
        cost: Counters::from([("readBytes", length as u64), ("readOps", 1)]),
    })
}

#[cfg(target_os = "macos")]
pub fn snapshot() -> Result<Snapshot, String> {
    #[repr(C)]
    struct RusageInfoV4 {
        uuid: [u8; 16],
        fields: [u64; 35],
    }
    const RUSAGE_INFO_V4: i32 = 4;
    const INSTRUCTIONS: usize = 29;
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
    }
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    if unsafe { proc_pid_rusage(std::process::id() as i32, RUSAGE_INFO_V4, &mut info) } != 0 {
        return Err(format!(
            "proc_pid_rusage: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(Snapshot {
        counters: Counters::from([("instructions", info.fields[INSTRUCTIONS])]),
        cost: Counters::new(),
    })
}

#[cfg(target_os = "windows")]
pub fn snapshot() -> Result<Snapshot, String> {
    #[repr(C)]
    #[derive(Default)]
    struct IoCounters {
        read_operations: u64,
        write_operations: u64,
        other_operations: u64,
        read_bytes: u64,
        write_bytes: u64,
        other_bytes: u64,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessIoCounters(process: isize, counters: *mut IoCounters) -> i32;
    }
    let mut io = IoCounters::default();
    if unsafe { GetProcessIoCounters(GetCurrentProcess(), &mut io) } == 0 {
        return Err(format!(
            "GetProcessIoCounters: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(Snapshot {
        counters: Counters::from([
            ("readBytes", io.read_bytes),
            ("readOps", io.read_operations),
            ("writeBytes", io.write_bytes),
            ("writeOps", io.write_operations),
        ]),
        cost: Counters::new(),
    })
}

/// The work since `start`, excluding the reading of `start` itself.
pub fn since(start: &Snapshot) -> Result<Counters, String> {
    let end = snapshot()?;
    Ok(end
        .counters
        .iter()
        .map(|(name, value)| {
            let cost = start.cost.get(name).copied().unwrap_or(0);
            (*name, value - start.counters[name] - cost)
        })
        .collect())
}
