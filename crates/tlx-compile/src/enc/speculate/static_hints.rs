//! Static shape hints and the choice of definitions worth materializing.

use std::collections::HashSet;

use tlx_eval::strings::StringMakers;
use tlx_eval::InterpModel;
use tlx_layout::infer::Hints;
use tlx_sema::hir::{self, DefId};
use tlx_value::Value;

use super::model_strings;

/// Static hints for shape inference: atoms (cfg model values, string
/// literals) and integer ranges (cfg integers, constant intervals/sets).
pub fn hints(base: &InterpModel) -> Hints {
    let cfg = base.ip.cfg_values();
    let mut atoms: Vec<Value> = cfg.iter().flat_map(atoms_of).collect();
    let mut strs = HashSet::new();
    for f in base.ip.prog.files.iter().filter(|f| !f.path.starts_with("<stdlib>")) {
        for t in tlx_front::parse::token_texts(&f.src).1 {
            if strs.insert(t.clone()) {
                atoms.push(Value::str(&t));
            }
        }
    }
    let mut int_ranges: Vec<(i64, i64)> =
        cfg.iter().filter_map(|v| if let Value::Int(i) = v { Some((0.min(*i), 0.max(*i))) } else { None }).collect();
    for v in reachable_const_values(base).iter().chain(cfg.iter()) {
        match v {
            Value::Interval(lo, hi) if lo <= hi && hi - lo < 1 << 20 => int_ranges.push((*lo, *hi)),
            Value::Set(es) if !es.is_empty() && es.iter().all(|e| matches!(e, Value::Int(_))) => {
                let ints = es.iter().filter_map(|e| if let Value::Int(i) = e { Some(*i) } else { None });
                int_ranges.push(ints.fold((i64::MAX, i64::MIN), |(a, b), i| (a.min(i), b.max(i))));
            }
            _ => {}
        }
    }
    Hints { atoms, int_ranges }
}

/// Values of the 0-ary constant definitions the model uses (from Init, the
/// actions, invariants, constraints, action properties and VIEW), and of
/// the nondeterministic ones: other unused ones may be expensive (e.g. a CHOOSE over all graphs that the cfg
/// replaces by another definition).
fn reachable_const_values(base: &InterpModel) -> Vec<Value> {
    use tlx_sema::hir::{DefKind, ExprKind as H, OpRef};
    let ip = &base.ip;
    let mut stack: Vec<hir::Expr> = vec![base.init_expr().clone()];
    stack.extend(base.actions().iter().map(|a| a.expr.clone()));
    for es in [base.invariant_exprs(), base.constraint_exprs(), base.action_constraint_exprs(), base.action_property_exprs()] {
        stack.extend(es.iter().cloned());
    }
    stack.extend(base.view_expr().cloned());
    let mut seen = HashSet::new();
    let mut consts = Vec::new();
    while let Some(e) = stack.pop() {
        let mut def = |d: DefId, stack: &mut Vec<hir::Expr>| {
            let d = ip.redirect_def(d);
            if !seen.insert(d) {
                return;
            }
            let def = &ip.prog.defs[d.idx()];
            if def.builtin.is_some() {
                return;
            }
            if ip.is_const_def(d) {
                consts.push(d);
            }
            match &def.kind {
                DefKind::Op(b) => stack.push(b.clone()),
                DefKind::Fn { body, bounds } => {
                    stack.push(body.clone());
                    stack.extend(bounds.iter().map(|b| b.set.clone()));
                }
                DefKind::Pending => {}
            }
        };
        match &*e.kind {
            H::Apply { op: OpRef::Def(d), .. } | H::OpArg(OpRef::Def(d)) => def(*d, &mut stack),
            H::Apply { op: OpRef::Const(c), .. } | H::OpArg(OpRef::Const(c)) => {
                if let Some(OpRef::Def(d)) = ip.const_op_ref(*c) {
                    def(d, &mut stack);
                }
            }
            _ => {}
        }
        tlx_sema::visit::children(&e, &mut |c| stack.push(c.clone()));
    }
    // Constants making random choices (or printing) are evaluated even if
    // unused, all in definition order, as TLC does at startup (the first
    // evaluation of a random constant draws from the seeded generator).
    consts.extend(
        (0..ip.prog.defs.len() as u32)
            .map(DefId)
            .filter(|d| ip.is_const_def(*d) && !ip.is_pure_def(*d) && ip.prog.defs[d.idx()].builtin.is_none()),
    );
    consts.sort();
    consts.dedup();
    // (Not a definition TLC evaluates at first use that would create strings
    // now: see `search_makes_strings`.)
    let strings = StringMakers::typed(ip);
    consts
        .into_iter()
        .filter(|d| !strings.applied(*d))
        .filter_map(|d| {
            let call = hir::Expr::new(H::Apply { op: OpRef::Def(d), args: Vec::new() }, ip.prog.defs[d.idx()].span);
            ip.eval(&call, &tlx_eval::env::Env::default(), &tlx_eval::eval::Ctx::NONE).ok()
        })
        .collect()
}

/// The atoms (model values, strings) occurring in a value.
fn atoms_of(v: &Value) -> Vec<Value> {
    match v {
        Value::ModelValue(_) | Value::Str(_) => vec![v.clone()],
        Value::Set(es) | Value::Tuple(es) => es.iter().flat_map(atoms_of).collect(),
        Value::Record(fs) => fs.iter().flat_map(|(_, x)| atoms_of(x)).collect(),
        Value::Fcn(ps, _) => ps.iter().flat_map(|(k, x)| atoms_of(k).into_iter().chain(atoms_of(x))).collect(),
        _ => Vec::new(),
    }
}

/// 0-ary state-level definitions used by invariants and action properties
/// (directly or through other definitions): candidates for materialization.
pub fn materialize_candidates(base: &InterpModel) -> Vec<DefId> {
    use tlx_sema::hir::{DefKind, ExprKind as H, OpRef};
    let ip = &base.ip;
    // Probing a definition in every pre-run state would create its strings
    // ahead of the search (see `search_makes_strings`). (`Tail`, `SubSeq`
    // and `\o` of the sequences of the states create none.)
    let strings = model_strings(base);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<hir::Expr> = base.invariant_exprs().iter().chain(base.action_property_exprs()).cloned().collect();
    while let Some(e) = stack.pop() {
        if let H::Apply { op: OpRef::Def(d), .. } | H::OpArg(OpRef::Def(d)) = &*e.kind {
            let d = ip.redirect_def(*d);
            let zero = matches!(&*e.kind, H::Apply { args, .. } if args.is_empty());
            if seen.insert(d) {
                if zero && ip.is_state_cacheable(d) && !strings.applied(d) {
                    out.push(d);
                }
                match &ip.prog.defs[d.idx()].kind {
                    DefKind::Op(b) => stack.push(b.clone()),
                    DefKind::Fn { body, bounds } => {
                        stack.push(body.clone());
                        stack.extend(bounds.iter().map(|b| b.set.clone()));
                    }
                    DefKind::Pending => {}
                }
            }
        }
        tlx_sema::visit::children(&e, &mut |c| stack.push(c.clone()));
    }
    // The VIEW definition: its shape lets fingerprints be computed natively
    // (see `EncModel`).
    // It is evaluated in states only, so a definition classified
    // action-level only because its operators take operator arguments (by
    // name) qualifies too, if nothing it applies reads the next state.
    if let Some(d) = view_def(base) {
        if !seen.contains(&d) && (ip.is_state_cacheable(d) || state_level_def(base, d)) {
            out.push(d);
        }
    }
    out.sort();
    out
}

/// Whether 0-ary definition `d` is a deterministic function of the current
/// state: not constant-level, and neither it nor anything it applies
/// (definitions, LET definitions) primes, uses UNCHANGED, ENABLED or a
/// temporal operator, or a nondeterministic or side-effecting builtin.
fn state_level_def(base: &InterpModel, d: DefId) -> bool {
    use tlx_sema::builtins::Builtin as B;
    use tlx_sema::hir::{DefKind, ExprKind as H, OpRef};
    let ip = &base.ip;
    let def = &ip.prog.defs[d.idx()];
    if !def.params.is_empty() || def.builtin.is_some() || ip.is_const_def(d) || ip.has_def_value(d) || !matches!(def.kind, DefKind::Op(_)) {
        return false;
    }
    let mut seen = HashSet::new();
    let mut stack = vec![d];
    while let Some(x) = stack.pop() {
        if !seen.insert(x) {
            continue;
        }
        let exprs: Vec<&hir::Expr> = match &ip.prog.defs[x.idx()].kind {
            DefKind::Op(b) => vec![b],
            DefKind::Fn { body, bounds } => std::iter::once(body).chain(bounds.iter().map(|b| &b.set)).collect(),
            DefKind::Pending => return false,
        };
        let mut ok = true;
        let mut todo: Vec<hir::Expr> = exprs.into_iter().cloned().collect();
        while let Some(e) = todo.pop() {
            match &*e.kind {
                H::Builtin { op, .. }
                    if op.is_volatile() || matches!(op, B::Prime | B::Unchanged | B::Enabled | B::Always | B::Eventually | B::LeadsTo) =>
                {
                    ok = false
                }
                H::BoxAction { .. } | H::AngleAction { .. } | H::Fairness { .. } => ok = false,
                H::Apply { op: OpRef::Def(y), .. } | H::OpArg(OpRef::Def(y)) => stack.push(ip.redirect_def(*y)),
                H::Apply { op: OpRef::Const(_), .. } => ok = false,
                H::Let { defs, .. } => stack.extend(defs.iter().copied()),
                _ => {}
            }
            tlx_sema::visit::children(&e, &mut |c| todo.push(c.clone()));
        }
        if !ok {
            return false;
        }
    }
    true
}

/// The components of the VIEW definition `d` if its body is a tuple literal.
pub(in crate::enc) fn view_tuple(base: &InterpModel, d: DefId) -> Option<&[hir::Expr]> {
    match &base.ip.prog.defs[d.idx()].kind {
        hir::DefKind::Op(body) => match &*body.kind {
            hir::ExprKind::Tuple(es) => Some(es),
            _ => None,
        },
        _ => None,
    }
}

/// The 0-ary definition named by the cfg's VIEW, if any.
pub(in crate::enc) fn view_def(base: &InterpModel) -> Option<DefId> {
    use tlx_sema::hir::{ExprKind as H, OpRef};
    match &*base.view_expr()?.kind {
        H::Apply { op: OpRef::Def(d), args } if args.is_empty() => Some(base.ip.redirect_def(*d)),
        _ => None,
    }
}
