//! Builds of the same library: one compiler run at a time (the others wait
//! for it), and a build nobody waits for still lands in the cache.

use std::path::{Path, PathBuf};

use tlx_native::build::{build, cached, start, BuildOptions};

const SRC: &str = "int tlx_concurrent_build_test(int x) { return x + 1; }\n";

/// A scratch directory, and build options whose compiler is a script that
/// logs each run into `runs` and pauses (so that builds overlap) before
/// running the real compiler.
fn setup(name: &str) -> (PathBuf, BuildOptions) {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("tlx-native-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let real = BuildOptions::default().cc;
    let script = dir.join("cc.sh");
    std::fs::write(&script, format!("#!/bin/sh\necho run >> '{}'\nsleep 1\nexec {real} \"$@\"\n", dir.join("runs").display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let opts = BuildOptions { cc: script.display().to_string(), cache_dir: dir.join("cache"), ..Default::default() };
    (dir, opts)
}

/// How many times the compiler ran.
fn runs(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("runs")).map_or(0, |s| s.lines().count())
}

#[test]
fn concurrent_builds_compile_once() {
    let (dir, opts) = setup("concurrent");
    let libs: Vec<Result<PathBuf, String>> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..4).map(|_| s.spawn(|| build(SRC, &opts))).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let first = libs[0].clone().expect("build");
    assert!(libs.iter().all(|l| l.as_ref() == Ok(&first)), "{libs:?}");
    assert_eq!(runs(&dir), 1, "the compiler ran more than once");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unawaited_build_completes_into_the_cache() {
    let (dir, opts) = setup("unawaited");
    drop(start(SRC, &opts).expect("start"));
    assert!(cached(SRC, &opts).is_none(), "the build finished too early for the test");
    // Another build of the library waits for the first one.
    let lib = build(SRC, &opts).expect("build");
    assert_eq!(cached(SRC, &opts), Some(lib));
    assert_eq!(runs(&dir), 1, "the compiler ran more than once");
    let _ = std::fs::remove_dir_all(&dir);
}
