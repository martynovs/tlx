//! What interpreter fallbacks (`ExprKind::Interp`) capture from the
//! lowering: by-name arguments with one value per context are evaluated at
//! most once per fallback evaluation, and only when read (a failing
//! argument that is not read is no error, as in TLC); materialized
//! definitions are handed to the fallbacks that may evaluate them.

use std::sync::Arc;

use tlx_compile::exec::fast;
use tlx_compile::ir::{Deferred, Expr, ExprKind, PredFn};
use tlx_compile::lower::Lowerer;
use tlx_compile::types::Typing;
use tlx_eval::InterpModel;
use tlx_layout::{Layout, Shape};
use tlx_runtime::Model;
use tlx_value::Value;

const SPEC: &str = r#"---- MODULE Capture ----
EXTENDS Naturals, Sequences, FiniteSets, TLC
VARIABLES x, y
Seen == {n \in 0..4 : n <= x}
Init == x = 0 /\ y = <<>>
Next == x < 4 /\ x' = x + 1 /\ y' = Append(y, x + 1)
Keep(s, bad) == Len(SelectSeq(s, LAMBDA e: e > 0 \/ Cardinality(bad) = 0))
\* (ToString keeps the argument boxed: a typed set of integers would be
\* compiled natively, without a fallback.)
Unused == Keep(y, {ToString(1 \div (x - x))}) = Len(y)
Read(s, bad) == Len(SelectSeq(s, LAMBDA e: e > 3 \/ Cardinality(bad) = 0))
Used == Read(y, {ToString(1 \div (x - x))}) >= 0
Mat == Cardinality(Seen) = x + 1 /\ SortSeq(<<Cardinality(Seen), x>>, LAMBDA a, b: a < b) = <<x, x + 1>>
Step == /\ Cardinality(Seen') = Cardinality(Seen) + 1
        /\ SortSeq(<<Cardinality(Seen'), Cardinality(Seen)>>, LAMBDA a, b: a < b) = <<x + 1, x + 2>>
Prop == [][Step]_<<x, y>>
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Unused Used Mat\nPROPERTY Prop\n";

fn setup() -> (InterpModel, Typing) {
    let model = tlx_testkit::spec::load(SPEC, CFG);
    let x = Shape::Int { lo: 0, hi: 4 };
    let y = Shape::Seq { elem: Box::new(Shape::Int { lo: 1, hi: 4 }), max_len: 4 };
    let seen = model.ip.prog.defs.iter().position(|d| &*d.name == "Seen").expect("Seen");
    let defs = [(tlx_sema::hir::DefId(seen as u32), Shape::Set { elem: Box::new(Shape::Int { lo: 0, hi: 4 }), max_card: 5 })];
    let typing = Typing::with_defs(Arc::new(Layout::new(vec![x, y])), &defs);
    (model, typing)
}

fn lower(model: &InterpModel, typing: &Typing, name: &str) -> (PredFn, Typing) {
    let mut lw = Lowerer::with_typing(&model.ip, typing.clone());
    let p = match model.invariant_names().iter().position(|n| n == name) {
        Some(i) => lw.lower_pred(name, &model.invariant_exprs()[i]),
        None => lw.lower_trans_pred(name, &model.action_property_exprs()[0]),
    };
    (p, lw.into_typing())
}

fn words(typing: &Typing, x: i64) -> Vec<u64> {
    let vals = vec![Value::Int(x), Value::Tuple((1..=x).map(Value::Int).collect::<Vec<_>>().into())];
    let mut w = vec![0u64; typing.layout.words];
    typing.layout.encode_state(&vals, &mut w).expect("state fits the layout");
    w
}

/// The IR executor's verdict of `p` in the state with `x` (and, for a
/// transition predicate, the step to `x + 1`).
fn eval(model: &InterpModel, typing: &Typing, p: &PredFn, x: i64, step: bool) -> Result<bool, String> {
    let s0 = words(typing, x);
    let s1 = if step { words(typing, x + 1) } else { Vec::new() };
    let program = fast::Program::new(typing);
    let predicate = if step { program.transition(typing, p) } else { program.predicate(typing, p) }.expect("closure compilation");
    let run = fast::Run::words(&model.ip, typing, &program, 1);
    if step {
        // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
        unsafe { fast::eval_trans_words(&run, &predicate, &s0, &s1) }
    } else {
        // SAFETY: valid words from the matching encoder/program, with published Dyn handles.
        unsafe { fast::eval_pred_words(&run, &predicate, &s0) }
    }
    .map_err(|error| error.into_message())
}

fn interps(e: &Expr, out: &mut Vec<Expr>) {
    if matches!(&*e.kind, ExprKind::Interp { .. }) {
        out.push(e.clone());
    }
    e.for_each_child(&mut |c| interps(c, out));
}

fn deferred(p: &PredFn) -> Vec<Deferred> {
    let mut xs = Vec::new();
    interps(&p.body, &mut xs);
    xs.iter()
        .flat_map(|e| match &*e.kind {
            ExprKind::Interp { deferred, .. } => deferred.clone(),
            _ => unreachable!(),
        })
        .collect()
}

/// The `def_value_key`s handed to the fallbacks of `p`.
fn def_values(p: &PredFn) -> Vec<(tlx_sema::hir::DefId, bool)> {
    let mut xs = Vec::new();
    interps(&p.body, &mut xs);
    xs.iter()
        .flat_map(|e| match &*e.kind {
            ExprKind::Interp { binds, .. } => binds.iter().filter_map(|(l, _)| tlx_eval::env::def_of_value_key(*l)).collect::<Vec<_>>(),
            _ => unreachable!(),
        })
        .collect()
}

#[test]
fn unread_failing_argument_is_no_error() {
    let (model, typing) = setup();
    let (p, typing) = lower(&model, &typing, "Unused");
    assert!(deferred(&p).iter().any(|d| matches!(d, Deferred::Once(..))), "the argument is captured as evaluated at most once");
    for x in 0..=4 {
        assert_eq!(eval(&model, &typing, &p, x, false), Ok(true), "x={x}");
    }
}

#[test]
fn read_failing_argument_fails() {
    let (model, typing) = setup();
    let (p, typing) = lower(&model, &typing, "Used");
    assert!(deferred(&p).iter().any(|d| matches!(d, Deferred::Once(..))));
    // y = <<>>: the argument is not read.
    assert_eq!(eval(&model, &typing, &p, 0, false), Ok(true));
    // y = <<1>>: it is (and fails, each time it is read).
    for x in 1..=3 {
        assert!(eval(&model, &typing, &p, x, false).is_err(), "x={x}");
    }
    // y = <<1, 2, 3, 4>>: read for the first three elements only.
    assert!(eval(&model, &typing, &p, 4, false).is_err());
}

#[test]
fn materialized_definitions_are_handed_to_fallbacks() {
    let (model, typing) = setup();
    let (p, typing) = lower(&model, &typing, "Mat");
    let seen = model.ip.prog.defs.iter().position(|d| &*d.name == "Seen").unwrap() as u32;
    assert_eq!(def_values(&p), vec![(tlx_sema::hir::DefId(seen), false)]);
    for x in 0..=4 {
        assert_eq!(eval(&model, &typing, &p, x, false), Ok(true), "x={x}");
    }
}

#[test]
fn primed_materialized_definitions_are_handed_to_fallbacks() {
    let (model, typing) = setup();
    let (p, typing) = lower(&model, &typing, "Step");
    let seen = tlx_sema::hir::DefId(model.ip.prog.defs.iter().position(|d| &*d.name == "Seen").unwrap() as u32);
    let mut got = def_values(&p);
    got.sort_by_key(|(d, primed)| (d.0, *primed));
    assert_eq!(got, vec![(seen, false), (seen, true)]);
    for x in 0..=3 {
        assert_eq!(eval(&model, &typing, &p, x, true), Ok(true), "x={x}");
    }
}

const PARAM_SPEC: &str = r#"---- MODULE CaptureParam ----
EXTENDS Naturals, Sequences, FiniteSets, TLC
VARIABLES x, y
Seen == {n \in 0..4 : n <= x}
Init == x = 0 /\ y = <<>>
Next == x < 4 /\ x' = x + 1 /\ y' = Append(y, x + 1)
Ev(F(_)) == <>(Cardinality(Seen) > 0 /\ Len(SelectSeq(<<1>>, LAMBDA e : F(e) > 9)) = 1)
Live == Ev(LAMBDA a : Cardinality(Seen) + a)
====
"#;

/// A fallback applying an operator parameter bound in its
/// environment (here a LAMBDA of a liveness atom) may evaluate what the
/// argument does: the materialized definitions it reaches that way are
/// handed to it too.
#[test]
fn materialized_definitions_reached_through_operator_parameters() {
    let model = tlx_testkit::spec::load(PARAM_SPEC, "INIT Init\nNEXT Next\nPROPERTY Live\n");
    let x = Shape::Int { lo: 0, hi: 4 };
    let y = Shape::Seq { elem: Box::new(Shape::Int { lo: 1, hi: 4 }), max_len: 4 };
    let seen = tlx_sema::hir::DefId(model.ip.prog.defs.iter().position(|d| &*d.name == "Seen").expect("Seen") as u32);
    let defs = [(seen, Shape::Set { elem: Box::new(Shape::Int { lo: 0, hi: 4 }), max_card: 5 })];
    let typing = Typing::with_defs(Arc::new(Layout::new(vec![x, y])), &defs);
    let mut n = 0;
    for a in &model.live.state_atoms {
        let tlx_eval::model::liveness::StateAtom::Pred(a) = a else { continue };
        let mut lw = Lowerer::with_typing(&model.ip, typing.clone());
        let p = lw.lower_pred_in("atom", &a.expr, &a.env, false);
        let mut xs = Vec::new();
        interps(&p.body, &mut xs);
        if xs.is_empty() {
            continue;
        }
        assert!(def_values(&p).contains(&(seen, false)), "{:?}", p.body);
        n += 1;
    }
    assert!(n >= 1);
}
