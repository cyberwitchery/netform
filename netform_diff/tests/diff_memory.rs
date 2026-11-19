//! the diff's working memory grows with the size of its input, not with the
//! square of the number of changed lines. this file holds a single test, so
//! the counting allocator below sees no other test's allocations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use netform_diff::{NormalizeOptions, diff_documents};
use netform_ir::parse_generic;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded to the system allocator unchanged; the
// counters only observe sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// the peak heap use while diffing `n` distinct lines against `n` others.
fn peak_for_distinct_lines(n: usize) -> usize {
    let left: String = (0..n).map(|i| format!("left line {i}\n")).collect();
    let right: String = (0..n).map(|i| format!("right line {i}\n")).collect();
    let a = parse_generic(&left);
    let b = parse_generic(&right);

    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let peak = PEAK.load(Ordering::Relaxed) - base;

    assert!(diff.has_changes);
    drop(diff);
    peak
}

#[test]
fn diff_memory_is_linear_in_the_number_of_changed_lines() {
    let small = peak_for_distinct_lines(2_000);
    let large = peak_for_distinct_lines(4_000);
    // doubling the input roughly doubles linear memory and quadruples
    // quadratic memory.
    assert!(
        large < 3 * small,
        "peak heap went from {small} bytes for 2000 lines per side to {large} bytes for 4000"
    );
}
