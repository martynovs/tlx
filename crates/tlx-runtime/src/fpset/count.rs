//! The number of entries of a fingerprint set, counted without a counter
//! that every insert writes: with 8 threads inserting, that one
//! cache line made an insert about twice as slow.
//!
//! Each thread counts its inserts in a stripe of its own (`STRIPES` of
//! them, picked by the worker's number or else by a per-thread index;
//! threads that share one still count exactly, with atomic additions), and
//! moves them to the shared `flushed`
//! counter every `BATCH` entries. So `flushed` is behind the number of
//! entries by less than `SLACK`, and `get` (flushed plus the stripes) is
//! exact once no insert is in progress.
//!
//! Near the load limits of a table, the set counts in `flushed` directly
//! (`FpSet::insert`): the first such insert moves every stripe there
//! (`drain`), so that the hard limit and the high watermark are checked
//! against the exact number, as with a single counter.

use std::sync::atomic::{AtomicUsize, Ordering};

use tlx_base::counter::STRIPES;
use tlx_base::padded::Padded;

// Search workers pass their number directly; other inserts share the
// statistics counters' thread assignment, whose contention may differ
// from an independent allocator. Count's batching policy stays here.
pub(super) use tlx_base::counter::this_thread;

/// A stripe moves its entries to `flushed` when it holds this many.
const BATCH: usize = 64;

/// `flushed` is behind the number of entries by less than this (plus
/// one per insert in progress).
pub(super) const SLACK: usize = STRIPES * BATCH;

pub(super) struct Count {
    flushed: Padded<AtomicUsize>,
    stripes: Box<[Padded<AtomicUsize>]>,
}

impl Count {
    pub(super) fn new() -> Count {
        Count { flushed: Padded(AtomicUsize::new(0)), stripes: (0..STRIPES).map(|_| Padded(AtomicUsize::new(0))).collect() }
    }

    /// The entries moved to the shared counter: at most the number of
    /// entries, and less than `SLACK` behind it (plus one per insert in
    /// progress).
    #[inline]
    pub(super) fn flushed(&self) -> usize {
        self.flushed.0.load(Ordering::Relaxed)
    }

    /// Count an entry in stripe `stripe` (modulo their number).
    #[inline]
    pub(super) fn add_striped(&self, stripe: usize) {
        let s = &self.stripes[stripe % STRIPES].0;
        if s.fetch_add(1, Ordering::Relaxed) + 1 >= BATCH {
            let n = s.swap(0, Ordering::Relaxed);
            self.flushed.0.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// Count `n` entries in the shared counter; returns its new value.
    #[inline]
    pub(super) fn add_direct(&self, n: usize) -> usize {
        self.flushed.0.fetch_add(n, Ordering::Relaxed) + n
    }

    /// Move the entries of every stripe to the shared counter.
    pub(super) fn drain(&self) {
        for s in self.stripes.iter() {
            if s.0.load(Ordering::Relaxed) != 0 {
                let n = s.0.swap(0, Ordering::Relaxed);
                self.flushed.0.fetch_add(n, Ordering::Relaxed);
            }
        }
    }

    /// The number of entries (exact when no insert is in progress).
    pub(super) fn get(&self) -> usize {
        self.stripes.iter().fold(self.flushed(), |n, s| n + s.0.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batching_slack_and_drain_keep_fixed_layout() {
        let c = Count::new();
        assert_eq!(c.stripes.len(), 64);
        assert_eq!(SLACK, 4096);
        for stripe in 0..STRIPES {
            for _ in 0..BATCH - 1 {
                c.add_striped(stripe);
            }
        }
        assert_eq!(c.flushed(), 0);
        assert_eq!(c.get(), STRIPES * (BATCH - 1));
        assert!(c.get() < SLACK);
        // Modulo addressing still flushes exactly at the batch boundary.
        c.add_striped(STRIPES);
        assert_eq!(c.flushed(), BATCH);
        c.drain();
        let exact = STRIPES * (BATCH - 1) + 1;
        assert_eq!((c.flushed(), c.get()), (exact, exact));
        assert_eq!(c.add_direct(2), exact + 2);
        c.drain();
        assert_eq!((c.flushed(), c.get()), (exact + 2, exact + 2));
    }

    /// Threads sharing stripes, counting in them and directly while one
    /// drains them: the count is exact afterwards, and `flushed` is within
    /// `SLACK` of it before the drain.
    #[test]
    fn exact_with_shared_stripes_and_drains() {
        let c = Count::new();
        let per = if cfg!(miri) { 200 } else { 20_000 };
        std::thread::scope(|s| {
            for t in 0..8 {
                let c = &c;
                s.spawn(move || {
                    for i in 0..per {
                        // (Stripes 0..4: two threads on each.)
                        if i % 7 == 0 {
                            c.add_direct(1);
                        } else {
                            c.add_striped(t % 4);
                        }
                        if t == 0 && i % 1000 == 0 {
                            c.drain();
                        }
                    }
                });
            }
        });
        assert_eq!(c.get(), 8 * per);
        assert!(c.flushed() + SLACK > c.get());
        c.drain();
        assert_eq!((c.flushed(), c.get()), (8 * per, 8 * per));
    }
}
