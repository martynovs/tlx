//! What `TLCGet("spec")` and `TLCGet("action")` report: records naming the
//! parts of the model with their source locations, as TLC builds them
//! (tlc2.module.TLCGetSet, tlc2.tool.Action#toRecord).

use super::{location_record, InterpModel};
use crate::eval::Ctx;
use crate::SpecInfo;
use tlx_front::ast::Span;
use tlx_sema::hir::{DefKind, Expr, ExprKind, OpRef};
use tlx_sema::source::EndTrim;
use tlx_value::{Sym, Value};

fn rec(fields: Vec<(&str, Value)>) -> Value {
    Value::record(fields.into_iter().map(|(n, v)| (Sym::new(n), v)).collect())
}

/// A set of records; empty if they cannot be compared.
fn set(elems: Vec<Value>) -> Value {
    Value::set_from(elems).unwrap_or_else(|_| Value::empty_set())
}

/// TLC's location record of `sp` (ending at its last non-blank character).
fn location(m: &InterpModel, sp: Span) -> Value {
    location_record(&m.range(sp, EndTrim::Blank))
}

fn named(m: &InterpModel, name: &str, sp: Span) -> Value {
    rec(vec![("name", Value::str(name)), ("location", location(m, sp))])
}

/// The span TLC reports for `e`: the body of a (0-ary) operator it names.
fn body_span(m: &InterpModel, e: &Expr) -> Span {
    let mut e = e;
    for _ in 0..16 {
        let ExprKind::Apply { op: OpRef::Def(d), args } = &*e.kind else { break };
        if !args.is_empty() {
            break;
        }
        match &m.ip.prog.defs[d.idx()].kind {
            DefKind::Op(body) => e = body,
            _ => break,
        }
    }
    e.span
}

/// The span of the whole definition named by `e`.
fn def_span(m: &InterpModel, e: &Expr) -> Span {
    match &*e.kind {
        ExprKind::Apply { op: OpRef::Def(d), .. } => m.ip.prog.defs[d.idx()].span,
        _ => e.span,
    }
}

/// The record of action `i` of the model.
fn action(m: &InterpModel, i: usize) -> Value {
    let a = &m.actions()[i];
    // (An action not from a definition: TLC's Action.UNNAMED_ACTION.)
    let name = if a.def.is_some() { &*a.name } else { "UnnamedAction" };
    let mut fields = vec![("name", Value::str(name)), ("location", location(m, a.span()))];
    if let Some(d) = a.def {
        let params = &m.ip.prog.defs[d.idx()].params;
        let mut names = Vec::new();
        let mut context = Vec::new();
        for p in params {
            let name = m.ip.prog.locals[p.idx()].name.clone();
            let v = match a.env.lookup(*p) {
                Some(crate::env::Binding::Val(v)) => Some(v.clone()),
                Some(b @ (crate::env::Binding::Thunk(_) | crate::env::Binding::Name(..))) => {
                    let (e, env) = b.by_name(&m.ip.prog).unwrap();
                    m.ip.eval(e, env, &Ctx::NONE).ok()
                }
                _ => None,
            };
            // (TLC lists the parameters that have a value: none for an
            // operator applied to arguments that are not constant.)
            if let Some(v) = v {
                context.push((Sym::new(&name), v));
                names.push(Value::str(&name));
            }
        }
        if !names.is_empty() {
            fields.push(("context", Value::record(context)));
            fields.push(("parameters", Value::tuple(names)));
        }
    }
    rec(fields)
}

/// The record of the initial predicate.
fn init(m: &InterpModel) -> Value {
    let e = m.init_expr();
    let name = match &*e.kind {
        ExprKind::Apply { op: OpRef::Def(d), args } if args.is_empty() => m.ip.prog.defs[d.idx()].name.to_string(),
        _ => "Init".to_string(),
    };
    named(m, &name, body_span(m, e))
}

pub fn spec_info(m: &InterpModel) -> SpecInfo {
    let cfg = &m.ip.cfg;
    let parts = m.spec_parts();
    let named_parts = |ps: &[(String, Expr)]| -> Vec<Value> { ps.iter().map(|(n, e)| named(m, n, body_span(m, e))).collect() };
    let actions: Vec<Value> = (0..m.actions().len()).map(|i| action(m, i)).collect();
    let mut invariants: Vec<Value> = cfg.invariants.iter().zip(m.invariant_exprs()).map(|(n, e)| named(m, n, body_span(m, e))).collect();
    invariants.extend(named_parts(&parts.implied_invariants));
    let constraints = cfg.constraints.iter().zip(m.constraint_exprs()).map(|(n, e)| named(m, n, def_span(m, e))).collect();
    let action_constraints =
        cfg.action_constraints.iter().zip(m.action_constraint_exprs()).map(|(n, e)| named(m, n, def_span(m, e))).collect();
    let variables = m.ip.prog.vars.iter().map(|v| named(m, &v.name, v.span)).collect();
    let temporals = parts.temporals.iter().map(|e| named(m, "UnnamedAction", e.span)).collect();
    let init_action = init(m);
    let spec = rec(vec![
        ("impliedinits", set(named_parts(&parts.implied_inits))),
        ("impliedactions", set(named_parts(&parts.implied_actions))),
        ("invariants", set(invariants)),
        ("impliedtemporals", set(named_parts(&parts.implied_temporals))),
        ("temporals", set(temporals)),
        ("actions", set(actions.clone())),
        ("inits", set(if m.has_behavior { vec![init_action.clone()] } else { Vec::new() })),
        ("variables", set(variables)),
        ("actionconstraints", set(action_constraints)),
        ("constraints", set(constraints)),
    ]);
    SpecInfo { spec, init_action, actions }
}
