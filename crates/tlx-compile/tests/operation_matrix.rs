//! The operation-consumer matrix: which execution consumer runs each
//! operation family a case lowers to (docs/development-standards.md CODE-19,
//! ARCH-06).
//!
//! A model that reaches a verdict through the compiled pipeline still says
//! nothing about *which* tier ran each of its operations: a successful run
//! can delegate every operation and conceal an untested backend or fast
//! path. This matrix names, per operation family, the consumer that ran it
//! (`ran`), the consumer an operation was handed to (`delegated`, never
//! counted as execution) or why a consumer has no evidence (`gap`). It is
//! built by `Harness::consumers` from the lowered IR, the generated C source
//! and the native plan.
//!
//! Rows cover the record/reference operations (`ref`), maps, sets,
//! composition (`compose`, which the native tiers refuse) and
//! boxed/interpreted values (`dyn`, a whole-model fallback). `map-slice` is
//! a negative control: a lookup on a fixed-domain function lowers to
//! `Slice`, not to the map kinds, so the map markers report gaps - the
//! matrix must not manufacture coverage for a family the case never lowers.

use std::collections::BTreeSet;

use tlx_layout::store::StoreRef;
use tlx_layout::Shape;
use tlx_testkit::diff::{assert_host_classification_exercised, Backend, Consumer, FamilyConsumers, Harness};
use tlx_testkit::{module, note};

const CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv\n";

/// A bag of records with an EXCEPT update and a `@@` merge: the encoded
/// store's reference operations.
const BAG: &str = r#"EXTENDS Naturals, Sequences, TLC
VARIABLES bag, n
K(i) == [id |-> i % 3, log |-> <<i, 100 * i>>]
Init == bag = (K(0) :> 0) /\ n = 0
Next == n < 4 /\ \E k \in DOMAIN bag :
          /\ bag' = [bag EXCEPT ![k] = @ + 1] @@ (K(n + 1) :> n)
          /\ n' = n + 1
Inv == \A k \in DOMAIN bag : bag[k] >= 0
"#;

/// Maps whose domain is not a fixed range: the encoded map operations.
const MAPS: &str = r#"EXTENDS Naturals, TLC
VARIABLE m
Init == m = [k \in {} |-> 0]
Get == \E k \in 1..3 : m[k] = 0 /\ UNCHANGED m
Ins == \E k \in 1..3 : k \notin DOMAIN m /\ m' = m @@ (k :> 1)
Upd == \E k \in 1..3 : m' = [m EXCEPT ![k] = 1]
Rebuild == \E k \in 1..3 : m' = [j \in DOMAIN m \ {k} |-> m[j]]
Merge == m' = m @@ [k \in 1..3 |-> 2]
Next == Get \/ Ins \/ Upd \/ Rebuild \/ Merge
Inv == \A k \in DOMAIN m : m[k] \in 0..1
"#;

/// Small integer sets: the encoded bitset operations.
const BITSETS: &str = r#"EXTENDS Naturals, FiniteSets
VARIABLES S, T
Init == S = {} /\ T = {1, 2}
Add == \E i \in 1..4 : i \notin S /\ S' = S \cup {i} /\ T' = T
Rem == \E i \in S : S' = S \ {i} /\ T' = T \cup {i}
Mix == S' = S \cap T /\ T' = (T \ S) \cup {Cardinality(S)}
Filt == S' = {i \in T : i > 1} /\ T' = {i + 1 : i \in S \cap 0..3}
Next == Add \/ Rem \/ Mix \/ Filt
Inv == Cardinality(S) <= 4 /\ \A i \in S : i \in 0..5
"#;

/// `A \cdot B`: the composition the C and JIT tiers delegate to the closure
/// executor (docs/design.md "Action composition").
const COMPOSED: &str = r#"EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
A == x < 2 /\ x' = x + 1
B == x' = x /\ y' = x
Step == (A \cdot B) /\ y' = x + 1
Reset == x = 2 /\ x' = 0 /\ y' = 0
Next == Step \/ Reset
Inv == x + y >= 0
"#;

/// A lookup on a function with a fixed domain: lowers to `Slice` on the
/// encoded function, not to the map kinds (the negative control).
const MAPCALL: &str = r#"EXTENDS Naturals, TLC
VARIABLES m, n
G(k) == m[k]
Init == m = [k \in 1..3 |-> 0] /\ n = 0
Next == n < 2 /\ G(1) = 0 /\ m' = m /\ n' = n + 1
Inv == \A k \in DOMAIN m : m[k] >= 0
"#;

const REF_MARKERS: &[&str] = &["RefOp", "RefElems"];
/// The host callbacks the generated C calls for the store reference
/// operations.
const REF_DELEGATED: &[&str] = &["h->ref_op", "h->ref_elems"];
const MAP_MARKERS: &[&str] = &["MapGet", "MapPut", "MapBuild", "MapMerge"];
const SET_MARKERS: &[&str] = &["BitMember", "BitSet", "BitSetOp", "BitCard", "Filter", "MapSet"];
/// Boxed/interpreted value kinds: the interpreter fallback inside the
/// compiled tiers.
const BOXED_MARKERS: &[&str] = &["ValOp", "Apply", "Interp", "ValField"];
const COMPOSE_MARKERS: &[&str] = &["Compose"];

/// The native consumer this configuration built (the C library, or the JIT
/// under `TLX_JIT=1` with the `jit` feature).
fn native(backend: Backend) -> Consumer {
    match backend {
        Backend::C => Consumer::C,
        Backend::Jit => Consumer::Jit,
    }
}

struct Case {
    name: &'static str,
    harness: Harness,
    markers: &'static [&'static str],
    delegated: &'static [&'static str],
}

impl Case {
    fn row(&self) -> FamilyConsumers {
        self.harness.consumers(self.name, self.markers, self.delegated)
    }
}

fn cases() -> Vec<Case> {
    // `shapes: None` speculates the layout from the reachable states
    // (`Harness::new`); an explicit shape list is only needed for a case
    // whose layout the reachable states do not settle (the encoded store
    // reference, composition, the interpreter fallback).
    let mk = |name: &'static str,
              src: &str,
              shapes: Option<Vec<Shape>>,
              markers: &'static [&'static str],
              delegated: &'static [&'static str]| {
        let src = module(if name == "map-slice" { "MapCall" } else { name }, src);
        let harness = match shapes {
            Some(shapes) => Harness::with_shapes(&src, CFG, shapes),
            None => Harness::new(&src, CFG),
        };
        Case { name, harness, markers, delegated }
    };
    vec![
        mk("ref", BAG, Some(vec![Shape::Ref(StoreRef::new(1 << 20)), Shape::Int { lo: 0, hi: 9 }]), REF_MARKERS, REF_DELEGATED),
        mk("map", MAPS, None, MAP_MARKERS, &[]),
        mk("map-slice", MAPCALL, None, MAP_MARKERS, &[]),
        mk("set", BITSETS, None, SET_MARKERS, &[]),
        mk("compose", COMPOSED, Some(vec![Shape::Int { lo: 0, hi: 4 }; 2]), COMPOSE_MARKERS, &[]),
        mk("dyn", BAG, Some(vec![Shape::Dyn, Shape::Int { lo: 0, hi: 9 }]), BOXED_MARKERS, &[]),
    ]
}

fn row_of<'a>(rows: &'a [FamilyConsumers], name: &str) -> &'a FamilyConsumers {
    rows.iter().find(|r| r.family == name).unwrap_or_else(|| panic!("no row {name}"))
}

/// Every cell of every row is exactly one kind of evidence (execution,
/// delegation or a named gap), the interpreter tiers and the closure
/// executor run the families they lower, and the host classification is
/// exercised rather than vacuously true.
///
/// The executed/delegated cells per family pin the shape of the matrix, so a
/// tier that silently stops running a family fails here instead of passing
/// with a smaller count (docs/development-standards.md CODE-19).
#[test]
fn every_family_reaches_a_named_consumer_or_an_explicit_gap() {
    let cases = cases();
    let rows: Vec<FamilyConsumers> = cases.iter().map(Case::row).collect();
    let mut out = String::new();
    for (c, row) in cases.iter().zip(&rows) {
        row.assert_ok();
        let executed = Consumer::ALL.iter().filter(|k| row.cell(**k).is_executed()).count();
        assert!(executed > 0, "{}: no consumer ran the family", c.name);
        out.push_str(&row.render());
        out.push('\n');
    }

    // The interpreter tiers (tree and the pre-resolved code) and the closure
    // executor lower and run every one of these families.
    for name in ["ref", "map", "set", "compose", "dyn"] {
        let row = row_of(&rows, name);
        row.assert_lowered();
        row.assert_executed(Consumer::Closure);
    }

    // ref: the native backend hands the store operations to a host callback.
    let r = cases.iter().find(|c| c.name == "ref").unwrap();
    let ref_row = row_of(&rows, "ref");
    ref_row.assert_delegated(native(r.harness.backend));
    ref_row.assert_executed(Consumer::Host);
    ref_row.assert_gap(Consumer::Unsupported);

    // map and set run inline in the native backend, with no host callback.
    for name in ["map", "set"] {
        let r = cases.iter().find(|c| c.name == name).unwrap();
        let row = row_of(&rows, name);
        row.assert_executed(native(r.harness.backend));
        row.assert_gap(Consumer::Host);
        row.assert_gap(Consumer::Unsupported);
    }

    // compose and dyn are refused by the native tiers (Compose and the boxed
    // value kinds), so the host executor keeps them.
    for name in ["compose", "dyn"] {
        let r = cases.iter().find(|c| c.name == name).unwrap();
        let row = row_of(&rows, name);
        row.assert_gap(native(r.harness.backend));
        row.assert_executed(Consumer::Host);
        row.assert_executed(Consumer::Unsupported);
    }

    // The negative control: the map markers are absent because the case
    // lowers its lookup to `Slice`, so the map cells are gaps.
    let slice_case = cases.iter().find(|c| c.name == "map-slice").unwrap();
    let slice = row_of(&rows, "map-slice");
    slice.assert_gap(Consumer::Tree);
    slice.assert_gap(Consumer::Closure);
    slice.assert_gap(native(slice_case.harness.backend));

    assert_host_classification_exercised(&rows);
    note(&out);
}

/// Delegation is not execution: `ref` hands its store operations to the host
/// callback `h->ref_op`, so the native cell is a delegation and the matrix
/// refuses to count it as the native backend running the family.
#[test]
#[should_panic(expected = "expected execution")]
fn a_delegated_operation_is_not_native_execution() {
    let c = cases().into_iter().find(|c| c.name == "ref").unwrap();
    c.row().assert_ok().assert_delegated(native(c.harness.backend)).assert_executed(native(c.harness.backend));
}

/// A whole-model fallback is not native execution: `dyn` lowers none of its
/// functions to the native tiers, so claiming the native backend ran the
/// boxed family must fail.
#[test]
#[should_panic(expected = "expected execution")]
fn a_whole_model_fallback_is_not_native_execution() {
    let c = cases().into_iter().find(|c| c.name == "dyn").unwrap();
    c.row().assert_ok().assert_executed(native(c.harness.backend));
}

/// A family the case never lowers reads as a gap, not as coverage: passing
/// the map markers to a case whose lookup lowers to `Slice` must not claim
/// the interpreter ran the map family.
#[test]
#[should_panic(expected = "expected execution")]
fn a_family_the_case_never_lowers_is_a_gap() {
    let c = cases().into_iter().find(|c| c.name == "map-slice").unwrap();
    c.row().assert_ok().assert_executed(Consumer::Tree);
}

/// The matrix must not pass vacuously: rows that never reach the host are
/// refused, so the delegation rules above are always exercised by some row.
#[test]
#[should_panic(expected = "no family in the matrix runs on the host")]
fn the_host_classification_must_be_exercised() {
    let c = cases().into_iter().find(|c| c.name == "map").unwrap();
    assert_host_classification_exercised(&[c.row()]);
}

/// Kinds no case here lowers, recorded so the gap is explicit rather than a
/// silent zero (CODE-19). The boxed record field access and tuple kinds are
/// reached only through the whole-model fallback this matrix exercises with
/// `dyn`; if a case starts lowering them, this fails and the matrix grows.
#[test]
fn unreached_kinds_are_recorded_gaps() {
    let mut kinds: BTreeSet<&'static str> = BTreeSet::new();
    for c in cases() {
        kinds.extend(c.harness.node_kinds());
    }
    let gaps = ["ValField", "Tuple"];
    let reached: Vec<&str> = gaps.iter().copied().filter(|k| kinds.contains(k)).collect();
    assert!(reached.is_empty(), "these kinds are now lowered by a case: add them to a family row instead of leaving a gap: {reached:?}");
}
