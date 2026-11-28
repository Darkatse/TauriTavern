//! The allocator tt-perf runs on: the system allocator behind named functions that count what the code asks of it.
//! What the allocator does inside depends on the heap the whole process has left so far, not on the step that asks,
//! so the steps count the requests, and callgrind's instructions leave these functions out (tests/perf/backend.mjs).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

/// The allocations and reallocations so far, and the bytes they asked for.
pub(super) fn requests() -> [(&'static str, u64); 2] {
    [
        ("allocations", ALLOCATIONS.load(Ordering::Relaxed)),
        ("allocatedBytes", ALLOCATED_BYTES.load(Ordering::Relaxed)),
    ]
}

pub struct Allocator;

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { tt_perf_alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { tt_perf_alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { tt_perf_realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { tt_perf_dealloc(ptr, layout) }
    }
}

fn count(bytes: usize) {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    ALLOCATED_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
unsafe fn tt_perf_alloc(layout: Layout) -> *mut u8 {
    count(layout.size());
    unsafe { System.alloc(layout) }
}

#[unsafe(no_mangle)]
#[inline(never)]
unsafe fn tt_perf_alloc_zeroed(layout: Layout) -> *mut u8 {
    count(layout.size());
    unsafe { System.alloc_zeroed(layout) }
}

#[unsafe(no_mangle)]
#[inline(never)]
unsafe fn tt_perf_realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    count(new_size);
    unsafe { System.realloc(ptr, layout, new_size) }
}

#[unsafe(no_mangle)]
#[inline(never)]
unsafe fn tt_perf_dealloc(ptr: *mut u8, layout: Layout) {
    unsafe { System.dealloc(ptr, layout) }
}
