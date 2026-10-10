//! Throughput of the explorer on a trivial native model (run with
//! `cargo test --release -p tlx-native --test bench_native -- --ignored --nocapture`).

mod common;

use common::{NativeModel, COUNTERS};
use tlx_native::build::{build, BuildOptions};
use tlx_native::native::NativeLib;
use tlx_runtime::{explore, Options};

#[test]
#[ignore]
fn bench_counters() {
    let src = COUNTERS.replace("#define N 30", "#define N 2000").replace("BAD", "0");
    let dir = std::env::temp_dir().join("tlx-native-bench");
    let lib = build(&src, &BuildOptions { cache_dir: dir, ..Default::default() }).unwrap();
    // SAFETY: COUNTERS implements the ABI with static metadata and two-word
    // buffers; only the bound changes. The test compiler/cache are trusted.
    let m = NativeModel { lib: unsafe { NativeLib::load(&lib) }.unwrap() };
    for workers in [1, 4, 8, 16] {
        let r = explore(&m, &Options { workers, check_deadlock: false, ..Default::default() });
        let secs = r.elapsed.as_secs_f64();
        println!(
            "workers {workers:2}: {} distinct in {secs:.2}s = {:.1} M distinct/s",
            r.stats.distinct,
            r.stats.distinct as f64 / secs / 1e6
        );
    }
}
