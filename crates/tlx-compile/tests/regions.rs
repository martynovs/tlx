//! Boxed regions (docs/mixed-mode.md §4): R3 folds an all-boxed
//! subtree into one interpreter fallback, R9 widens a binding construct
//! whose body depends on the element through a fallback into one fallback,
//! and hoists loop-invariant regions out of binding constructs (guarded by
//! a non-empty source). The transformed IR must evaluate exactly as before;
//! the region table lists the regions with their context.

use std::sync::Arc;

use tlx_compile::enc::mixed::{Reads, RegionCtx, RegionTable};
use tlx_compile::exec::fast;
use tlx_compile::ir::{Expr, ExprKind, PredFn, Ty};
use tlx_compile::lower::Lowerer;
use tlx_compile::types::Typing;
use tlx_eval::InterpModel;
use tlx_layout::{Layout, Shape};
use tlx_runtime::Model;
use tlx_value::Value;

const SPEC: &str = r#"---- MODULE Regions ----
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
Next == x < 3 /\ x' = x + 1 /\ p' = [p EXCEPT ![x + 1] = 1] /\ TC(Par)[1, 1] \in BOOLEAN
Fold == TC(Par)[1, 1] \in BOOLEAN
Reach == \A a \in S : TC(Par)[a, 1]
Hoist == \A a \in S : TC(Par)[1, 1] \in BOOLEAN /\ a + 5 > x
Guard == \A a \in {v \in S : v > x + 10} : TC(Par)[1, x + 4] /\ a > 0
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Fold\nINVARIANT Reach\nINVARIANT Hoist\nINVARIANT Guard\n";

fn setup() -> (InterpModel, Typing) {
    let model = tlx_testkit::spec::load(SPEC, CFG);
    let x = Shape::Int { lo: 0, hi: 3 };
    let p = Shape::Tuple(vec![Shape::Int { lo: 1, hi: 3 }; 3].into());
    (model, Typing::new(Arc::new(Layout::new(vec![x, p]))))
}

/// The invariant `name` lowered with typing, with or without regions, and
/// the typing extended with the shapes the lowering interned.
fn lower(model: &InterpModel, typing: &Typing, name: &str, regions: bool) -> (PredFn, Typing) {
    let i = model.invariant_names().iter().position(|n| n == name).expect("invariant");
    let mut lw = Lowerer::with_typing(&model.ip, typing.clone());
    lw.regions = regions;
    let p = lw.lower_pred(name, &model.invariant_exprs()[i]);
    (p, lw.into_typing())
}

/// The IR executor's verdict of `p` on the state (x, p).
fn eval(model: &InterpModel, typing: &Typing, p: &PredFn, x: i64, par: [i64; 3]) -> Result<bool, String> {
    let vals = vec![Value::Int(x), Value::Tuple(par.iter().map(|v| Value::Int(*v)).collect::<Vec<_>>().into())];
    let mut words = vec![0u64; typing.layout.words];
    typing.layout.encode_state(&vals, &mut words).expect("state fits the layout");
    let program = fast::Program::new(typing);
    let predicate = program.predicate(typing, p).expect("closure compilation");

    // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
    unsafe { fast::eval_pred_words(&fast::Run::words(&model.ip, typing, &program, 1), &predicate, &words) }
        .map_err(|error| error.into_message())
}

fn count(e: &Expr, pred: &dyn Fn(&Expr) -> bool) -> usize {
    let mut n = pred(e) as usize;
    if !matches!(&*e.kind, ExprKind::Interp { .. }) {
        e.for_each_child(&mut |c| n += count(c, pred));
    }
    n
}

fn is_interp(e: &Expr) -> bool {
    matches!(&*e.kind, ExprKind::Interp { .. })
}

/// Both lowerings agree with the expected verdicts on two states.
fn check_same(model: &InterpModel, typing: &Typing, name: &str, want: [bool; 2]) -> (PredFn, PredFn) {
    let (on, ton) = lower(model, typing, name, true);
    let (off, toff) = lower(model, typing, name, false);
    for (k, par) in [[1, 1, 2], [2, 3, 3]].into_iter().enumerate() {
        for x in 0..=3 {
            assert_eq!(eval(model, &ton, &on, x, par), Ok(want[k]), "{name} with regions at x={x}, p={par:?}");
            assert_eq!(eval(model, &toff, &off, x, par), Ok(want[k]), "{name} without regions at x={x}, p={par:?}");
        }
    }
    (on, off)
}

#[test]
fn r3_folds_an_all_boxed_subtree() {
    let (model, typing) = setup();
    let (on, off) = check_same(&model, &typing, "Fold", [true, true]);
    // Without regions: `\in` of an application of a fallback.
    assert!(count(&off.body, &|e| matches!(&*e.kind, ExprKind::ValOp { .. } | ExprKind::Apply(..))) > 0);
    // With: one fallback for the whole expression.
    assert_eq!(count(&on.body, &is_interp), 1);
    assert_eq!(count(&on.body, &|e| matches!(&*e.kind, ExprKind::ValOp { .. } | ExprKind::Apply(..))), 0);
    match &*on.body.kind {
        ExprKind::Convert(x) => assert!(is_interp(x)),
        k => panic!("unexpected root {k:?}"),
    }
}

#[test]
fn r9_widens_a_quantifier_over_a_dependent_fallback() {
    let (model, typing) = setup();
    let (on, off) = check_same(&model, &typing, "Reach", [true, false]);
    let forall = |e: &Expr| matches!(&*e.kind, ExprKind::Forall { .. });
    assert_eq!(count(&off.body, &forall), 1);
    assert_eq!(count(&on.body, &forall), 0);
    assert_eq!(count(&on.body, &is_interp), 1);
}

#[test]
fn r9_hoists_an_invariant_region() {
    let (model, typing) = setup();
    let (on, off) = check_same(&model, &typing, "Hoist", [true, true]);
    assert!(matches!(&*off.body.kind, ExprKind::Forall { .. }));
    // `let t = <fallback> in \A a \in S : t /\ ...` (a constant source:
    // no emptiness guard).
    let ExprKind::Let { value, body, .. } = &*on.body.kind else { panic!("not hoisted: {:?}", on.body) };
    assert!(count(value, &is_interp) == 1 && !matches!(&*value.kind, ExprKind::If(..)));
    let ExprKind::Forall { body: inner, .. } = &*body.kind else { panic!("loop expected") };
    assert_eq!(count(inner, &is_interp), 0);
}

#[test]
fn r9_hoisting_is_guarded_by_a_non_empty_source() {
    let (model, typing) = setup();
    // The hoisted region fails (`4 + x` is outside the domain), but the
    // source is always empty: the quantifier holds, as without hoisting.
    let (on, _) = check_same(&model, &typing, "Guard", [true, true]);
    let mut lets = 0;
    let mut e = &on.body;
    while let ExprKind::Let { value, body, .. } = &*e.kind {
        if let ExprKind::If(c, ..) = &*value.kind {
            assert!(matches!(&*c.kind, ExprKind::Exists { .. }), "guard {c:?}");
        }
        lets += 1;
        e = body;
    }
    assert!(lets >= 2, "source and region bound before the loop: {:?}", on.body);
    assert!(matches!(&*e.kind, ExprKind::Forall { .. }));
}

#[test]
fn region_table_lists_regions_with_context() {
    let (model, typing) = setup();
    let mut lw = Lowerer::with_typing(&model.ip, typing);
    let mut regions = Vec::new();
    let hoist = lw.lower_pred("Hoist", &model.invariant_exprs()[2]);
    lw.pred_regions(&hoist, false, &mut regions);
    assert_eq!(regions.len(), 1, "{regions:?}");
    let r = &regions[0];
    assert_eq!((r.ctx, r.expr.ty, r.info.pure, r.info.enabled), (RegionCtx::StatePred, Ty::Bool, true, false));
    assert_eq!(&*r.info.func, "Hoist");
    assert!(r.inputs.is_empty());
    // The fallback reads `p` (through `Par`).
    assert_eq!(r.info.reads, Some(Reads { cur: vec![1], next: vec![] }));

    let next = lw.lower_action(&model.actions()[0]).expect("lowered");
    lw.action_regions(&next, false, &mut regions);
    assert!(regions.len() >= 2, "{regions:?}");
    let r = regions.last().unwrap();
    assert_eq!(r.ctx, RegionCtx::Action);
    // Evaluated after both assignments.
    assert_eq!(&r.assigned[..], &[true, true]);
    let typing = lw.into_typing();
    let table = RegionTable::new(&typing, regions.clone());
    assert!(table.canonical().starts_with("region 0 StatePred result Bool inputs [] assigned  temps"), "{}", table.canonical());
    assert!(table.canonical().contains("in \"Hoist\" pure reads [1]/[]"), "{}", table.canonical());
    assert_eq!(RegionTable::new(&typing, regions.clone()).hash(), table.hash());
    let mut fewer = regions;
    fewer.pop();
    assert_ne!(RegionTable::new(&typing, fewer).hash(), table.hash());
}

const PARAM_SPEC: &str = r#"---- MODULE ParamReads ----
EXTENDS Naturals, TLC
VARIABLES x, y
Ev(F(_)) == <>(ToString(F(1)) = "a")
Twice(G(_)) == Ev(G)
Init == x = 0 /\ y = 0
Next == x < 2 /\ x' = x + 1 /\ y' = y
Live == Ev(LAMBDA a : y + a)
Pass == Twice(LAMBDA a : y + a)
====
"#;

/// A fallback applying an operator parameter reads what the
/// argument bound to it reads: here a LAMBDA bound in the base environment
/// of a liveness atom (also when passed on through another operator).
#[test]
fn region_reads_follow_operator_parameters() {
    let model = tlx_testkit::spec::load(PARAM_SPEC, "INIT Init\nNEXT Next\nPROPERTY Live\nPROPERTY Pass\n");
    let typing = Typing::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 2 }, Shape::Int { lo: 0, hi: 0 }])));
    let mut n = 0;
    for a in &model.live.state_atoms {
        let tlx_eval::model::liveness::StateAtom::Pred(a) = a else { continue };
        let mut lw = Lowerer::with_typing(&model.ip, typing.clone());
        let p = lw.lower_pred_in("atom", &a.expr, &a.env, false);
        let mut regions = Vec::new();
        lw.pred_regions(&p, false, &mut regions);
        assert_eq!(regions.len(), 1, "{:?}", p.body);
        assert_eq!(regions[0].info.reads, Some(Reads { cur: vec![1], next: vec![] }), "{:?}", p.body);
        n += 1;
    }
    assert!(n >= 1);
}

const CALL_SPEC: &str = r#"---- MODULE CallReads ----
EXTENDS Integers, FiniteSets
VARIABLES y, x
Step(S) == S \cup {i \in 1..3 : i \notin S /\ x[i] < Cardinality(SUBSET S)}
RECURSIVE Grow(_)
Grow(S) == LET nxt == Step(S) IN IF nxt = S THEN S ELSE Grow(nxt)
Init == y = 0 /\ x \in [1..3 -> 0..2]
Next == UNCHANGED <<y, x>>
Inv == Grow({}) # {}
====
"#;

/// The first `Call` node of `e`.
fn find_call(e: &Expr) -> Option<Expr> {
    if matches!(&*e.kind, ExprKind::Call { .. }) {
        return Some(e.clone());
    }
    let mut found = None;
    e.for_each_child(&mut |c| {
        if found.is_none() {
            found = find_call(c);
        }
    });
    found
}

/// A region that calls a compiled function (a RECURSIVE operator) reads
/// what the function's body reads: the call `Grow({})` has no state
/// variable among its arguments, but its memo key must include `x`
/// (Cassandra Accord's NoStuck was memoized across states without it).
#[test]
fn region_reads_follow_compiled_functions() {
    let model = tlx_testkit::spec::load(CALL_SPEC, "INIT Init\nNEXT Next\nINVARIANT Inv\n");
    let x = Shape::Tuple(vec![Shape::Int { lo: 0, hi: 2 }; 3].into());
    let typing = Typing::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 0 }, x])));
    let mut lw = Lowerer::with_typing(&model.ip, typing);
    let p = lw.lower_pred("Inv", &model.invariant_exprs()[0]);
    let call = find_call(&p.body).unwrap_or_else(|| panic!("Grow compiled to a function: {:?}", p.body));
    assert_eq!(lw.region_reads(&call), Some(Reads { cur: vec![1], next: vec![] }), "{call:?}");
}
