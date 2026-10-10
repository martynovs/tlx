//! Compiled code never stores a bitset with more elements than its set
//! shape admits: such a value is a layout miss, as for
//! sets kept as sorted arrays. Before, the compiled tier wrote it, a later
//! widening copied it into the resumed search, and `TLX_PARANOID` found
//! that its transcoded encoding differed from its encoding (which misses).

use std::path::Path;

/// The pre-run (one initial state) sees `s = {{}}`; the other initial
/// states fill bitsets over `Nodes` beyond the cardinality the layout
/// allows, and `t` then widens and resumes the search over them. (The
/// bitsets are set elements: a bitset elsewhere admits every subset of its
/// universe.)
const SPEC: &str = r#"---- MODULE BitCard ----
EXTENDS Naturals
VARIABLES s, t
Nodes == 0..6
Init == \E k \in 1..7 : s = {{m \in Nodes : m < k}} /\ t = 0
Next == t < 30 /\ t' = t + 1 /\ UNCHANGED s
TypeOK == t \in Nat
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nINVARIANT TypeOK\nCHECK_DEADLOCK FALSE\n";

fn spec_dir(_test: &str) -> tlx_testkit::TempDir {
    tlx_testkit::TempDir::with_spec("bitset_card", SPEC, CFG)
}

/// `tlx check spec args` in `dir` with environment `env`: exit code and
/// output (stdout and stderr).
fn run(dir: &Path, spec: &str, args: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let mut cmd = tlx_testkit::tlx!();
    cmd.args(["check", spec]).args(args).dir(dir);
    let out = cmd.envs(env.iter().copied()).run();
    (out.code, out.text())
}

use tlx_testkit::cli::counts;

/// Paranoid runs with forced misses (a one-state pre-run), on the IR
/// executor and on native code, with one and several workers.
const MODES: [&[(&str, &str)]; 2] = [
    &[("TLX_PARANOID", "1"), ("TLX_PRERUN", "1"), ("TLX_NO_NATIVE", "1")],
    &[("TLX_PARANOID", "1"), ("TLX_PRERUN", "1"), ("TLX_NATIVE_SYNC", "1")],
];

#[test]
fn bitset_over_cardinality_is_a_layout_miss() {
    let dir = spec_dir("miss");
    let (code, plain) = run(&dir, "BitCard.tla", &[], &[]);
    assert_eq!(code, 0, "{plain}");
    assert_eq!(
        counts(&plain),
        ["217 states generated, 217 distinct states found, 0 states left on queue.", "The depth of the complete state graph search is 31."]
    );
    for env in MODES {
        for workers in ["1", "4"] {
            let (code, out) = run(&dir, "BitCard.tla", &["-compile", "-workers", workers], env);
            assert_eq!((code, counts(&out)), (0, counts(&plain)), "{env:?} -workers {workers}:\n{out}");
            assert!(out.lines().any(|l| l.starts_with("compile: layout miss on s")), "{env:?} -workers {workers}:\n{out}");
            assert!(out.contains("compile: resuming the search"), "{env:?} -workers {workers}:\n{out}");
        }
    }
}

/// tests/specs/TU1: sets of pairs of union-encoded vertices.
#[test]
fn tu1_resumes_under_paranoid() {
    let specs = tlx_testkit::cli::tests_specs();
    let (code, plain) = run(&specs, "TU1.tla", &[], &[]);
    assert_eq!(code, 0, "{plain}");
    for env in MODES {
        let (code, out) = run(&specs, "TU1.tla", &["-compile", "-workers", "1"], env);
        assert_eq!((code, counts(&out)), (0, counts(&plain)), "{env:?}:\n{out}");
        assert!(out.contains("compile: resuming the search"), "{env:?}:\n{out}");
    }
}

/// The parameter of `Down` is inferred from the call `Down({k})`: a set
/// shape of at most one element, an intermediate shape that no state or
/// definition stores; the recursive call passes larger sets. Its maximal
/// cardinality must not be checked: the check failed each such evaluation,
/// which then fell back to the interpreter, with no layout miss to widen
/// the shape (dag-consensus/TLCSailfish1 ran 10x slower).
const FN_SPEC: &str = r#"---- MODULE BitCardFn ----
EXTENDS Naturals
VARIABLES s, t
Nodes == 0..6
RECURSIVE Down(_)
Down(S) == IF S = {} THEN {} ELSE LET c == {m \in Nodes : \E x \in S : m + 1 = x \/ m + 2 = x} IN c \cup Down(c)
Init == s \in SUBSET Nodes /\ t = 0
Next == \E k \in s : s' = (s \ {k}) \cup Down({k}) /\ t' = (t + 1) % 3
TypeOK == s \subseteq Nodes
====
"#;

#[test]
fn intermediate_bitsets_are_not_card_checked() {
    let dir = tlx_testkit::TempDir::new("bitset_card");
    std::fs::write(dir.join("BitCardFn.tla"), FN_SPEC).unwrap();
    std::fs::write(dir.join("BitCardFn.cfg"), "INIT Init\nNEXT Next\nINVARIANT TypeOK\nCHECK_DEADLOCK FALSE\n").unwrap();
    let (code, plain) = run(&dir, "BitCardFn.tla", &[], &[]);
    assert_eq!(code, 0, "{plain}");
    assert_eq!(
        counts(&plain),
        ["1452 states generated, 381 distinct states found, 0 states left on queue.", "The depth of the complete state graph search is 3."]
    );
    let modes: [&[(&str, &str)]; 3] =
        [&[("TLX_NO_NATIVE", "1")], &[("TLX_NATIVE_SYNC", "1")], &[("TLX_PARANOID", "1"), ("TLX_NATIVE_SYNC", "1")]];
    for env in modes {
        let (code, out) = run(&dir, "BitCardFn.tla", &["-compile"], env);
        assert_eq!((code, counts(&out)), (0, counts(&plain)), "{env:?}:\n{out}");
        assert!(!out.contains("fell back to the interpreter"), "{env:?}:\n{out}");
    }
}

/// `SetToSeq` (`CHOOSE f \in [1..n -> S] : IsInj(f)`) over the parameter
/// `S` of `Down`, inferred from the call `Down({k})`: an intermediate bitset
/// shape of at most one element whose values may hold any subset of `Nodes`
/// (a bounded-cardinality bitset). The collected elements' buffer was sized by that maximum, so
/// every larger set failed the evaluation, which fell back to the
/// interpreter.
const CHOOSE_SPEC: &str = r#"---- MODULE BitCardChoose ----
EXTENDS Naturals, Sequences, FiniteSets
Nodes == 0..6
IsInj(f) == \A a, b \in DOMAIN f : f[a] = f[b] => a = b
RECURSIVE Down(_)
Down(S) == IF S = {} THEN <<>>
           ELSE LET c == {m \in Nodes : \E x \in S : m + 1 = x \/ m + 2 = x}
                IN (CHOOSE f \in [1..Cardinality(S) -> S] : IsInj(f)) \o Down(c)
VARIABLES s, t
Init == s \in SUBSET Nodes /\ t = <<>>
Next == \E k \in s : s' = s \ {k} /\ t' = Down({k})
Inv == \A i \in DOMAIN t : t[i] \in Nodes
====
"#;

#[test]
fn choose_seq_over_intermediate_bitset() {
    let dir = tlx_testkit::TempDir::new("bitset_card");
    std::fs::write(dir.join("BitCardChoose.tla"), CHOOSE_SPEC).unwrap();
    std::fs::write(dir.join("BitCardChoose.cfg"), "INIT Init\nNEXT Next\nINVARIANT Inv\nCHECK_DEADLOCK FALSE\n").unwrap();
    let (code, plain) = run(&dir, "BitCardChoose.tla", &[], &[]);
    assert_eq!(code, 0, "{plain}");
    // TLC: 1920 states generated, 576 distinct states found, depth 2.
    assert_eq!(
        counts(&plain),
        ["1920 states generated, 576 distinct states found, 0 states left on queue.", "The depth of the complete state graph search is 2."]
    );
    let modes: [&[(&str, &str)]; 3] =
        [&[("TLX_NO_NATIVE", "1")], &[("TLX_NATIVE_SYNC", "1")], &[("TLX_PARANOID", "1"), ("TLX_NATIVE_SYNC", "1")]];
    for env in modes {
        let (code, out) = run(&dir, "BitCardChoose.tla", &["-compile"], env);
        assert_eq!((code, counts(&out)), (0, counts(&plain)), "{env:?}:\n{out}");
        assert!(!out.contains("fell back to the interpreter"), "{env:?}:\n{out}");
    }
}
