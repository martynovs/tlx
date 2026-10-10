//! The value (sequence) operation family (`Slice`, `Index`, `ExceptIndex`,
//! `SeqLen`, `SeqIndex`, `SeqTail`, `SeqSub`, `SeqFilter`, `SeqConcat`,
//! `SeqAppend`, ...) after it moved out of the per-backend `encoded.rs`
//! dispatchers into `expr/sequences.rs` (cleanup batch A16).
//!
//! - `seq_family_agrees_across_tiers` drives one spec that lowers the whole
//!   family through the differential harness: the closure executor and the
//!   native tier (generated C, or the Cranelift library under `TLX_JIT=1`)
//!   must agree with the interpreter on every reachable state and on random
//!   states of the layout, including the empty-sequence, out-of-bounds and
//!   sub-sequence errors the family guards against;
//! - `seq_family_c_keeps_the_guards` inspects the generated C and pins the
//!   family's contracts: the empty-`Tail` guard, the 1-based `SeqIndex`
//!   bound, the `SubSeq` range check, the `max_len` capacity guards of
//!   `SeqConcat` / `SeqAppend`, whole-word `memmove` shifting and no
//!   per-operation FFI helper or host delegation.

use tlx_compile::cgen::CGen;
use tlx_runtime::Model;
use tlx_testkit::diff::{DiffReport, Harness};
use tlx_testkit::{module, seed, Rng};

const CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv1 Inv2\n";

/// The action names mark the family member and the failure mode they lower
/// to:
///
/// - `Push`/`Pop` are `Append`/`Tail` (`SeqAppend` / `SeqTail`);
/// - `PopEmpty` is `Tail` of the empty sequence (`SeqTail`'s guard);
/// - `Cat` is `\o` (`SeqConcat`), `Sub` is `SubSeq` (`SeqSub`);
/// - `SubErr` asks for a sub-sequence past the end (`SeqSub`'s guard);
/// - `Flt` is `SelectSeq` (`SeqFilter`);
/// - `Write` is an in-place `EXCEPT` on a sequence (`Slice` + `ExceptIndex`
///   + `Concat`);
/// - `Idxt` indexes a tuple (`Index`) and the sequence (`SeqIndex`);
/// - `IdxErr` indexes one past the end (`SeqIndex`'s bound);
/// - `Clear` empties the sequence.
const SEQ: &str = r#"EXTENDS Naturals, Sequences
VARIABLES q, t
Init == q = <<>> /\ t = <<0, 0, 0>>
Push == Len(q) < 3 /\ \E v \in 1..2 : q' = Append(q, v) /\ UNCHANGED t
Pop == Len(q) > 0 /\ q' = Tail(q) /\ UNCHANGED t
PopEmpty == Len(q) = 0 /\ q' = Tail(q) /\ UNCHANGED t
Cat == Len(q) <= 1 /\ q' = q \o q /\ UNCHANGED t
Sub == Len(q) >= 2 /\ q' = SubSeq(q, 2, Len(q)) /\ UNCHANGED t
SubErr == q' = SubSeq(q, 2, Len(q) + 1) /\ UNCHANGED t
Flt == q' = SelectSeq(q, LAMBDA e : e = 1) /\ UNCHANGED t
Write == \E i \in 1..Len(q) : q' = [q EXCEPT ![i] = IF @ = 1 THEN 2 ELSE 1] /\ UNCHANGED t
Idxt == \E i \in 1..Len(q) : t' = <<q[i], t[2], t[3]>> /\ UNCHANGED q
IdxErr == \E i \in 1..Len(q) + 1 : t' = <<q[i], t[2], t[3]>> /\ UNCHANGED q
Clear == q' = <<>> /\ t' = t
Next == Push \/ Pop \/ PopEmpty \/ Cat \/ Sub \/ SubErr \/ Flt \/ Write \/ Idxt \/ IdxErr \/ Clear
Inv1 == \A i \in 1..Len(q) : q[i] \in 1..2
Inv2 == \A i \in 1..Len(t) : t[i] \in 0..2
"#;

/// The whole family is lowered (so the differential run below really covers
/// it), the three tiers agree on every reachable state and on random states,
/// and the error paths (empty `Tail`, out-of-bounds index, `SubSeq` past the
/// end) are compared too.
#[test]
fn seq_family_agrees_across_tiers() {
    let h = Harness::new(&module("SeqFamily", SEQ), CFG);
    let kinds = h.node_kinds();
    for k in ["Slice", "Index", "ExceptIndex", "Concat", "SeqLen", "SeqIndex", "SeqTail", "SeqSub", "SeqFilter", "SeqConcat", "SeqAppend"] {
        assert!(kinds.contains(k), "the case does not lower {k}: {kinds:?}");
    }
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    assert!(h.not_native().is_empty(), "not native: {:?} (notes: {:?})", h.not_native(), h.notes);
    let mut rep: DiffReport = h.run_reachable(400);
    h.run_random(&mut Rng::new(seed(0xa16)), 300, &mut rep);
    rep.assert_ok("sequence family");
    let (exec, native) = rep.totals();
    assert!(exec.agree > 0 && native.agree > 0, "nothing compared\n{}", rep.render());
    // The empty-`Tail` / out-of-bounds index / `SubSeq` errors are reached
    // and both the interpreter and the compiled tiers report them.
    assert!(native.both_fail > 0 && exec.both_fail > 0, "no error path compared\n{}", rep.render());
    assert!(native.wrong == 0 && exec.wrong == 0, "{}", rep.render());
}

/// The generated C keeps the family's contracts and stays inline.
#[test]
fn seq_family_c_keeps_the_guards() {
    let src = module("SeqFamilyC", SEQ);
    let h = Harness::new(&src, CFG);
    let verified =
        tlx_compile::verify::Program::new(&h.typing, h.init.as_ref(), &h.actions, &h.invariants, &h.aprops).expect("valid lowered IR");
    let refops = tlx_compile::exec::refops::RefOpTable::of(&verified);
    let mut gen = CGen::new(&verified).with_refops(refops);
    let c = gen.library(h.base.action_names(), h.base.invariant_names()).src;

    // `SeqTail` of the empty sequence is an error, and the shift is a
    // whole-block `memmove`, not a per-element loop.
    assert!(c.contains("if (s_[0] == 0) return TLX_ERR;"), "no empty-Tail guard");
    assert!(c.contains("memmove("), "Tail does not shift the elements wordwise");
    // `SeqIndex` is 1-based and bounded by the length word.
    assert!(c.contains("(uint64_t)i_ > s_[0]) return TLX_ERR;"), "no SeqIndex bound");
    // `SubSeq` rejects a range past the end of the sequence.
    assert!(c.contains("n_ > (int64_t)s_[0]) return TLX_ERR;"), "no SubSeq range check");
    // `SeqConcat` / `SeqAppend` reject a result past the layout `max_len`.
    assert!(c.contains("a_[0] + b_[0] > "), "no SeqConcat capacity guard");
    assert!(c.contains("s_[0] >= "), "no SeqAppend capacity guard");
    // The 1-based `EXCEPT` on a sequence copies the block whole and only
    // rewrites an in-range slot (an out-of-range key leaves it unchanged,
    // as in TLC).
    assert!(c.contains("a_ = -1;"), "no in-place sequence EXCEPT slot mapping");
    // No per-operation FFI helper and no host delegation on the family's path.
    assert!(!c.contains("tlx_seq"), "a per-operation sequence FFI helper appeared");
    assert!(!c.contains("tlx_tail"), "a per-operation sequence FFI helper appeared");
    assert!(!c.contains("host->"), "the sequence family was sent to the host");
}

// ---------------------------------------------------------------------------
// `TLC!SortSeq` (a comparator that compiles)

/// A sort whose comparator is a lambda, a named operator or a *non-strict*
/// one, over a sequence the state builds, plus the `SetToSeq` of a set
/// state variable:
///
/// - `Push` sorts `Append(q, v)` with `LAMBDA a, b : a < b` (the usual
///   strict order);
/// - `Named` sorts `q` with the named operator `Lt`;
/// - `Tie` sorts pairs with `a[1] <= b[1]`: the comparator does not
///   distinguish the second component, so the result is *not* stable and
///   pinning it is what the interpreter's algorithm decides;
/// - `Div` uses a comparator that errors on two equal elements
///   (`a \div (a - b)`, division by zero);
/// - `Grow`/`FromSet` sort `SetToSeq(c)` of a set state variable;
/// - `Recs` sorts records with a *named* comparator reading a field
///   (`x.v < y.v`), the shape the CCF consistency specs use.
const SORT: &str = r#"EXTENDS Naturals, Sequences, SequencesExt, TLC
VARIABLES q, s, t, c, r, u
Lt(a, b) == a < b
RecLt(x, y) == x.v < y.v
Init == q = <<>> /\ s = <<>> /\ t = <<>> /\ c = {} /\ r = <<>> /\ u = <<>>
Push == \E v \in 1..3 : Len(q) < 3 /\ q' = SortSeq(Append(q, v), LAMBDA a, b : a < b) /\ UNCHANGED <<s, t, c, r, u>>
Named == s' = SortSeq(q, Lt) /\ UNCHANGED <<q, t, c, r, u>>
Tie == \E v \in 1..2 : Len(r) < 3 /\ r' = SortSeq(Append(r, <<v, 1>>), LAMBDA a, b : a[1] <= b[1]) /\ UNCHANGED <<q, s, t, c, u>>
Recs == \E v \in 1..2 : Len(u) < 3 /\ u' = SortSeq(Append(u, [v |-> v, w |-> 1]), RecLt) /\ UNCHANGED <<q, s, t, c, r>>
Grow == \E v \in 1..3 : v \notin c /\ c' = c \cup {v} /\ UNCHANGED <<q, s, t, r, u>>
FromSet == t' = SortSeq(SetToSeq(c), LAMBDA a, b : a < b) /\ UNCHANGED <<q, s, c, r, u>>
Div == t' = SortSeq(q, LAMBDA a, b : a \div (a - b) < 1) /\ UNCHANGED <<s, c, r, u>>
Next == Push \/ Named \/ Tie \/ Recs \/ Grow \/ FromSet \/ Div
Inv1 == Len(s) = Len(q)
Inv2 == \A i \in 1..Len(q) - 1 : q[i] <= q[i + 1]
"#;

const SORT_CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv1 Inv2\n";

/// The sorts are lowered to the two compiled functions and every tier
/// agrees with the interpreter on the reachable states and on random states
/// of the layout — including the comparator's division error and the
/// non-stable tie order above.
#[test]
fn sort_seq_agrees_across_tiers() {
    let h = Harness::new(&module("SortSeq", SORT), SORT_CFG);
    assert!(h.notes.is_empty(), "not lowered: {:?}", h.notes);
    assert!(h.not_native().is_empty(), "not native: {:?} (notes: {:?})", h.not_native(), h.notes);
    let names: Vec<&str> = h.typing.funcs.iter().map(|f| &*f.name).collect();
    for n in ["SortSeq.insert", "SortSeq.sort"] {
        assert!(names.contains(&n), "the sort's compiled function {n} is missing: {names:?}");
    }
    // No part of the spec goes back to the host: every sort (and the
    // `SetToSeq` whose result it sorts) is entirely compiled.
    assert!(h.node_kinds().contains("Collect"), "SetToSeq was not lowered to Collect: {:?}", h.node_kinds());
    assert!(!h.native.mixed_init, "Init has host regions");
    assert!(h.native.mixed_actions.iter().all(|m| !m), "an action has host regions: {:?}", h.native.mixed_actions);
    assert!(h.native.mixed_invariants.iter().all(|m| !m), "an invariant has host regions: {:?}", h.native.mixed_invariants);
    let mut rep: DiffReport = h.run_reachable(400);
    h.run_random(&mut Rng::new(seed(0xa16)), 300, &mut rep);
    rep.assert_ok("sort family");
    let (exec, native) = rep.totals();
    assert!(exec.agree > 0 && native.agree > 0, "nothing compared\n{}", rep.render());
    // The comparator's error is reached (equal neighbours) and both the
    // interpreter and the compiled tiers report a failure for it.
    assert!(native.both_fail > 0 && exec.both_fail > 0, "no comparator error compared\n{}", rep.render());
    assert!(native.wrong == 0 && exec.wrong == 0, "{}", rep.render());
}

/// The generated C sorts in place: the two functions call themselves and
/// each other, and nothing on the sort's path goes back to the host.
#[test]
fn sort_seq_c_recurses_without_host() {
    let src = module("SortSeqC", SORT);
    let h = Harness::new(&src, SORT_CFG);
    let verified =
        tlx_compile::verify::Program::new(&h.typing, h.init.as_ref(), &h.actions, &h.invariants, &h.aprops).expect("valid lowered IR");
    let refops = tlx_compile::exec::refops::RefOpTable::of(&verified);
    let mut gen = CGen::new(&verified).with_refops(refops);
    let c = gen.library(h.base.action_names(), h.base.invariant_names()).src;

    let insert = h.typing.funcs.iter().position(|f| &*f.name == "SortSeq.insert").expect("SortSeq.insert");
    let sort = h.typing.funcs.iter().position(|f| &*f.name == "SortSeq.sort").expect("SortSeq.sort");
    // Both compiled functions exist and are native C functions (a rejected
    // function is emitted as a stub returning TLX_UNSUPPORTED).
    for (i, n) in [(insert, "SortSeq.insert"), (sort, "SortSeq.sort")] {
        assert!(c.contains(&format!("fn_{i}(")), "the C library has no {n}");
    }
    // `sort` calls itself and `insert`; both are definitions, not stubs.
    let body = fn_body(&c, sort);
    assert!(body.contains(&format!("fn_{sort}(")), "sort does not recurse: {body}");
    assert!(body.contains(&format!("fn_{insert}(")), "sort does not call insert: {body}");
    // No host delegation and no per-operation FFI helper on the sort.
    assert!(!body.contains("host->"), "the sort was sent to the host");
    assert!(!c.contains("tlx_sort("), "a per-operation sort FFI helper appeared");
    assert!(!c.contains("tlx_sort_seq"), "a per-operation sort FFI helper appeared");
}

/// The C definition of compiled function `i` (the last occurrence: the
/// prototypes come first).
fn fn_body(c: &str, i: usize) -> String {
    let sig = format!("static int fn_{i}(");
    let start = c.rfind(&sig).expect("the function is defined");
    let rest = &c[start..];
    let end = rest[1..].find("\nstatic int fn_").map_or(rest.len(), |p| p + 1);
    rest[..end].to_string()
}
