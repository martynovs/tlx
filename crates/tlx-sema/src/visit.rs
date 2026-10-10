//! Generic traversal of HIR expressions.

use std::collections::HashMap;

use crate::hir::*;

/// The direct sub-expressions of an `ExprKind`, shared by `children` and
/// `children_mut` (`$iter` is `iter` / `iter_mut`, `$m` an optional `mut`).
macro_rules! for_each_child {
    ($kind:expr, $f:ident, $iter:ident $(, $m:tt)?) => {
        match $kind {
            ExprKind::Num(_)
            | ExprKind::Decimal(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Var(_)
            | ExprKind::Const(_)
            | ExprKind::Local(_)
            | ExprKind::OpArg(_)
            | ExprKind::At => {}
            ExprKind::Apply { args, .. } | ExprKind::Builtin { args, .. } => args.$iter().for_each(&mut *$f),
            ExprKind::Lambda { body, .. } => $f(body),
            ExprKind::InstVar { subst, .. } => $f(subst),
            ExprKind::And(es) | ExprKind::Or(es) | ExprKind::SetEnum(es) | ExprKind::Tuple(es) => es.$iter().for_each(&mut *$f),
            ExprKind::Quant { bounds: bs, body, .. } | ExprKind::FnConstruct { bounds: bs, body } => {
                for b in bs.$iter() {
                    $f(&$($m)? b.set);
                }
                $f(body);
            }
            ExprKind::QuantUnbounded { body, .. } | ExprKind::TemporalQuant { body, .. } => $f(body),
            ExprKind::Choose { set, body, .. } => {
                if let Some(s) = set {
                    $f(s);
                }
                $f(body);
            }
            ExprKind::If { cond, then, els } => {
                $f(cond);
                $f(then);
                $f(els);
            }
            ExprKind::Case { arms, other } => {
                for (g, v) in arms {
                    $f(g);
                    $f(v);
                }
                if let Some(o) = other {
                    $f(o);
                }
            }
            ExprKind::Let { body, .. } => $f(body),
            ExprKind::SetFilter { bound, pred } => {
                $f(&$($m)? bound.set);
                $f(pred);
            }
            ExprKind::SetMap { expr, bounds: bs } => {
                for b in bs.$iter() {
                    $f(&$($m)? b.set);
                }
                $f(expr);
            }
            ExprKind::FnApp { func, args } => {
                $f(func);
                args.$iter().for_each(&mut *$f);
            }
            ExprKind::FnSet { domain, range } => {
                $f(domain);
                $f(range);
            }
            ExprKind::Record(fs) | ExprKind::RecordSet(fs) => fs.$iter().for_each(|(_, v)| $f(v)),
            ExprKind::Field { record, .. } => $f(record),
            ExprKind::Except { base, updates } => {
                $f(base);
                for (keys, v) in updates {
                    for k in keys {
                        if let ExceptKey::Index(ix) = k {
                            ix.$iter().for_each(&mut *$f);
                        }
                    }
                    $f(v);
                }
            }
            ExprKind::BoxAction { action, sub } | ExprKind::AngleAction { action, sub } => {
                $f(action);
                $f(sub);
            }
            ExprKind::Fairness { sub, action, enabled, .. } => {
                $f(sub);
                $f(action);
                if let Some(x) = enabled {
                    $f(x);
                }
            }
        }
    };
}

/// Call `f` on every direct sub-expression of `e`.
pub fn children_mut(e: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
    for_each_child!(std::sync::Arc::make_mut(&mut e.kind), f, iter_mut, mut)
}

/// Call `f` on every direct sub-expression of `e` (read-only).
pub fn children(e: &Expr, f: &mut impl FnMut(&Expr)) {
    for_each_child!(&*e.kind, f, iter)
}

/// Replace occurrences of the given locals by expressions (capture-free since
/// every binding site has a unique `LocalId`). LET definition bodies are
/// separate `Def`s and are not rewritten.
pub fn subst_locals(e: &mut Expr, map: &HashMap<LocalId, Expr>) {
    if let ExprKind::Local(l) = &*e.kind {
        if let Some(r) = map.get(l) {
            *e = r.clone();
        }
        return;
    }
    children_mut(e, &mut |c| subst_locals(c, map));
}
