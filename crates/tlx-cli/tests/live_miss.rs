//! Layout misses after the compiled search: nothing restarts then,
//! so a phase after it must not end in a layout miss. tests/specs/LVM1 and
//! LVM2 (a property that holds, one that is violated): with a one-state
//! pre-run the search misses and resumes, records no liveness edges, and a
//! materialized definition read by a native liveness atom does not fit its
//! shape, which only the liveness phase finds (the interpreter evaluates
//! that atom). A miss injected after the search (`TLX_INJECT_MISMATCH=miss`)
//! has the liveness phase, or the `-dump` search, redone by the interpreter.
//! The results are the interpreter's (and TLC's).

use std::path::Path;

/// A directory holding copies of tests/specs/LVM1 and LVM2 (one per test).
fn spec_dir(_test: &str) -> tlx_testkit::TempDir {
    let specs = tlx_testkit::cli::tests_specs();
    let dir = tlx_testkit::TempDir::new("live_miss");
    for f in ["LVM1.tla", "LVM1.cfg", "LVM2.tla", "LVM2.cfg"] {
        std::fs::copy(specs.join(f), dir.join(f)).unwrap();
    }
    dir
}

/// `tlx check NAME.tla` with `extra` arguments and environment `env`: exit
/// code and output (stdout and stderr).
fn run(dir: &Path, name: &str, extra: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", &format!("{name}.tla")]).args(extra).dir(dir);
    let out = cmd.envs(env.iter().copied()).run();
    (out.code, out.text())
}

use tlx_testkit::cli::counts;

/// The counterexample of a liveness violation (its state lines).
fn counterexample(out: &str) -> Vec<&str> {
    out.lines()
        .skip_while(|l| !l.starts_with("Error: The following behavior"))
        .take_while(|l| !l.starts_with("Finished checking temporal properties"))
        .collect()
}

#[test]
fn miss_found_after_the_search_takes_the_interpreters_verdict() {
    let dir = spec_dir("verdict");
    let env = [("TLX_PRERUN", "1"), ("TLX_NATIVE_SYNC", "1")];
    // TLC: LVM1 holds, LVM2 violates Prop.
    for (name, want) in [("LVM1", 0), ("LVM2", 13)] {
        let (code, plain) = run(&dir, name, &["-workers", "1"], &[]);
        assert_eq!(code, want, "{plain}");
        for workers in ["1", "8"] {
            let (code, out) = run(&dir, name, &["-compile", "-workers", workers], &env);
            assert_eq!((code, counts(&out)), (want, counts(&plain)), "{name} -workers {workers}:\n{out}");
            assert!(out.contains("compile: layout miss on "), "no miss in the search:\n{out}");
            // (The atom is evaluated by the interpreter: the liveness phase
            // is not redone.)
            assert!(!out.contains("Error: layout miss") && !out.contains("with the interpreter"), "{out}");
            if workers == "1" {
                assert_eq!(counterexample(&out), counterexample(&plain), "{name}:\n{out}");
            }
        }
    }
}

#[test]
fn miss_injected_after_the_search_is_redone_by_the_interpreter() {
    let dir = spec_dir("inject");
    let (code, plain) = run(&dir, "LVM2", &["-workers", "1", "-dump", "plain.dump"], &[]);
    assert_eq!(code, 13, "{plain}");
    assert!(!counterexample(&plain).is_empty(), "{plain}");
    // The liveness phase rebuilds the counterexample with the compiled
    // model (the 4th evaluation after the search misses): the interpreter
    // searches again, recording the graph, and checks the properties.
    let env = [("TLX_NATIVE_SYNC", "1"), ("TLX_INJECT_MISMATCH", "miss:3")];
    let (code, out) = run(&dir, "LVM2", &["-compile", "-workers", "1"], &env);
    assert!(out.contains("compile: layout miss checking temporal properties; checking them with the interpreter"), "{out}");
    assert_eq!((code, counts(&out), counterexample(&out)), (13, counts(&plain), counterexample(&plain)), "{out}");
    // The -dump search (its first compiled action run misses).
    let env = [("TLX_NATIVE_SYNC", "1"), ("TLX_INJECT_MISMATCH", "miss")];
    let (code, out) = run(&dir, "LVM2", &["-compile", "-workers", "1", "-dump", "compiled.dump"], &env);
    assert!(out.contains("compile: layout miss writing the -dump file; writing it with the interpreter"), "{out}");
    assert_eq!((code, counts(&out), counterexample(&out)), (13, counts(&plain), counterexample(&plain)), "{out}");
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
    assert_eq!(read("compiled.dump"), read("plain.dump"));
}
