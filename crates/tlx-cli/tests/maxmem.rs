//! The memory budget (`-maxmem`, docs/scale.md §2): a budget too small for
//! anything forces every stage of memory pressure at once (the archive for
//! resuming is dropped, the queue compressed, the fingerprint set does not
//! grow) without changing the results; a fingerprint set that cannot grow
//! within the budget rotates to disk, or with `-spill off` fills up with a
//! capacity error asking for a larger `-maxmem`. With the default budget a
//! layout miss resumes the search from the archive.

use std::path::Path;

/// 300,000 states: more than a fingerprint set of 1 MiB holds.
const BIG: &str = "---- MODULE MemBig ----\nEXTENDS Naturals\nVARIABLE x\nInit == x = 0\nNext == x < 300000 /\\ x' = x + 1\n====\n";

/// 6,400 states, strings and counters that outgrow a small pre-run's layout.
const WIDEN: &str = r#"---- MODULE MemWiden ----
EXTENDS Naturals, Sequences, TLC
VARIABLES x, y, s
Init == x = 0 /\ y = 0 /\ s = "n0"
Next == \/ x < 79 /\ x' = x + 1 /\ y' = y /\ s' = "n" \o ToString(x' * 1000 + y')
        \/ y < 79 /\ y' = y + 1 /\ x' = x /\ s' = "n" \o ToString(x' * 1000 + y')
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n";

/// A directory holding spec `name` (one per test).
fn spec_dir(_test: &str, name: &str, src: &str) -> tlx_testkit::TempDir {
    let dir = tlx_testkit::TempDir::new("maxmem");
    std::fs::write(dir.join(format!("{name}.tla")), src).unwrap();
    std::fs::write(dir.join(format!("{name}.cfg")), CFG).unwrap();
    dir
}

/// `tlx check name.tla` with `args` and environment `env`: exit code and
/// output (stdout and stderr).
fn run(dir: &Path, name: &str, args: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", &format!("{name}.tla"), "-config", &format!("{name}.cfg")]).args(args).dir(dir);
    let out = cmd.envs(env.iter().copied()).run();
    (out.code, out.text())
}

use tlx_testkit::cli::counts;

/// The fingerprint set cannot grow beyond the budget: with `-spill off`, a
/// capacity error asking for a larger `-maxmem`, no behavior, exit 255;
/// `-stats` shows the budget and the last stage of pressure. With the disk
/// tier (the default) it rotates to disk and the search completes.
#[test]
fn full_within_the_budget() {
    let dir = spec_dir("full", "MemBig", BIG);
    for args in
        [&["-maxmem", "1", "-stats", "-spill", "off"][..], &["-maxmem", "1", "-compile", "-workers", "4", "-stats", "-spill", "off"]]
    {
        let (code, out) = run(&dir, "MemBig", args, &[]);
        assert_eq!(code, 255, "{args:?}:\n{out}");
        assert!(out.contains("Error: The fingerprint set is full: it cannot grow within the memory budget; rerun with a larger -maxmem.\n  Distinct states found so far: "), "{out}");
        assert!(
            out.contains("\n  Within -maxmem 1 MiB the fingerprint set holds 117965 states.\n  Suggested -maxmem: 2 MiB or more.\n"),
            "{out}"
        );
        assert!(!out.contains("State 1:"), "{out}");
        assert!(out.contains("budget (-maxmem)                     1.0 MiB"), "{out}");
        assert!(out.contains("budget pressure stage                5 (fingerprint set to disk; stopped growing with -spill off)"), "{out}");
        let args = &args[..args.len() - 2];
        let (code, out) = run(&dir, "MemBig", args, &[]);
        assert_eq!(code, 0, "{args:?}:\n{out}");
        assert_eq!(
            counts(&out),
            [
                "300001 states generated, 300001 distinct states found, 0 states left on queue.",
                "The depth of the complete state graph search is 300001."
            ],
            "{out}"
        );
        assert!(out.contains("budget pressure stage                5 (fingerprint set to disk; stopped growing with -spill off)"), "{out}");
        assert!(out.contains("fpset rotations to disk"), "{out}");
    }
    // `TLX_MAXMEM` is the same; `-maxmem` wins.
    let (code, out) = run(&dir, "MemBig", &["-spill", "off"], &[("TLX_MAXMEM", "1M")]);
    assert_eq!(code, 255, "{out}");
    assert!(out.contains("Within -maxmem 1 MiB"), "{out}");
    let (code, out) = run(&dir, "MemBig", &["-maxmem", "64", "-spill", "off"], &[("TLX_MAXMEM", "1M")]);
    assert_eq!(code, 0, "{out}");
}

/// A tiny budget forces every stage from the start (the archive and the
/// queue on disk: layout misses resume from the archive; with `-spill off`
/// no archive, misses start over, the queue only compressed; no growth of
/// the set) with the results of an unlimited run; the default budget
/// resumes after a miss.
#[test]
fn tiny_budget_same_results() {
    let dir = spec_dir("tiny", "MemWiden", WIDEN);
    let (code, plain) = run(&dir, "MemWiden", &[], &[]);
    assert_eq!(code, 0, "{plain}");
    assert_eq!(
        counts(&plain),
        [
            "12641 states generated, 6400 distinct states found, 0 states left on queue.",
            "The depth of the complete state graph search is 159."
        ]
    );
    for workers in ["1", "4"] {
        let (code, out) = run(&dir, "MemWiden", &["-compile", "-workers", workers], &[("TLX_PRERUN", "50")]);
        assert_eq!((code, counts(&out)), (0, counts(&plain)), "{out}");
        assert!(out.contains("compile: resuming the search at depth"), "{out}");
        let (code, out) = run(&dir, "MemWiden", &["-compile", "-workers", workers, "-maxmem", "1", "-stats"], &[("TLX_PRERUN", "50")]);
        assert_eq!((code, counts(&out)), (0, counts(&plain)), "{out}");
        assert!(out.contains("compile: layout miss"), "{out}");
        assert!(out.contains("compile: resuming the search"), "{out}");
        assert!(out.contains("disk: archive segments spilled"), "{out}");
        assert!(!out.contains("archived states (for resuming)       dropped"), "{out}");
        assert!(
            out.contains("budget pressure stage                4 (frontier compressed, then to disk; only compressed with -spill off)"),
            "{out}"
        );
        let (code, out) =
            run(&dir, "MemWiden", &["-compile", "-workers", workers, "-maxmem", "1", "-spill", "off", "-stats"], &[("TLX_PRERUN", "50")]);
        assert_eq!((code, counts(&out)), (0, counts(&plain)), "{out}");
        assert!(out.contains("compile: layout miss"), "{out}");
        assert!(!out.contains("compile: resuming the search"), "{out}");
        assert!(out.contains("archived states (for resuming)       dropped"), "{out}");
        assert!(
            out.contains("budget pressure stage                4 (frontier compressed, then to disk; only compressed with -spill off)"),
            "{out}"
        );
        assert!(!out.contains("disk: "), "{out}");
        let (code, out) = run(&dir, "MemWiden", &["-workers", workers, "-maxmem", "1"], &[]);
        assert_eq!((code, counts(&out)), (0, counts(&plain)), "{out}");
    }
}
