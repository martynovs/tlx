//! A field the pre-run saw with one TLC model-value type, that a later state
//! stores a value of another type into.
//!
//! The layout gives such a field the model values of the type it saw (at most
//! all of them, plus the untyped ones), so the later value does not fit: the
//! compiled tier reports a layout miss, widens the shape, re-runs, and ends
//! with the interpreter's results. A universe that already held every model
//! value of the model would have hidden the value in a shape that claims two
//! differently-typed model values compare as values, which TLC refuses
//! (`tlx-layout`'s `extend_atoms`).

use std::path::Path;

const SPEC: &str = r#"---- MODULE AtomTypes ----
EXTENDS Naturals, Sequences, SequencesExt, FiniteSets, TLC

CONSTANT T_a, T_b, S_c, R_x

VARIABLES n, history

Limit == 3

Step(i) == [tx |-> i, type |-> T_a]
Resp(i) == [type |-> T_b, tx_id |-> <<0, i>>, status |-> S_c]
Alien == [type |-> T_b, tx_id |-> <<0, 0>>, status |-> R_x]

Init == n = 0 /\ history = <<>>

Next == \/ n < Limit /\ n' = n + 1 /\ history' = Append(history, Step(n))
        \/ n = Limit /\ n' = n + 1 /\ history' = Append(history, Resp(0))
        \/ n > Limit /\ n < 150 /\ n' = n + 1 /\ UNCHANGED history
        \/ n = 150 /\ n' = 151 /\ history' = Append(history, Alien)

\* The pre-run's records only; the alien below shares their arm, so it is
\* never compared with them (TLC refuses to compare R_x with S_c).
Indexes == {i \in DOMAIN history : i = Limit + 1}
S == {history[i] : i \in Indexes}
Sorted == SortSeq(SetToSeq(S), LAMBDA x, y : x.tx_id[2] < y.tx_id[2])

Inv == /\ Len(Sorted) = Cardinality(S)
       /\ \A i \in 1..Len(Sorted) : Sorted[i] \in S
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv\nCHECK_DEADLOCK FALSE\n\
CONSTANT T_a = T_a\nCONSTANT T_b = T_b\nCONSTANT S_c = S_c\nCONSTANT R_x = R_x\n";

/// A directory holding the spec as AtomTypes.tla (one per test).
fn spec_dir(test: &str) -> tlx_testkit::TempDir {
    tlx_testkit::TempDir::with_spec(test, SPEC, CFG)
}

/// `tlx check` with `extra` arguments and environment `env`: exit code and
/// output (stdout and stderr).
fn run(dir: &Path, extra: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", "AtomTypes.tla", "-config", "AtomTypes.cfg", "-workers", "1"]).args(extra).dir(dir);
    let out = cmd.envs(env.iter().copied()).run();
    (out.code, out.text())
}

use tlx_testkit::cli::counts;

fn misses(out: &str) -> usize {
    out.lines().filter(|l| l.starts_with("compile: layout miss")).count()
}

/// The value of another type is a layout miss, and the widened run has the
/// interpreter's results.
#[test]
fn a_later_value_of_another_type_is_a_layout_miss() {
    let dir = spec_dir("atom-types");
    let (code, plain) = run(&dir, &[], &[]);
    assert_eq!(code, 0, "{plain}");
    // A 50-state pre-run stops far below the state that stores the alien
    // model value (the chain is 151 states long).
    let env = [("TLX_PRERUN", "50")];
    let (code, out) = run(&dir, &["-compile"], &env);
    assert_eq!(code, 0, "{out}");
    assert!(misses(&out) >= 1, "{env:?}: no layout miss:\n{out}");
    // The miss is the variable holding the alien model value: a shape that
    // carried every model value of the model would not have missed.
    assert!(out.lines().any(|l| l.starts_with("compile: layout miss on history")), "{env:?}:\n{out}");
    assert_eq!(counts(&out), counts(&plain), "{env:?}:\n{out}");
    // The widened layout keeps the value: no further restart is needed.
    assert!(!out.contains("continuing with dynamic values"), "{out}");
}
