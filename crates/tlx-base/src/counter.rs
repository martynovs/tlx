//! A statistics counter that many threads add to without sharing a cache
//! line.
//!
//! A single `AtomicU64` that every search worker increments makes its cache
//! line move between the cores on every increment, and every other field on
//! that line with it (fpset/count.rs measured an insert twice as slow with
//! 8 threads for such a counter). `Striped` keeps one padded stripe per
//! thread (threads beyond `STRIPES` share stripes, still counting exactly)
//! and sums them when read; reads are meant to be rare (reports).

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::padded::Padded;

/// The number of stripes shared by statistics and fingerprint-set counters.
pub const STRIPES: usize = 64;

/// A counter added to by many threads, read rarely (see the module).
pub struct Striped {
    stripes: Box<[Padded<AtomicU64>]>,
}

impl Default for Striped {
    fn default() -> Striped {
        Striped::new()
    }
}

impl std::fmt::Debug for Striped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Striped({})", self.get())
    }
}

/// The calling thread's stripe in `0..STRIPES` (assigned on first use,
/// round robin, and stable for the thread's lifetime).
///
/// All counters share this assignment, including fingerprint-set inserts
/// without a search-worker number. Sharing changes which threads collide
/// compared with independent allocators; it does not guarantee no contention.
#[inline]
pub fn this_thread() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static STRIPE: Cell<usize> = const { Cell::new(usize::MAX) };
    }
    STRIPE.with(|s| {
        let i = s.get();
        if i != usize::MAX {
            return i;
        }
        let i = NEXT.fetch_add(1, Ordering::Relaxed) % STRIPES;
        s.set(i);
        i
    })
}

impl Striped {
    pub fn new() -> Striped {
        Striped { stripes: (0..STRIPES).map(|_| Padded::default()).collect() }
    }

    /// Add `n` (in the calling thread's stripe).
    #[inline]
    pub fn add(&self, n: u64) {
        self.stripes[this_thread()].fetch_add(n, Ordering::Relaxed);
    }

    /// The sum of what was added (exact once no addition is in progress).
    pub fn get(&self) -> u64 {
        self.stripes.iter().map(|s| s.load(Ordering::Relaxed)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_is_stable_across_counters() {
        std::thread::spawn(|| {
            let stripe = this_thread();
            assert!(stripe < STRIPES);
            for n in [1, 5] {
                let c = Striped::new();
                c.add(n);
                assert_eq!(this_thread(), stripe);
                for (i, s) in c.stripes.iter().enumerate() {
                    assert_eq!(s.load(Ordering::Relaxed), if i == stripe { n } else { 0 });
                }
            }
        })
        .join()
        .unwrap();
    }

    #[test]
    fn exact_across_threads_sharing_stripes() {
        let c = Striped::new();
        let per = if cfg!(miri) { 100 } else { 10_000 };
        // More threads than stripes: some share one.
        let threads = if cfg!(miri) { 4 } else { STRIPES + 6 };
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| (0..per).for_each(|_| c.add(1)));
            }
        });
        assert_eq!(c.get(), (threads * per) as u64);
        c.add(5);
        assert_eq!(c.get(), (threads * per) as u64 + 5);
    }

    #[test]
    fn stripes_do_not_share_lines() {
        assert_eq!(STRIPES, 64);
        assert_eq!(Striped::new().stripes.len(), 64);
        assert_eq!(std::mem::align_of::<Padded<AtomicU64>>(), 128);
        assert_eq!(std::mem::size_of::<Padded<AtomicU64>>(), 128);
    }
}
