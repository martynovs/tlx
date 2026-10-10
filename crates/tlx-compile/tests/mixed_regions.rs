//! Mixed mode, host side: lowered IR evaluated as host regions
//! (`enc::mixed::HostRegions`, i.e. the IR executor behind `host->eval`)
//! from a hand-written C library, against the interpreter. Until the code
//! generator emits regions, the region table is made by hand from the
//! lowered predicates plus a few IR expressions on boxed values.

use std::collections::BTreeSet;
use std::sync::Arc;

use tlx_compile::enc::mixed::{FastRegions, HostRegions, Reads, Region, RegionCtx, RegionInfo, RegionTable};
use tlx_compile::exec::fast;
use tlx_compile::exec::memo::{thread_counts, MemoScope};
use tlx_compile::ir::{ActionFn, Expr, ExprKind, PredFn, Stmt, Ty};
use tlx_eval::model::State;
use tlx_native::build::{build, BuildOptions};
use tlx_native::mixed::MixedCx;
use tlx_native::native::{Mixed, NativeLib, ERR};
use tlx_testkit::diff::{expr_kinds, Harness};
use tlx_testkit::module;

const SPEC: &str = r#"EXTENDS Naturals, FiniteSets
VARIABLES x, y
Init == x = 0 /\ y = {}
Next == x < 4 /\ x' = x + 1 /\ y' = y \cup {x}
Inv == Cardinality({s \in SUBSET y : x - 1 \in s}) # 4
Bad == x # 3 \/ (CHOOSE z \in {} : TRUE) = 0
TPos == [][x' > x /\ y \subseteq y']_<<x, y>>
"#;

/// Regions: 0 Inv, 1 Bad (state predicates), 2 TPos (transition
/// predicate), 3 `y` and 4 `x` as boxed values, 5 `x'` as a boxed value
/// (action), 6 equality of two boxed inputs.
/// Invariants 0 and 1 call regions 0 and 1; invariant 2 checks `y = y`
/// through handles; action 0 copies the state into its next-state buffer
/// and emits it iff `x' = x` holds through handles; action property 0 calls
/// region 2.
const LIB: &str = r#"
#include <string.h>
#include "tlx_abi.h"
static const char *acts[] = {"Same"};
static const char *invs[] = {"Inv", "Bad", "SameY"};
static const char *errs[] = {"boom"};
static const tlx_spec_info info = {TLX_ABI_VERSION, WORDS, 1, 3, acts, invs, 1, errs, 7, HASHULL};
const tlx_spec_info *tlx_info(void) { return &info; }
int tlx_init(const tlx_host *h, tlx_ctx *cx) { return TLX_OK; }
static int same(const tlx_host *h, tlx_ctx *cx, uint32_t r, const uint64_t *s, const uint64_t *t) {
  uint64_t a, b; int64_t eq;
  if (h->eval(cx, r, s, t, 0, &a) || h->eval(cx, r == 5 ? 4 : r, s, t, 0, &b)) return TLX_ERR;
  tlx_arg in[2] = {{.h = a}, {.h = b}};
  if (h->eval(cx, 6, s, t, in, (uint64_t *)&eq)) return TLX_ERR;
  return (int)eq;
}
int tlx_next(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t a) {
  uint64_t t[WORDS];
  memcpy(t, s, sizeof t);
  int eq = same(h, cx, 5, s, t);
  if (eq < 0) return TLX_ERR;
  return eq && h->emit(cx, t, 0) ? TLX_STOP : TLX_OK;
}
int tlx_invariant(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, uint32_t i) {
  int64_t r;
  if (i == 2) return same(h, cx, 3, s, 0);
  if (h->eval(cx, i, s, 0, 0, (uint64_t *)&r)) return TLX_ERR;
  return (int)r;
}
int tlx_action_property(const tlx_host *h, tlx_ctx *cx, const uint64_t *s, const uint64_t *t, uint32_t i) {
  int64_t r;
  if (h->eval(cx, 2, s, t, 0, (uint64_t *)&r)) return TLX_ERR;
  return (int)r;
}
"#;

fn table(h: &Harness) -> RegionTable {
    let nvars = h.layout.vars.len();
    let pred = |i: usize, ctx| {
        let p = if ctx == RegionCtx::TransPred { &h.aprops[i] } else { &h.invariants[i] };
        Region {
            expr: p.body.clone(),
            temps: p.temps.clone().into(),
            inputs: Vec::new(),
            ctx,
            assigned: Arc::from([]),
            info: Default::default(),
        }
    };
    let none: Arc<[Ty]> = Arc::from([]);
    let boxed = |kind, ctx, assigned: Vec<bool>| Region {
        expr: Expr::new(Ty::Val, kind),
        temps: none.clone(),
        inputs: Vec::new(),
        ctx,
        assigned: assigned.into(),
        info: Default::default(),
    };
    let eq = Region {
        expr: Expr::new(Ty::Bool, ExprKind::Eq(Expr::new(Ty::Val, ExprKind::Temp(1)), Expr::new(Ty::Val, ExprKind::Temp(0)))),
        temps: vec![Ty::Val, Ty::Val].into(),
        inputs: vec![0, 1],
        ctx: RegionCtx::StatePred,
        assigned: Arc::from([]),
        info: Default::default(),
    };
    let mut x_assigned = vec![false; nvars];
    x_assigned[0] = true;
    // Inv: pure, reads both variables (its results may be memoized).
    let mut inv = pred(0, RegionCtx::StatePred);
    inv.info = RegionInfo { func: "Inv".into(), enabled: false, pure: true, reads: Some(Reads { cur: vec![0, 1], next: vec![] }) };
    let regions = vec![
        inv,
        pred(1, RegionCtx::StatePred),
        pred(0, RegionCtx::TransPred),
        boxed(ExprKind::Var(1), RegionCtx::StatePred, vec![]),
        boxed(ExprKind::Var(0), RegionCtx::Action, vec![false; nvars]),
        boxed(ExprKind::NextVar(0), RegionCtx::Action, x_assigned),
        eq,
    ];
    RegionTable::new(&h.typing, regions)
}

#[test]
fn ir_regions_match_the_interpreter() {
    let h = Harness::new(&module("MixedRegions", SPEC), "INIT Init\nNEXT Next\nINVARIANT Inv Bad\nPROPERTY TPos\n");
    let mut kinds = BTreeSet::new();
    expr_kinds(&h.invariants[0].body, &mut kinds);
    assert!(kinds.contains("Interp") || kinds.contains("ValOp"), "Inv should have boxed nodes: {kinds:?}");
    let t = table(&h);
    let slots = FastRegions::slots(&t);
    let fast = Some(FastRegions { prog: &h.fast, progs: &slots });
    let regions = HostRegions { ip: &h.base.ip, typing: &h.typing, table: &t, memo: 0, fast };
    let src = LIB.replace("WORDS", &h.layout.words.to_string()).replace("HASH", &format!("{:#x}", t.hash()));
    let dir = std::env::temp_dir().join(format!("tlx-mixed-regions-{}", std::process::id()));
    let path = build(&src, &BuildOptions { cache_dir: dir, ..Default::default() }).expect("build");
    // SAFETY: LIB uses static metadata and WORDS-sized state buffers; its
    // callback slots match table() above. The test compiler/cache are trusted.
    let lib = unsafe { NativeLib::load_with(&path, Some(&regions), None) }.expect("load");
    // SAFETY: LIB uses static metadata and WORDS-sized state buffers; its
    // callback slots match table() above. The test compiler/cache are trusted.
    assert!(unsafe { NativeLib::load(&path) }.is_err(), "the region table is required");

    let (states, steps) = h.reachable(100);
    assert_eq!(states.len(), 5);
    let mut cx = MixedCx::new();
    let mut checked = (0, 0);
    let mut verdicts = BTreeSet::new();
    for vals in &states {
        let w = h.encode(vals).expect("state fits the layout");
        let st = State::new(vals.clone(), 1, 0);
        for i in 0..2 {
            let want = h.base.check_invariant(&st, i).map(|r| r.is_none());
            let got = lib.invariant_mixed(Mixed { cx: &mut cx, regions: &regions, level: 1 }, &w, i as u32);
            match want {
                Ok(b) => {
                    assert_eq!(got, b as i32, "invariant {i} on {vals:?}");
                    verdicts.insert((i, b));
                }
                Err(_) => {
                    assert_eq!(got, ERR, "invariant {i} on {vals:?}");
                    checked.1 += 1;
                }
            }
            checked.0 += 1;
        }
        assert_eq!(lib.invariant_mixed(Mixed { cx: &mut cx, regions: &regions, level: 1 }, &w, 2), 1);
        let mut out = Vec::new();
        lib.next_mixed(Mixed { cx: &mut cx, regions: &regions, level: 1 }, &w, 0, &mut out);
        assert_eq!(out, vec![w.clone()], "x' = x through handles");
    }
    assert_eq!(checked, (10, 1), "Bad fails on x = 3 only");
    assert_eq!(verdicts, BTreeSet::from([(0, false), (0, true), (1, true)]), "Inv is violated on x = 3 only");
    assert_eq!(cx.errors, 1);
    // One decoding per state (shared by the invariants and the action).
    assert_eq!(cx.cache.decodes, states.len() as u64);
    assert!(!steps.is_empty());
    for (i, j) in steps {
        let (s, t) = (&states[i], &states[j]);
        let want = h.base.check_action_property(&State::new(s.clone(), 1, 0), &State::new(t.clone(), 2, 0), 0).expect("TPos");
        let (ws, wt) = (h.encode(s).unwrap(), h.encode(t).unwrap());
        assert_eq!(lib.action_property_mixed(Mixed { cx: &mut cx, regions: &regions, level: 1 }, &ws, &wt, 0), want as i32);
        // And on the reversed step, where it is violated.
        let back = lib.action_property_mixed(Mixed { cx: &mut cx, regions: &regions, level: 1 }, &wt, &ws, 0);
        assert_eq!(back, 0);
    }
}

#[test]
fn malformed_closures_are_rejected_before_execution() {
    let harness = Harness::new(&module("MixedRegions", SPEC), "INIT Init\nNEXT Next\nINVARIANT Inv Bad\nPROPERTY TPos\n");
    let value = Expr::new(harness.typing.var_ty(0), ExprKind::ConstWords(vec![0u64].into()));
    let body = Expr::new(
        Ty::Bool,
        ExprKind::Let { temp: 0, value: value.clone(), body: Expr::new(Ty::Bool, ExprKind::Const(tlx_value::Value::TRUE)) },
    );
    let predicate = PredFn { name: "uncovered".into(), temps: vec![Ty::Val], body: body.clone() };
    let action =
        ActionFn { name: "uncovered".into(), temps: vec![Ty::Val], body: Stmt::Let { temp: 0, value, body: Box::new(Stmt::Stop) } };
    let mut notes = Vec::new();
    let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fast::Compiled::new(
            &harness.typing,
            Some(&action),
            &[Some(action.clone())],
            std::slice::from_ref(&predicate),
            std::slice::from_ref(&predicate),
            &mut notes,
        )
    }));
    let env = tlx_base::knobs::Env::process();
    if cfg!(debug_assertions) || env.flag("TLX_PARANOID") || env.flag("TLX_IR_VERIFY_HARD") {
        assert!(compiled.is_err(), "development mode must fail hard");
    } else {
        let compiled = compiled.expect("release must select interpreter fallback");
        assert!(compiled.init.is_none() && compiled.actions[0].is_none() && compiled.preds[0].is_none() && compiled.aprops[0].is_none());
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("IR verification failed") && notes[0].contains("type mismatch"));
    }
    let mut regions = table(&harness).regions().to_vec();
    regions[0].expr = body;
    regions[0].temps = vec![Ty::Val].into();
    assert!(tlx_compile::verify::region(&harness.typing, &regions[0]).is_err());
}

/// Region results memoized by their inputs and the state words they
/// read give the same verdicts, and the second pass over the states hits.
#[test]
fn memoized_regions_match_the_interpreter() {
    let h = Harness::new(&module("MixedRegions", SPEC), "INIT Init\nNEXT Next\nINVARIANT Inv Bad\nPROPERTY TPos\n");
    let t = table(&h);
    let scope = MemoScope::always("regions");
    let slots = FastRegions::slots(&t);
    let regions = HostRegions {
        ip: &h.base.ip,
        typing: &h.typing,
        table: &t,
        memo: scope.exec_id(),
        fast: Some(FastRegions { prog: &h.fast, progs: &slots }),
    };
    let src = LIB.replace("WORDS", &h.layout.words.to_string()).replace("HASH", &format!("{:#x}", t.hash()));
    let dir = std::env::temp_dir().join(format!("tlx-mixed-regions-memo-{}", std::process::id()));
    let path = build(&src, &BuildOptions { cache_dir: dir, ..Default::default() }).expect("build");
    // SAFETY: LIB uses static metadata and WORDS-sized state buffers; its
    // callback slots match table() above. The test compiler/cache are trusted.
    let lib = unsafe { NativeLib::load_with(&path, Some(&regions), None) }.expect("load");
    let (states, _) = h.reachable(100);
    let mut cx = MixedCx::new();
    let (lookups0, hits0) = thread_counts();
    for pass in 0..2 {
        for vals in &states {
            let w = h.encode(vals).expect("state fits the layout");
            let want = h.base.check_invariant(&State::new(vals.clone(), 1, 0), 0).map(|r| r.is_none()).expect("Inv");
            let got = lib.invariant_mixed(Mixed { cx: &mut cx, regions: &regions, level: 1 }, &w, 0);
            assert_eq!(got, want as i32, "pass {pass}: Inv on {vals:?}");
        }
    }
    let (lookups, hits) = thread_counts();
    assert_eq!(lookups - lookups0, 2 * states.len() as u64);
    assert_eq!(hits - hits0, states.len() as u64, "the second pass hits");
}

/// Memoizable regions that compute the same value from the same
/// memo key share a memo node, whatever their function, context or temp
/// numbering; a different read set, an unknown one or a different
/// computation does not.
#[test]
fn equivalent_regions_share_a_memo_node() {
    let h = Harness::new(&module("MixedRegions", SPEC), "INIT Init\nNEXT Next\nINVARIANT Inv Bad\nPROPERTY TPos\n");
    let base = table(&h);
    let inv = base.regions()[0].clone();
    let pure =
        |func: &str, cur: Vec<u32>| RegionInfo { func: func.into(), enabled: false, pure: true, reads: Some(Reads { cur, next: vec![] }) };
    let as_trans = Region { ctx: RegionCtx::TransPred, info: pure("TPos", vec![0, 1]), ..inv.clone() };
    let fewer_reads = Region { info: pure("Inv", vec![0]), ..inv.clone() };
    let unknown_reads = Region { info: RegionInfo { reads: None, ..pure("Inv", vec![]) }, ..inv.clone() };
    let temp = |t| Expr::new(Ty::Val, ExprKind::Temp(t));
    let eq = |a, b, inputs: Vec<u32>, func: &str| Region {
        expr: Expr::new(Ty::Bool, ExprKind::Eq(temp(a), temp(b))),
        temps: vec![Ty::Val, Ty::Val].into(),
        inputs,
        ctx: RegionCtx::StatePred,
        assigned: Arc::from([]),
        info: pure(func, vec![]),
    };
    let regions = vec![
        inv,
        as_trans,
        fewer_reads,
        unknown_reads,
        // input 1 = input 0
        eq(1, 0, vec![0, 1], "A"),
        // the same, temps renamed
        eq(0, 1, vec![1, 0], "B"),
        // input 0 = input 1
        eq(0, 1, vec![0, 1], "C"),
    ];
    let t = RegionTable::new(&h.typing, regions);
    assert_eq!(t.memo_class(&h.base.ip.prog), &[0, 0, 2, 3, 4, 4, 6]);
}
