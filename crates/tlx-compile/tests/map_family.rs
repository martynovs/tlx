//! The map operation family (`MapGet`, `MapPut`, `MapBuild`, `MapMerge`)
//! after it moved out of the per-backend `encoded.rs` dispatchers into
//! `expr/maps.rs` (cleanup batch A15).
//!
//! - `map_family_agrees_across_tiers` drives one spec that lowers all four
//!   node kinds through the differential harness: the closure executor and
//!   the native tier (generated C, or the Cranelift library under
//!   `TLX_JIT=1`) must agree with the interpreter on every reachable state
//!   and on random states of the layout;
//! - `map_family_c_is_inline_and_wordwise` inspects the generated C and
//!   pins the family's idioms: an inlined bit test for `MapGet`, whole-word
//!   `memcpy` moves for `MapPut` / `MapBuild` / `MapMerge`, the
//!   `__builtin_ctzll` worklist of `MapMerge`, the rank guards beside them
//!   and no per-word FFI helper.

use tlx_compile::cgen::CGen;

use tlx_layout::Shape;
use tlx_runtime::Model;
use tlx_testkit::diff::{DiffReport, Harness};
use tlx_testkit::{module, seed, Rng};

const CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv\n";

/// A map over the key universe `1..3`; the action names mark the family
/// member they lower to:
///
/// - `Get` reads a present or absent key (`MapGet`, an error when absent);
/// - `Ins` writes `k :> 1` (`MapPut` with the insert plan, `k >= 0` short
///   circuit);
/// - `Upd` is a key-constant `EXCEPT` (`MapPut` with the unchanged plan:
///   an absent key leaves the map alone, the empty map keeps rejecting
///   non-integer keys);
/// - `Rebuild` is a set map over a map domain (`MapBuild`);
/// - `Merge` is `m @@ [k \in 1..3 |-> 2]` (`MapMerge`).
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

/// The four kinds are all lowered (so the differential run below really
/// covers every family member) and the three tiers agree on every reachable
/// state and on random states.
#[test]
fn map_family_agrees_across_tiers() {
    let h = Harness::new(&module("MapFamily", MAPS), CFG);
    let kinds = h.node_kinds();
    for k in ["MapGet", "MapPut", "MapBuild", "MapMerge"] {
        assert!(kinds.contains(k), "the case does not lower {k}: {kinds:?}");
    }
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    assert!(h.not_native().is_empty(), "not native: {:?} (notes: {:?})", h.not_native(), h.notes);
    let mut rep: DiffReport = h.run_reachable(400);
    h.run_random(&mut Rng::new(seed(0xa15)), 300, &mut rep);
    rep.assert_ok("map family");
    let (exec, native) = rep.totals();
    assert!(exec.agree > 0 && native.agree > 0, "nothing compared\n{}", rep.render());
    // An absent key errors in the interpreter and in the compiled tiers
    // (which may bail out instead of reporting it themselves).
    assert!(native.wrong == 0 && exec.wrong == 0, "{}", rep.render());
}

/// The generated C inlines the family over whole words: no per-word FFI.
#[test]
fn map_family_c_is_inline_and_wordwise() {
    let src = module("MapFamilyC", MAPS);
    let h = Harness::new(&src, CFG);
    let verified =
        tlx_compile::verify::Program::new(&h.typing, h.init.as_ref(), &h.actions, &h.invariants, &h.aprops).expect("valid lowered IR");
    let refops = tlx_compile::exec::refops::RefOpTable::of(&verified);
    let mut gen = CGen::new(&verified).with_refops(refops);
    let c = gen.library(h.base.action_names(), h.base.invariant_names()).src;

    // MapGet: the bit test and the slot address are inline expressions.
    assert!(c.contains("tlx_bit("), "no inlined map bit test");
    // MapBuild zeroes the slot block and MapPut/MapBuild/MapMerge move the
    // entry words with memcpy.
    assert!(c.contains("memset("), "no zeroing of a built map");
    assert!(c.matches("memcpy(").count() >= 4, "the entries are not moved wordwise");
    // MapMerge walks the set bits with a builtin, not a runtime call.
    assert!(c.contains("__builtin_ctzll"), "MapMerge does not walk set bits inline");
    // No per-word map helper and no host delegation on the family's path.
    assert!(!c.contains("tlx_map"), "a per-word map FFI helper appeared");
    assert!(!c.contains("host->"), "the map family was sent to the host");
}

// ---------------------------------------------------------------------------
// `[i \in DOMAIN s |-> body]` into a sequence

/// A set map whose domain is the domain of an *encoded sequence* into a
/// sequence shape. `DOMAIN s` of a sequence is exactly `1..Len(s)`, so the
/// result is built slot by slot; the map-over-the-key-universe form and its
/// `Convert` back to the sequence shape (a host region) are not used.
///
/// The spec mirrors the CCF consistency specs, where the shape appears:
///
/// - `Field` is a named operator building `[i \in DOMAIN u |-> u[i]]` from
///   `SubSeq(s, 1, Len(s))` into a record's sequence field — the source is
///   computed, so its length is dynamic and the sequence is a *slice*;
/// - `Let` is the same construction behind a `LET`, whose binding is
///   evaluated once and shared with the domain;
/// - `Push`/`Pop` move the length, so the empty sequence, a partial fill and
///   a full one all occur; the slots past `Len(s)` must not be evaluated
///   (`s[i]` there is out of bounds in the interpreter);
/// - `Const` builds from the domain of a *constant* sequence, whose length
///   the existing constant-domain path already knows (the hook must not
///   change it);
/// - `Err` has a body that errors inside the domain, so error-versus-value
///   is compared too;
/// - `MapKeep` is a set map over a *map* domain: a map's domain has holes,
///   so it must keep the existing `MapBuild` path.
const MAPSEQ: &str = r#"EXTENDS Naturals, Sequences, TLC
VARIABLES s, q, r, m
LedgerObs(branch) == LET u == SubSeq(branch, 1, Len(branch)) IN [i \in DOMAIN u |-> u[i]]
Init == s = <<>> /\ q = <<>> /\ r = [obs |-> <<>>, tag |-> 0] /\ m = [k \in {} |-> 0]
Push == \E v \in 1..3 : Len(s) < 3 /\ s' = Append(s, v) /\ UNCHANGED <<q, r, m>>
Pop == Len(s) > 0 /\ s' = SubSeq(s, 1, Len(s) - 1) /\ UNCHANGED <<q, r, m>>
Field == r' = [obs |-> LedgerObs(s), tag |-> r.tag] /\ UNCHANGED <<s, q, m>>
Let == LET u == SubSeq(s, 2, Len(s)) IN q' = [i \in DOMAIN u |-> u[i]] /\ UNCHANGED <<s, r, m>>
Const == q' = [i \in DOMAIN <<1, 2>> |-> i] /\ UNCHANGED <<s, r, m>>
Err == \E v \in 1..2 : s' = Append(s, v) /\ r' = [obs |-> [i \in DOMAIN s' |-> (s')[i] \div ((s')[i] - (s')[i])], tag |-> r.tag] /\ UNCHANGED <<q, m>>
Grow == \E k \in 1..2 : k \notin DOMAIN m /\ m' = m @@ (k :> 0) /\ UNCHANGED <<s, q, r>>
MapKeep == m' = [k \in DOMAIN m |-> m[k] + 1] /\ UNCHANGED <<s, q, r>>
Next == Push \/ Pop \/ Field \/ Let \/ Const \/ Err \/ Grow \/ MapKeep
Inv1 == r.obs \in Seq(1..3)
Inv2 == \A i \in DOMAIN q : q[i] \in 1..3
Inv3 == \A k \in DOMAIN m : m[k] \in 0..2
"#;

const MAPSEQ_CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv1 Inv2 Inv3\n";

/// Every action is fully native (no host region for the `Convert`) and the
/// closure executor, the generated C and the interpreter agree on every
/// reachable state and on random states — including the states where the
/// sequence is shorter than the layout's `max_len`, where an unguarded slot
/// would read out of bounds, and the erroring bodies.
#[test]
fn seq_domain_set_map_is_native_across_tiers() {
    let h = Harness::new(&module("MapSeq", MAPSEQ), MAPSEQ_CFG);
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    assert!(h.not_native().is_empty(), "not native: {:?} (notes: {:?})", h.not_native(), h.notes);
    // The set map over a map's domain keeps the `MapBuild` path.
    assert!(h.node_kinds().contains("MapBuild"), "the map-domain set map is not lowered: {:?}", h.node_kinds());
    // No action has a host region: the `Convert` from the map shape to the
    // sequence shape is gone.
    assert!(!h.native.mixed_init, "Init has host regions");
    assert!(h.native.mixed_actions.iter().all(|m| !m), "an action has host regions: {:?}", h.native.mixed_actions);
    assert!(h.native.mixed_invariants.iter().all(|m| !m), "an invariant has host regions: {:?}", h.native.mixed_invariants);
    let mut rep: DiffReport = h.run_reachable(400);
    h.run_random(&mut Rng::new(seed(0xa17)), 300, &mut rep);
    rep.assert_ok("sequence domain set map");
    let (exec, native) = rep.totals();
    assert!(exec.agree > 0 && native.agree > 0, "nothing compared\n{}", rep.render());
    // The erroring body is reached: the interpreter and the compiled tiers
    // both report a failure (never a value).
    assert!(native.both_fail > 0 && exec.both_fail > 0, "no body error compared\n{}", rep.render());
    assert!(native.wrong == 0 && exec.wrong == 0, "{}", rep.render());
}

/// The generated C builds the sequence in place: the length word is the
/// source's `Len`, every slot past it is guarded, and nothing on the path
/// goes back to the host.
#[test]
fn seq_domain_set_map_c_is_inline() {
    let src = module("MapSeqC", MAPSEQ);
    let h = Harness::new(&src, MAPSEQ_CFG);
    let verified =
        tlx_compile::verify::Program::new(&h.typing, h.init.as_ref(), &h.actions, &h.invariants, &h.aprops).expect("valid lowered IR");
    let refops = tlx_compile::exec::refops::RefOpTable::of(&verified);
    let mut gen = CGen::new(&verified).with_refops(refops);
    let c = gen.library(h.base.action_names(), h.base.invariant_names()).src;

    // The `Field` action (a computed sequence source) is a real C definition
    // — a non-native one is emitted as a `return TLX_UNSUPPORTED;` stub — and
    // its slots are moved as whole words with no host region call
    // (`h->eval`, the mixed-mode call site of `cgen/region.rs`): the
    // `Convert` from the map shape to the sequence shape is gone.
    let field = h.base.action_names().iter().position(|n| n == "Field").expect("the Field action");
    let body = fn_body(&c, &format!("static int act_{field}("));
    assert!(!body.contains("TLX_UNSUPPORTED"), "the Field action is not native: {body}");
    assert!(body.contains("memcpy("), "the sequence slots are not moved wordwise: {body}");
    assert!(!body.contains("h->eval("), "the sequence domain set map was sent to the host: {body}");
    assert!(!c.contains("h->eval("), "a host region survived");
    assert!(!c.contains("host->"), "the sequence domain set map was sent to the host");
    assert!(!c.contains("tlx_conv"), "a conversion FFI helper appeared");
}

/// The C definition that starts with `sig` (the last occurrence: the
/// prototypes come first), as `seq_family.rs` reads the sort functions.
fn fn_body(c: &str, sig: &str) -> String {
    let start = c.rfind(sig).expect("the function is defined");
    let rest = &c[start..];
    let end = rest[1..].find("\nstatic int ").map_or(rest.len(), |p| p + 1);
    rest[..end].to_string()
}

/// The same construction into a *tuple* target. `seq_domain_len` also feeds
/// `fcn_into`'s tuple branch, which checks the built length against the
/// layout's arity; the source's length is dynamic, so the check is a run-time
/// one.
///
/// `s` keeps its length (a tuple value of another length is a layout miss,
/// which would end the run), and the shapes are pinned so the target really
/// is a tuple rather than a sequence of the same length.
const MAPTUPLE: &str = r#"EXTENDS Naturals, Sequences, TLC
VARIABLES s, t
Init == s = <<1, 2, 3>> /\ t = <<0, 0, 0>>
Rot == \E v \in 1..3 : s' = <<v, s[2], s[3]>> /\ t' = [i \in DOMAIN s |-> s[i]]
Shift == s' = <<s[2], s[3], s[1]>> /\ t' = [i \in DOMAIN s' |-> (s')[i]]
Next == Rot \/ Shift
Inv == t[1] \in 0..3 /\ t[2] \in 0..3 /\ t[3] \in 0..3
"#;

const MAPTUPLE_CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv\n";

#[test]
fn seq_domain_set_map_into_a_tuple_is_native_across_tiers() {
    use std::sync::Arc;
    let shapes = vec![
        Shape::Seq { elem: Box::new(Shape::Int { lo: 1, hi: 3 }), max_len: 3 },
        Shape::Tuple(Arc::from(vec![Shape::Int { lo: 0, hi: 3 }; 3])),
    ];
    let h = Harness::with_shapes(&module("MapTuple", MAPTUPLE), MAPTUPLE_CFG, shapes);
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    assert!(h.not_native().is_empty(), "not native: {:?} (notes: {:?})", h.not_native(), h.notes);
    assert!(h.native.mixed_actions.iter().all(|m| !m), "an action has host regions: {:?}", h.native.mixed_actions);
    let mut rep: DiffReport = h.run_reachable(400);
    h.run_random(&mut Rng::new(seed(0xa18)), 300, &mut rep);
    rep.assert_ok("sequence domain set map into a tuple");
    let (exec, native) = rep.totals();
    assert!(exec.agree > 0 && native.agree > 0, "nothing compared\n{}", rep.render());
    assert!(native.wrong == 0 && exec.wrong == 0, "{}", rep.render());
}
