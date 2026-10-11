//! Clocks that measure the work done, not the machine's load.
//!
//! The wall clock measures how busy the machine is as much as the work a
//! thread does. A heuristic that has to decide the same thing on a loaded
//! machine and on an idle one (the pre-run's settle rule, design.md §6.5.1)
//! reads CPU time instead: it advances only while the process runs.

use std::time::Duration;

/// CPU time consumed by the process (all its threads) so far.
///
/// Monotonic and independent of the machine's load: the same run on a busy
/// machine reads about the same value as on an idle one, where
/// `Instant::now` would report the waiting. Waiting (sleeping, blocked I/O)
/// does not count.
#[cfg(unix)]
pub fn process_cpu() -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `clock_gettime` only writes the timespec it is given.
    if unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) } == 0 {
        return Duration::new(ts.tv_sec.max(0) as u64, ts.tv_nsec.clamp(0, 999_999_999) as u32);
    }
    // (No supported clock: the wall clock since the first call is the
    // closest substitute, and only loses the load independence.)
    wall_since_start()
}

/// `process_cpu` where there is no process CPU clock to read.
#[cfg(not(unix))]
pub fn process_cpu() -> Duration {
    wall_since_start()
}

fn wall_since_start() -> Duration {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}
