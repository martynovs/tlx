//! `UNION { f(x) : x \in S }` when the members are sets of composite elements
//! whose universe is larger than the bitset universe (`MAX_BITSET_UNIVERSE`).
//!
//! A set of such elements is a sorted array, not a bitset, and its words are
//! not a set. The lowering folded the members with a word-wise or
//! (`ExprKind::UnionFold`, the bitset union), which turns a sorted array into
//! garbage: two members or two elements and the *elements* of the union are
//! the or of encodings. The regression is `ext/ccf/MCccfraft_1C3N`, whose
//! `TypeInv` reads `m.source`/`m.dest` of an element of `UNION {Range(messages[s])}`:
//! the corrupted element reported the invariant violated at depth 3 while TLC
//! and the interpreter complete it (docs/issues.md).
//!
//! - `union_of_ranges_over_a_large_universe_stays_sound` pins the fix: the
//!   invariant must not lower to the bitset union (the interpreter or a boxed
//!   `UNION` evaluates it), and every tier must agree with the interpreter on
//!   the reachable states, on random states of the layout and on the steps
//!   between them;
//! - `union_of_ranges_over_a_small_universe_keeps_the_bitset_fold` pins that
//!   the fold is *not* disabled where it is correct: with an atoms-only
//!   universe the member sets are bitsets, the union stays encoded as the or
//!   and native code, and the tiers agree.

use tlx_testkit::diff::Harness;
use tlx_testkit::{module, seed, Rng};

/// Records with three different field-name sets (three union arms), all
/// fields atoms or integers of one shared universe: the union of the ranges
/// is a set whose element universe is far above `MAX_BITSET_UNIVERSE`.
const LARGE: (&str, &str) = (
    r#"EXTENDS Integers, Sequences, Functions
CONSTANTS n1, n2, n3, TA, TB, TC
VARIABLES msgs, k
Init == msgs = [i \in {n1, n2, n3} |-> <<>>] /\ k = 0
Next ==
  /\ k < 3
  /\ k' = k + 1
  /\ \/ msgs' = [msgs EXCEPT ![n2] = Append(@, [dest |-> n2, source |-> n1, term |-> 1, type |-> TA])]
     \/ msgs' = [msgs EXCEPT ![n3] = Append(@, [dest |-> n3, source |-> n1, term |-> 1, type |-> TA])]
     \/ msgs' = [msgs EXCEPT ![n1] = Append(@, [dest |-> n1, isPreVote |-> TRUE, source |-> n2, term |-> 1, type |-> TB, voteGranted |-> FALSE])]
     \/ msgs' = [msgs EXCEPT ![n3] = Append(@, [dest |-> n3, lastCommittableIndex |-> 1, lastCommittableTerm |-> 1, source |-> n1, term |-> 1, type |-> TC])]
ManualMessages == UNION { Range(msgs[i]) : i \in {n1, n2, n3} }
Inv == \A m \in ManualMessages : m.dest = n1 \/ m.dest = n2 \/ m.dest = n3
"#,
    "CONSTANTS n1 = C_n1 n2 = C_n2 n3 = C_n3 TA = C_TA TB = C_TB TC = C_TC\nINIT Init\nNEXT Next\nINVARIANTS Inv\n",
);

/// Sets of atoms over a *non-constant* domain (`live`): the universe is
/// three, so the sets are bitsets and the union is the bitset or.
const SMALL: (&str, &str) = (
    r#"EXTENDS Integers, FiniteSets
CONSTANTS n1, n2, n3
VARIABLES seen, live, k
Init == seen = [i \in {n1, n2, n3} |-> {}] /\ live = {n1} /\ k = 0
Next ==
  /\ k < 3
  /\ k' = k + 1
  /\ \/ seen' = [seen EXCEPT ![n1] = @ \cup {n2}] /\ live' = live \cup {n2}
     \/ seen' = [seen EXCEPT ![n2] = @ \cup {n3}] /\ live' = live \cup {n3}
     \/ seen' = [seen EXCEPT ![n3] = @ \cup {n1, n2}] /\ live' = live
AllSeen == UNION { seen[i] : i \in live }
Inv == \A x \in AllSeen : x \in {n1, n2, n3}
"#,
    "CONSTANTS n1 = C_n1 n2 = C_n2 n3 = C_n3\nINIT Init\nNEXT Next\nINVARIANTS Inv\n",
);

#[test]
fn union_of_ranges_over_a_large_universe_stays_sound() {
    let (src, cfg) = LARGE;
    let h = Harness::new(&module("UnionOfRangesLarge", src), cfg);
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    let read = h.dump("Inv");
    assert!(
        !read.contains("UnionFold") && !read.contains("union t"),
        "a union over a sorted-array set is folded with a word-wise or:\n{read}"
    );
    let mut report = h.run_reachable(200);
    h.run_random(&mut Rng::new(seed(0x0ccf)), 200, &mut report);
    report.assert_ok("union over a large element universe");
    let (exec, native) = report.totals();
    assert!(exec.agree > 0 && native.agree > 0, "nothing compared\n{}", report.render());
    assert_eq!(exec.wrong + native.wrong, 0, "{}", report.render());
}

#[test]
fn union_of_ranges_over_a_small_universe_keeps_the_bitset_fold() {
    let (src, cfg) = SMALL;
    let h = Harness::new(&module("UnionOfRangesSmall", src), cfg);
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    let read = h.dump("Inv");
    assert!(read.contains("union t"), "the bitset union was disabled where it is valid:\n{read}");
    let mut report = h.run_reachable(200);
    h.run_random(&mut Rng::new(seed(0x0ccf)), 200, &mut report);
    report.assert_ok("union over a small element universe");
    let (exec, native) = report.totals();
    assert!(exec.agree > 0 && native.agree > 0, "nothing compared\n{}", report.render());
    assert_eq!(exec.wrong + native.wrong, 0, "{}", report.render());
}
