//! Region formation around nested native iteration (R9 widening,
//! `lower/regions.rs`).
//!
//! R9 replaces a binding construct whose element work is an interpreter
//! fallback by one fallback of the whole construct. A fallback that an
//! inner widening produced from a *loop* is not that: it is a whole loop
//! handed to the host, and folding the enclosing construct into it as well
//! discards the enclosing loop's native iteration for nothing.
//!
//! `NESTED` is IsConflictSerializable's shape — `UNION {f(x) : x \in S}`
//! over a LET-level RECURSIVE operator, which the lowering unrolls into a
//! tower of native unions with one fallback at the recursion limit. Before
//! the fix the widening cascaded from that fallback outward and the whole
//! invariant became one fallback, so every evaluation ran the recursive
//! graph search in the interpreter and no `UnionFold` node survived.
//!
//! The tests run on the closure IR executor and the generated C library
//! (`TLX_NO_NATIVE=1` / the default and `TLX_PARANOID` settings); the JIT
//! library is the same tests under
//! `TLX_JIT=1 cargo test --profile test-release -p tlx-compile --features tlx-testkit/jit`
//! (docs/testing.md §3).

use std::sync::Arc;

use tlx_compile::enc::EncModel;
use tlx_compile::ir::{Expr, ExprKind, PredFn};
use tlx_compile::settings::Settings;
use tlx_runtime::{explore, Options, Outcome};
use tlx_testkit::diff::Harness;

/// `UNION {reach(s, {}) : s \in start}` over a LET-level RECURSIVE
/// operator. The module-level form compiles to a native function call
/// instead (the compiled-function path), so the recursion must stay inside
/// a LET to reach the unrolled tower.
const NESTED: &str = r#"---- MODULE NestedUnion ----
EXTENDS Naturals, FiniteSets
VARIABLES edges, n
All ==
    LET RECURSIVE reach(_, _)
        reach(node, seen) ==
            IF node \in seen THEN {node}
            ELSE LET next == seen \union {node}
                     succ == {b : <<a, b>> \in {<<a, b>> \in edges : a = node}}
                 IN UNION {reach(b, next) : b \in succ}
        start == {a : <<a, b>> \in edges}
    IN UNION {reach(s, {}) : s \in start}
Init == edges = {<<1, 2>>, <<2, 1>>} /\ n = 0
Next == (n < 3 /\ n' = n + 1 /\ edges' = edges \cup {<<n + 1, n + 2>>})
             \/ UNCHANGED <<edges, n>>
Inv == All # {}
====
"#;

const NESTED_CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv\n";

/// `NESTED` with a failing element source: `1 \div (node - 1)` fails for
/// the start node 1, so the member `reach(1, {})` of the outer union
/// raises an error. The interpreter and the native union fold must report
/// it alike, from the same search prefix.
const NESTED_ERR: &str = r#"---- MODULE NestedUnionErr ----
EXTENDS Naturals, FiniteSets
VARIABLES edges, n
All ==
    LET RECURSIVE reach(_, _)
        reach(node, seen) ==
            IF node \in seen THEN {node}
            ELSE LET next == seen \union {node}
                     succ == {b : <<a, b>> \in {<<a, b>> \in edges : a = node} /\ 1 \div (node - 1) > 0}
                 IN UNION {reach(b, next) : b \in succ}
        start == {a : <<a, b>> \in edges}
    IN UNION {reach(s, {}) : s \in start}
Init == edges = {<<1, 2>>, <<2, 1>>} /\ n = 0
Next == (n < 3 /\ n' = n + 1 /\ edges' = edges \cup {<<n + 1, n + 2>>})
             \/ UNCHANGED <<edges, n>>
Inv == All # {}
====
"#;

/// A quantifier whose element work *is* the fallback: R9 must still fold it
/// into one fallback (the rule the fix narrows, not removes).
const GENUINE: &str = r#"---- MODULE GenuineWiden ----
EXTENDS Naturals, FiniteSets
VARIABLES x, p
S == 1..3
TC(R) == LET N == Cardinality(S)
             trcl[n \in Nat] == [a, b \in S |-> IF n = 0 THEN R[a, b]
                                               ELSE \/ trcl[n-1][a, b]
                                                    \/ \E c \in S : trcl[n-1][a, c] /\ trcl[n-1][c, b]]
         IN trcl[N]
Par == [a, b \in S |-> b = p[a]]
Init == x = 0 /\ p = <<1, 1, 2>>
Next == x < 3 /\ x' = x + 1 /\ p' = [p EXCEPT ![x + 1] = 1]
Reach == \A a \in S : TC(Par)[a, 1]
Nested == \A a \in S : \E b \in S : TC(Par)[a, b]
====
"#;

const GENUINE_CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Reach\nINVARIANT Nested\n";

/// How many nodes of `e` satisfy `f` (not looking inside fallbacks).
fn count(e: &Expr, f: &dyn Fn(&Expr) -> bool) -> usize {
    let mut n = f(e) as usize;
    if !matches!(&*e.kind, ExprKind::Interp { .. }) {
        e.for_each_child(&mut |c| n += count(c, f));
    }
    n
}

fn unions(p: &PredFn) -> usize {
    count(&p.body, &|e| matches!(&*e.kind, ExprKind::UnionFold { .. }))
}

fn interps(p: &PredFn) -> usize {
    count(&p.body, &|e| matches!(&*e.kind, ExprKind::Interp { .. }))
}

/// `p`'s body under the `Not`/`Convert`/`Eq` wrappers of `All # {}`: the
/// node the outer union lowers to.
fn guarded_root(p: &PredFn) -> &Expr {
    fn peel(e: &Expr) -> &Expr {
        match &*e.kind {
            ExprKind::Not(x) | ExprKind::Convert(x) => peel(x),
            ExprKind::Eq(a, _) => peel(a),
            _ => e,
        }
    }
    peel(&p.body)
}

/// Search `src` in the compiled tier with `env` and compare the whole
/// result with the interpreter's: counts, depth and outcome.
fn search(src: &str, cfg: &str, env: &[(&str, &str)]) -> (Outcome, tlx_runtime::Stats) {
    let h = Harness::new(src, cfg);
    let want = explore(&h.base, &Options::default());
    let settings = Settings::from_lookup(|n| env.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string())).expect("settings");
    let base = Arc::new(tlx_testkit::spec::load(src, cfg));
    let m = EncModel::new(base, h.layout.clone(), Vec::new(), &settings);
    let got = explore(&m, &Options::default());
    assert!(
        !m.notes.iter().any(|n| n.contains("interpreting model") || n.contains("IR verification failed")),
        "a whole-model fallback hides the lowering: {:?}",
        m.notes
    );
    assert_eq!(
        (got.stats.distinct, got.stats.generated, got.stats.depth),
        (want.stats.distinct, want.stats.generated, want.stats.depth),
        "{env:?}: counts"
    );
    assert_eq!(format!("{:?}", got.outcome), format!("{:?}", want.outcome), "{env:?}: outcome");
    (got.outcome, got.stats)
}

/// The tower of native unions survives: the invariant keeps `UnionFold`
/// nodes and the outer union is not one fallback (before the fix: no
/// `UnionFold` node at all, one fallback for the whole invariant).
#[test]
fn a_nested_loop_keeps_its_native_iteration() {
    let h = Harness::new(NESTED, NESTED_CFG);
    let p = &h.invariants[0];
    assert!(matches!(&*guarded_root(p).kind, ExprKind::UnionFold { .. }), "the outer union is a fallback again: {}", h.dump("Inv"));
    assert!(unions(p) >= 2, "the unrolled recursion is not native: {}", h.dump("Inv"));
    assert_eq!(interps(p), 1, "the recursion limit falls back once: {}", h.dump("Inv"));
}

/// The rule the fix narrows, not removes: a quantifier whose element work is
/// a fallback is still one fallback.
#[test]
fn a_dependent_fallback_still_widens() {
    let h = Harness::new(GENUINE, GENUINE_CFG);
    let p = &h.invariants[0];
    assert!(matches!(&*guarded_root(p).kind, ExprKind::Interp { .. }), "not widened: {}", h.dump("Reach"));
    assert_eq!(interps(p), 1, "{}", h.dump("Reach"));
}

/// A body that is one whole iteration has no element work of its own: the
/// host call the inner construct makes is the element's work, so the
/// enclosing loop folds too (the other half of the shield — without it a
/// loop of loops pays one host call per element).
#[test]
fn a_body_that_is_one_iteration_still_widens() {
    let h = Harness::new(GENUINE, GENUINE_CFG);
    let p = &h.invariants[1];
    assert!(matches!(&*guarded_root(p).kind, ExprKind::Interp { .. }), "not widened: {}", h.dump("Nested"));
    assert_eq!(interps(p), 1, "{}", h.dump("Nested"));
}

#[test]
fn nested_searches_match_the_interpreter() {
    for env in [
        &[("TLX_CC_DELAY_MS", "0"), ("TLX_MIXED_ADAPT", "0")][..],
        &[("TLX_CC_DELAY_MS", "0"), ("TLX_MIXED_ADAPT", "0"), ("TLX_PARANOID", "1")][..],
        &[("TLX_NO_NATIVE", "1")][..],
    ] {
        let (outcome, _) = search(NESTED, NESTED_CFG, env);
        assert!(matches!(outcome, Outcome::Ok), "{env:?}: {outcome:?}");
    }
}

#[test]
fn nested_errors_match_the_interpreter() {
    for env in [&[("TLX_CC_DELAY_MS", "0"), ("TLX_MIXED_ADAPT", "0")][..], &[("TLX_PARANOID", "1")][..]] {
        let (outcome, _) = search(NESTED_ERR, NESTED_CFG, env);
        assert!(matches!(outcome, Outcome::Error(_)), "{env:?}: the failing member is not reported: {outcome:?}");
    }
}
