//! `UNION {f(x) : x \in S}` over a runtime `S` (`UnionFold`) and the
//! cartesian product `A \X B` of two encoded sets, both lowered in
//! `lower/sets.rs`.
//!
//! TLC builds the set `{f(x) : x \in S}` before taking its union, so every
//! member is evaluated and an error in any of them is reported even when the
//! union would not need it; the folded lowering runs one loop over `S` in
//! enumeration order, evaluating each member once.
//!
//! - `union_fold_agrees_across_tiers` drives a spec whose invariants union
//!   singletons, two-element sets, filtered sets and a set of a set's own
//!   elements: the closure executor and the native tier (generated C, or the
//!   Cranelift library under `TLX_JIT=1`) must agree with the interpreter on
//!   every reachable and random state;
//! - `union_fold_evaluates_every_member` is the error-versus-value case: one
//!   member divides by zero on the state where the map is not empty, so a
//!   lowering that skipped a member (or that computed a value where TLC
//!   reports an error) would disagree;
//! - `cartesian_product_agrees_across_tiers` covers `\X` over encoded sets:
//!   membership, cardinality, a filter over the product and a nested
//!   quantifier over it, including the empty operand.

use tlx_testkit::diff::{DiffReport, Harness};
use tlx_testkit::{module, seed, Rng};

/// `S` grows, shifts and filters; the invariants union a map of it in four
/// shapes, so the fold is exercised with members of one and of two elements,
/// with a member computed by a filter and with a member enumerated from
/// another set.
const UNIONS: &str = r#"EXTENDS Naturals, FiniteSets
VARIABLES S, T
Init == S = {} /\ T = {1, 2}
Grow == \E i \in 1..4 : i \notin S /\ S' = S \cup {i} /\ T' = T
Shift == S' = {i - 1 : i \in S} /\ T' = T
Filt == S' = {j \in T : j > 1} /\ T' = T \cup {1}
Next == Grow \/ Shift \/ Filt
Inv1 == UNION { {i} : i \in S } = S
Inv2 == UNION { {i, i + 1} : i \in S } \subseteq 0..5
Inv3 == UNION { {j \in T : j > i} : i \in S } \subseteq T
Inv4 == \A i \in S : i \in UNION { {j} : j \in T }
Inv5 == Cardinality(UNION { {i} : i \in S }) <= 4
Inv6 == UNION { {i} : i \in T } \cup S = S \cup T
"#;
const UNIONS_CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv1 Inv2 Inv3 Inv4 Inv5 Inv6\n";

/// `S` starts at `{1}` and gains `2`; the member `5 \div (y - 2)` is a value
/// for `y = 1` and an error for `y = 2`, which TLC reaches because it
/// evaluates every member of the set map before the union.
const MEMBER_ERROR: &str = r#"EXTENDS Naturals, FiniteSets
VARIABLES S
Init == S = {1}
Add == S' = S \cup {2}
Drop == S' = {}
Next == Add \/ Drop
Inv == UNION { {1} \cup {5 \div (y - 2)} : y \in S } = {}
"#;
const MEMBER_ERROR_CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv\n";

/// `A` and `B` are sets of small integers over which the product is built;
/// the invariants take its membership, its cardinality, a filter over it and
/// a doubly quantified membership, and `Inv6` reaches the empty product.
const PRODUCTS: &str = r#"EXTENDS Naturals, FiniteSets
VARIABLES A, B
Init == A = {} /\ B = {1, 2}
Grow == \E i \in 1..3 : i \notin A /\ A' = A \cup {i} /\ B' = B
Shrink == A' = A /\ B' = {j \in B : j > 1}
Swap == A' = B /\ B' = A
Next == Grow \/ Shrink \/ Swap
Inv1 == (A \X B) = (A \X B)
Inv2 == Cardinality(A \X B) = Cardinality(A) * Cardinality(B)
Inv3 == \A p \in A \X B : p[1] \in A /\ p[2] \in B
Inv4 == \A a \in A : \A b \in B : <<a, b>> \in A \X B
Inv5 == {p \in A \X B : p[1] = p[2]} \subseteq A \X B
Inv6 == (A = {} \/ B = {}) => (A \X B) = {}
"#;
const PRODUCTS_CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv1 Inv2 Inv3 Inv4 Inv5 Inv6\n";

/// The fold is lowered and every tier agrees on every reachable and random
/// state; no invariant falls back to the interpreter.
#[test]
fn union_fold_agrees_across_tiers() {
    let h = Harness::new(&module("UnionFold", UNIONS), UNIONS_CFG);
    let kinds = h.node_kinds();
    assert!(kinds.contains("UnionFold"), "the case does not lower UnionFold: {kinds:?}");
    assert!(kinds.contains("MapSet"), "the case does not lower MapSet: {kinds:?}");
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    assert!(h.not_native().is_empty(), "not native: {:?} (notes: {:?})", h.not_native(), h.notes);
    let mut rep: DiffReport = h.run_reachable(400);
    h.run_random(&mut Rng::new(seed(0x0f01)), 300, &mut rep);
    rep.assert_ok("union fold");
    let (exec, native) = rep.totals();
    assert!(exec.agree > 0 && native.agree > 0, "nothing compared\n{}", rep.render());
    assert_eq!(native.unsupported, 0, "{}", rep.render());
    assert_eq!(exec.wrong + native.wrong, 0, "{}", rep.render());
}

/// The member that divides by zero is reached, and every tier reports an
/// error for the states where `2 \in S` (never a value).
#[test]
fn union_fold_evaluates_every_member() {
    let h = Harness::new(&module("UnionFoldMemberError", MEMBER_ERROR), MEMBER_ERROR_CFG);
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    let mut rep: DiffReport = h.run_reachable(20);
    h.run_random(&mut Rng::new(seed(0x0f02)), 50, &mut rep);
    rep.assert_ok("union fold member error");
    let (exec, native) = rep.totals();
    assert!(exec.both_fail > 0 || native.both_fail > 0, "the error path was not reached\n{}", rep.render());
    assert_eq!(exec.wrong + native.wrong, 0, "a tier disagreed with the interpreter\n{}", rep.render());
}

/// The product is lowered and every tier agrees, including on the empty
/// operands and on the states where the elements do not fit the other side's
/// universe.
#[test]
fn cartesian_product_agrees_across_tiers() {
    let h = Harness::new(&module("CartesianProduct", PRODUCTS), PRODUCTS_CFG);
    let kinds = h.node_kinds();
    assert!(kinds.contains("Filter"), "the case does not lower Filter: {kinds:?}");
    assert!(kinds.contains("BitMember"), "the case does not lower BitMember: {kinds:?}");
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    assert!(h.not_native().is_empty(), "not native: {:?} (notes: {:?})", h.not_native(), h.notes);
    let mut rep: DiffReport = h.run_reachable(400);
    h.run_random(&mut Rng::new(seed(0x0f03)), 300, &mut rep);
    rep.assert_ok("cartesian product");
    let (exec, native) = rep.totals();
    assert!(exec.agree > 0 && native.agree > 0, "nothing compared\n{}", rep.render());
    assert_eq!(native.unsupported, 0, "{}", rep.render());
    assert_eq!(exec.wrong + native.wrong, 0, "{}", rep.render());
}

/// Neither lowering delegates to the host: the generated C of both cases
/// reaches the host interpreter for nothing on their paths.
#[test]
fn the_new_lowerings_do_not_delegate_to_the_host() {
    for (name, src, cfg) in [("UnionFoldC", UNIONS, UNIONS_CFG), ("CartesianProductC", PRODUCTS, PRODUCTS_CFG)] {
        let h = Harness::new(&module(name, src), cfg);
        assert!(h.c_src.contains("tlx_"), "{name}: the generated C is empty");
        let callbacks = tlx_testkit::diff::host_callbacks(&h.c_src);
        let delegated: Vec<&String> =
            callbacks.iter().filter(|c| tlx_testkit::diff::HOST_OPERATION_CALLBACKS.contains(&c.as_str())).collect();
        assert!(delegated.is_empty(), "{name} delegates to the host interpreter: {delegated:?} (all callbacks: {callbacks:?})");
    }
}
