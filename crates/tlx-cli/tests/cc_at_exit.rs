//! `TLX_CC_AT_EXIT_MS`: with `TLX_JIT=tier`, a compiled run that ends before
//! the delayed C build started (`TLX_CC_DELAY_MS`) still starts the build at
//! its end when the JIT compilation was expensive, so the cache is filled for
//! the next run. It must not start one when the threshold is off.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SPEC: &str = r#"---- MODULE C ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 6
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\n";

/// A directory holding the spec; `name` keeps the two tests' caches apart.
fn work_dir(_name: &str) -> tlx_testkit::TempDir {
    tlx_testkit::TempDir::with_spec("cc_at_exit", SPEC, CFG)
}

/// Run `tlx check -compile` with a fresh `TLX_CACHE` and the two timing
/// knobs; the run is short and the delayed build far away, so only the
/// at-exit build can fill the cache.
fn run(dir: &Path, cache: &Path, at_exit_ms: &str) -> String {
    let out = tlx_testkit::tlx!()
        .args(["check", "C.tla", "-config", "C.cfg", "-workers", "1", "-compile"])
        .dir(dir)
        .env("TLX_CACHE", cache)
        .env("TLX_CC_DELAY_MS", "60000")
        .env("TLX_CC_AT_EXIT_MS", at_exit_ms)
        // (The caller's environment must not change the tier.)
        .run();
    out.text()
}

/// The built library anywhere under `cache`, if there is one.
fn find_lib(cache: &Path) -> Option<PathBuf> {
    let mut stack = vec![cache.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n == "model.dylib" || n == "model.so") {
                return Some(p);
            }
        }
    }
    None
}

/// Wait for the at-exit build (it finishes after tlx exits) to land in the
/// cache, up to 120 s.
fn wait_for_lib(cache: &Path) -> Option<PathBuf> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(lib) = find_lib(cache) {
            return Some(lib);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn at_exit_build_fills_the_cache_when_on() {
    let dir = work_dir("on");
    let cache = dir.join("cache");
    // (Any JIT compilation, even one well under a millisecond.)
    let text = run(&dir, &cache, "0.001");
    assert!(text.contains("native code:"), "the run did not use native code:\n{text}");
    let lib = wait_for_lib(&cache).expect("the C build started at exit did not land in the cache");
    assert!(lib.exists());
}

#[test]
fn no_at_exit_build_when_off() {
    let dir = work_dir("off");
    let cache = dir.join("cache");
    let text = run(&dir, &cache, "0");
    assert!(text.contains("native code:"), "the run did not use native code:\n{text}");
    // The delayed build is 60 s away and the threshold off: nothing ever
    // starts, so a short grace period is enough to be sure.
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(find_lib(&cache), None, "the cache was filled with TLX_CC_AT_EXIT_MS=0");
}
