//! Differential tests for the field-level comparability of a set whose
//! elements are records from several union arms, each arm holding typed model
//! values of one type (CCF's `{history[i] : i \in RwTxResponseCommittedEventIndexes}`),
//! and for the iteration order of an encoded set of sequences.
//!
//! The element shape of the CCF set is a union of records whose `type` (and
//! `status`) fields hold model values: TLC refuses to compare two typed model
//! values of different types, so the layout keeps each arm's field universe to
//! one type (see `tlx-layout`'s `extend_atoms`) and the set is encoded instead
//! of staying in the interpreter. Its consumers are the reachable ones:
//! `SetToSeq` iterates the encoded set in TLC order (the ordering plan covers
//! sequences too, see `encoded_order`'s `order_safe`) and `SortSeq` sorts it
//! with the lowered comparator.
//!
//! The interpreter is the oracle for every state. The invariants below are
//! functions of the sorted sequence, of the set's iteration order and of a
//! comparison TLC refuses to make, so a wrong order, a missing error or a
//! dropped member shows up as a differing verdict.

use tlx_testkit::diff::Harness;

/// CCF's history: three record arms, `type` of one model-value type per arm,
/// one arm holding a sequence, and a set built from the indexes of one arm.
/// `obs` is a set of sequences of different lengths, whose TLC order
/// (lengths first, then the elements) differs from the lexicographic one.
const SPEC: &str = r#"---- MODULE FieldComparability ----
EXTENDS Integers, Sequences, SequencesExt, FiniteSets, TLC

CONSTANT T_a, T_b, S_c

VARIABLES history, obs

Limit == 3

TxID(v, s) == <<v, s>>
View(t) == t[1]
SeqNum(t) == t[2]

Observed(i) == [j \in 1..i |-> j]
Shorty == <<5>>
Longy == <<1, 1>>
Thrice == <<2, 2, 2>>

TxIDStrictlyLessThan(x, y) ==
    \/ View(x) < View(y)
    \/ /\ View(x) = View(y)
       /\ SeqNum(x) < SeqNum(y)

Neverless(x, y) == FALSE

A(i) == [tx |-> i, type |-> T_a]
B(i) == [type |-> T_b, tx_id |-> TxID(0, i), status |-> S_c]
C(i) == [tx |-> i, type |-> T_a, observed |-> Observed(i), tx_id |-> TxID(0, i)]

Init == history = <<>> /\ obs = {}

Next == \/ Len(history) < Limit /\ history' = Append(history, A(Len(history))) /\ UNCHANGED obs
        \/ Len(history) < Limit /\ history' = Append(history, B(Len(history))) /\ UNCHANGED obs
        \/ Len(history) < Limit /\ history' = Append(history, C(Len(history))) /\ UNCHANGED obs
        \/ obs' = obs \cup {Shorty} /\ UNCHANGED history
        \/ obs' = obs \cup {Longy} /\ UNCHANGED history
        \/ obs' = obs \cup {Thrice} /\ UNCHANGED history

Indexes == {i \in DOMAIN history : history[i].type = T_b}
S == {history[i] : i \in Indexes}
Sorted == SortSeq(SetToSeq(S), LAMBDA x, y : TxIDStrictlyLessThan(x.tx_id, y.tx_id))

\* The members, the size, and the order the sort puts them in.
Inv == /\ Len(Sorted) = Cardinality(S)
       /\ \A i \in 1..Len(Sorted) : Sorted[i] \in S
       /\ \A i \in 1..Len(Sorted)-1 : SeqNum(Sorted[i].tx_id) < SeqNum(Sorted[i+1].tx_id)

\* A comparator that never orders makes the sort stable, so the result is the
\* set's iteration order: the first of the sequences is the shortest only when
\* the iteration is TLC's (lengths first; lexicographic would put <<1,1>>
\* before <<5>>).
ObsInv == LET seq == SortSeq(SetToSeq(obs), LAMBDA x, y : Neverless(x, y)) IN
          /\ Len(seq) = Cardinality(obs)
          /\ \A i \in 1..Len(seq)-1 : Len(seq[i]) <= Len(seq[i+1])

\* A field one of the arms does not have: TLC fails while the set map is
\* built, and the compiled tiers must fail too, not answer a value.
ErrInv == Cardinality({history[i].tx_id : i \in DOMAIN history}) >= 0
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv\nINVARIANT ObsInv\nINVARIANT ErrInv\n\
CHECK_DEADLOCK FALSE\nCONSTANT T_a = T_a\nCONSTANT T_b = T_b\nCONSTANT S_c = S_c\n";

/// The layout settles on the shapes above from the reachable states.
fn harness() -> Harness {
    Harness::new(SPEC, CFG)
}

/// The set of union records is encoded and read natively: every state agrees
/// with the interpreter, and no state is answered wrongly.
#[test]
fn union_of_records_with_typed_model_values_is_encoded() {
    let h = harness();
    let report = h.run_reachable(400);
    report.assert_ok(SPEC);
    let (exec, native) = report.tallies.get("invariant Inv").expect("invariant Inv evaluated");
    assert!(exec.agree > 0, "{}", report.render());
    assert_eq!(native.wrong + native.unsupported, 0, "{}", report.render());
    assert!(native.agree > 0, "{}", report.render());
}

/// `SetToSeq` of an encoded set of sequences of different lengths iterates in
/// TLC's order: the stable sort above reproduces it exactly.
#[test]
fn set_of_sequences_iterates_in_tlc_order() {
    let h = harness();
    let report = h.run_reachable(400);
    report.assert_ok(SPEC);
    let (exec, native) = report.tallies.get("invariant ObsInv").expect("invariant ObsInv evaluated");
    assert!(exec.agree > 0, "{}", report.render());
    assert_eq!(native.wrong + native.unsupported, 0, "{}", report.render());
    assert!(native.agree > 0, "{}", report.render());
}

/// An error stays an error: reading a field an arm of the union does not have
/// fails while the set is built, in the interpreter and in the compiled tiers.
#[test]
fn a_missing_field_still_fails_in_a_set_map() {
    let h = harness();
    let report = h.run_reachable(400);
    report.assert_ok(SPEC);
    let (exec, native) = report.tallies.get("invariant ErrInv").expect("invariant ErrInv evaluated");
    assert!(exec.both_fail > 0, "{}", report.render());
    assert!(native.both_fail + native.unsupported > 0, "{}", report.render());
}
