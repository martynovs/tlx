//! Run alone: the process clock includes every thread, including other tests.

use std::time::Duration;
use tlx_base::time::process_cpu;

fn main() {
    let t0 = process_cpu();
    let mut acc = 0u64;
    for i in 0..500_000u64 {
        acc = acc.wrapping_add(std::hint::black_box(i));
    }
    std::hint::black_box(acc);
    let t1 = process_cpu();
    assert!(t1 >= t0, "the clock went backwards: {t0:?} -> {t1:?}");
    assert!(t1 - t0 >= Duration::from_micros(50), "burning CPU did not advance the clock: {:?}", t1 - t0);
    std::thread::sleep(Duration::from_millis(60));
    let t2 = process_cpu();
    assert!(t2 - t1 < Duration::from_millis(30), "waiting counted as CPU time: {:?}", t2 - t1);
}
