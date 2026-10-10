//! A full fingerprint set (`-fpmem` too small) is a resource limit, not a
//! violation of the specification: a capacity error with the numbers of the
//! run and a system-error exit code, and no behavior (the trace of the state
//! that could not be inserted can have tens of thousands of states, one per
//! state found).
//!
//! `-fpmem 1` is the smallest the CLI accepts; it holds 117,965 states, so
//! the specs below (300,000 reachable states, or 200,001 initial ones) fill
//! the set without any test-only knob.

use std::path::Path;

/// 300,000 states: more than the set holds.
const SPEC: &str = r#"---- MODULE FpFull ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 300000 /\ x' = x + 1
====
"#;

/// Both predicates.
const CFG: &str = "INIT Init\nNEXT Next\n";

/// `Init` alone generates more states than the set holds.
const SPEC_INIT: &str = r#"---- MODULE FpFullInit ----
EXTENDS Naturals
VARIABLE x
Init == x \in 0..200000
Next == UNCHANGED x
====
"#;

/// The same, with no next-state action at all (`no_next`: TLC fails with
/// EC.TLC_STATES_AND_NO_NEXT_ACTION unless the set fills up first).
const SPEC_NO_NEXT: &str = r#"---- MODULE FpFullNoNext ----
EXTENDS Naturals
VARIABLE x
Init == x \in 0..200000
====
"#;

/// Run `tlx check <name>.tla -fpmem 1` with `args`: the whole output
/// (stdout and stderr) and the exit code.
fn run(dir: &Path, name: &str, args: &[&str]) -> (String, i32) {
    let out = tlx_testkit::tlx!()
        .args(["check", &format!("{name}.tla"), "-config", &format!("{name}.cfg"), "-fpmem", "1"])
        .args(args)
        .dir(dir)
        .run();
    (out.text(), out.code)
}

/// A run that filled the set: exit 255 (the system-error status, as TLC's
/// out-of-memory), the message with the numbers of the run, and no behavior.
fn assert_full(text: &str, code: i32) {
    assert_eq!(code, 255, "{text}");
    assert!(
        text.contains("Error: The fingerprint set is full; rerun with a larger -fpmem.\n  Distinct states found so far: "),
        "the capacity message is missing:\n{text}"
    );
    assert!(text.contains("\n  -fpmem 1 MiB holds 117965 states.\n  Suggested -fpmem: 2 MiB or more.\n"), "{text}");
    assert!(!text.contains("State 1:"), "a trace was printed:\n{text}");
    assert!(!text.contains("The behavior up to this point is:"), "{text}");
    // (The trace would be one state per line: hundreds of thousands.)
    assert!(text.lines().count() < 40, "{} lines of output:\n{text}", text.lines().count());
    // It is not an evaluation error.
    assert!(!text.contains("exit code 75"), "{text}");
}

#[test]
fn interpreter_one_worker() {
    let dir = tlx_testkit::TempDir::with_spec("interp1", SPEC, CFG);
    let (text, code) = run(&dir, "FpFull", &["-workers", "1"]);
    assert_full(&text, code);
    assert!(text.contains("117966 states generated, 117965 distinct states found"), "{text}");
}

#[test]
fn interpreter_four_workers() {
    let dir = tlx_testkit::TempDir::with_spec("interp4", SPEC, CFG);
    let (text, code) = run(&dir, "FpFull", &["-workers", "4"]);
    assert_full(&text, code);
}

#[test]
fn compiled() {
    let dir = tlx_testkit::TempDir::with_spec("compile", SPEC, CFG);
    let (text, code) = run(&dir, "FpFull", &["-compile", "-workers", "4"]);
    assert_full(&text, code);
}

/// The set fills up while the initial states are generated: same error, the
/// state is not shown (as for other failures of the initial states).
#[test]
fn initial_states() {
    let dir = tlx_testkit::TempDir::with_spec("init", SPEC_INIT, CFG);
    let (text, code) = run(&dir, "FpFullInit", &["-workers", "4"]);
    assert_full(&text, code);
    assert!(!text.contains("x = 0"), "an initial state was printed:\n{text}");
}

/// A spec with no next-state action: TLC computes the initial states and
/// then fails with EC.TLC_STATES_AND_NO_NEXT_ACTION; if the set fills up
/// while computing them, that (resource) error is reported instead.
#[test]
fn no_next_state_action() {
    let dir = tlx_testkit::TempDir::with_spec("nonext", SPEC_NO_NEXT, "INIT Init\n");
    let (text, code) = run(&dir, "FpFullNoNext", &["-workers", "1"]);
    assert_full(&text, code);
    assert!(!text.contains("No next state actions defined"), "{text}");
}
