//! The memory of the evaluation caches kept per thread (the IR executor's
//! memo tables of interpreter fallbacks and mixed-mode regions) within the
//! memory budget (docs/scale.md §2): a search gives them an allowance (a
//! share of its budget), each thread may hold its share of it, and the
//! allowance shrinks under memory pressure. The caches publish what they
//! hold, so that the budget accounts for it.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::mem::Gauge;

/// The bytes each thread's caches may hold (`usize::MAX`: no limit but
/// their own).
static SHARE: AtomicUsize = AtomicUsize::new(usize::MAX);
/// The threads the allowance is shared by.
static THREADS: AtomicUsize = AtomicUsize::new(1);
/// The bytes held by the caches of all threads (as published), and the
/// most held at a time.
static GAUGE: Gauge = Gauge::new();

/// The least share of a thread (a smaller cache is not worth keeping).
pub const MIN_SHARE: usize = 1 << 20;

/// The part of a search's memory budget that the caches may hold.
pub const BUDGET_FRACTION: usize = 8;

/// Share `total` bytes among `threads` threads (`usize::MAX`: no limit).
pub fn set_allowance(total: usize, threads: usize) {
    let threads = threads.max(1);
    THREADS.store(threads, Ordering::Relaxed);
    let share = if total == usize::MAX { usize::MAX } else { (total / threads).max(MIN_SHARE) };
    SHARE.store(share, Ordering::Relaxed);
}

/// The bytes the calling thread's caches may hold.
#[inline]
pub fn share() -> usize {
    SHARE.load(Ordering::Relaxed)
}

/// The threads the allowance is shared by (those of the last search; 1
/// before any).
#[inline]
pub fn threads() -> usize {
    THREADS.load(Ordering::Relaxed)
}

/// Halve the allowance (memory pressure); false if it is at its least
/// already. The threads clear their caches when they next exceed it.
pub fn shrink() -> bool {
    let cur = SHARE.load(Ordering::Relaxed);
    let cur = if cur == usize::MAX { (GAUGE.bytes() / THREADS.load(Ordering::Relaxed)).max(MIN_SHARE) } else { cur };
    let next = (cur / 2).max(MIN_SHARE);
    SHARE.store(next, Ordering::Relaxed);
    next < cur
}

/// The allowance of all threads (bytes; `usize::MAX`: none).
pub fn allowance() -> usize {
    SHARE.load(Ordering::Relaxed).saturating_mul(THREADS.load(Ordering::Relaxed))
}

/// The bytes held by the caches of all threads (as published by their
/// `Published`), and the most held at a time.
pub fn gauge() -> &'static Gauge {
    &GAUGE
}

/// What a thread's cache has published of its size: it publishes when the
/// size moved by more than `STEP` since. (Its part of the gauge, as a
/// `mem::Part`, but owned by its thread and published in steps.)
#[derive(Default)]
pub struct Published(usize);

impl Published {
    const STEP: usize = 1 << 20;

    /// The cache now holds `bytes`.
    #[inline]
    pub fn update(&mut self, bytes: usize) {
        if bytes.abs_diff(self.0) >= Self::STEP || (bytes == 0 && self.0 != 0) {
            if bytes >= self.0 {
                GAUGE.add(bytes - self.0);
            } else {
                GAUGE.sub(self.0 - bytes);
            }
            self.0 = bytes;
        }
    }
}

impl Drop for Published {
    fn drop(&mut self) {
        GAUGE.sub(self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_and_publishing() {
        set_allowance(64 << 20, 8);
        assert_eq!(share(), 8 << 20);
        assert_eq!(allowance(), 64 << 20);
        assert!(shrink());
        assert_eq!(share(), 4 << 20);
        set_allowance(1 << 20, 8);
        assert_eq!(share(), MIN_SHARE);
        assert!(!shrink());
        let before = gauge().bytes();
        {
            let mut p = Published::default();
            p.update(1000);
            assert_eq!(gauge().bytes(), before);
            p.update(3 << 20);
            assert!(gauge().bytes() >= before + (3 << 20));
        }
        set_allowance(usize::MAX, 1);
        assert_eq!(share(), usize::MAX);
    }
}
