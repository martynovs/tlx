//! Recognizers for the two fold idioms whose choice does not depend on
//! the order of a set's elements, and that the analysis therefore proves
//! order-independent: a CHOOSE with at most one candidate
//! (`unique_choice`) and a recursion over a set that CHOOSEs the next
//! element and folds it (`fold_chooses`).

use std::collections::HashSet;
use std::sync::Arc;

use tlx_sema::builtins::Builtin as B;
use tlx_sema::hir::*;

use crate::interp::Interp;

/// Whether `CHOOSE x \in S : P` has at most one candidate: P has a
/// conjunct `x = e` (or `e = x`) with `e` not mentioning `x`, or P is
/// `Q(x) /\ \A y \in S : Q(y) => y = x` (`x` is the only element
/// satisfying Q).
pub(super) fn unique_choice(vars: &BoundVars, set: &Expr, body: &Expr) -> bool {
    let BoundVars::Many(ids) = vars else { return false };
    let [x] = ids.as_slice() else { return false };
    let mut conjuncts = Vec::new();
    flatten_and(body, &mut conjuncts);
    let is_local = |e: &Expr, v: LocalId| matches!(&*e.kind, ExprKind::Local(l) if *l == v);
    let equation = conjuncts.iter().any(|c| match &*c.kind {
        ExprKind::Builtin { op: B::Eq, args } if args.len() == 2 => {
            (is_local(&args[0], *x) && !mentions_local(&args[1], *x)) || (is_local(&args[1], *x) && !mentions_local(&args[0], *x))
        }
        _ => false,
    });
    if equation {
        return true;
    }
    // Q(x) /\ \A y \in S : Q(y) => y = x.
    let [q, all] = conjuncts.as_slice() else { return false };
    let ExprKind::Quant { kind: QuantKind::Forall, bounds, body: imp } = &*all.kind else { return false };
    let [Bound { vars: BoundVars::Many(ys), set: s2 }] = bounds.as_slice() else { return false };
    let [y] = ys.as_slice() else { return false };
    let ExprKind::Builtin { op: B::Implies, args } = &*imp.kind else { return false };
    let [qy, eq] = args.as_slice() else { return false };
    let ExprKind::Builtin { op: B::Eq, args: e } = &*eq.kind else { return false };
    let names_both = e.len() == 2 && ((is_local(&e[0], *y) && is_local(&e[1], *x)) || (is_local(&e[0], *x) && is_local(&e[1], *y)));
    names_both && !mentions_local(s2, *x) && same(s2, set, None) && same(qy, q, Some((*y, *x)))
}

/// Structural equality of `a` and `b`, with local `r.0` of `a` standing for
/// `r.1` of `b`; false for forms it does not compare.
fn same(a: &Expr, b: &Expr, r: Option<(LocalId, LocalId)>) -> bool {
    let all = |xs: &[Expr], ys: &[Expr]| xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| same(x, y, r));
    match (&*a.kind, &*b.kind) {
        (ExprKind::Local(x), ExprKind::Local(y)) => x == y || r == Some((*x, *y)),
        (ExprKind::Var(x), ExprKind::Var(y)) => x == y,
        (ExprKind::Const(x), ExprKind::Const(y)) => x == y,
        (ExprKind::Num(x), ExprKind::Num(y)) => x == y,
        (ExprKind::Str(x), ExprKind::Str(y)) => x == y,
        (ExprKind::Bool(x), ExprKind::Bool(y)) => x == y,
        (ExprKind::Apply { op: o, args: xs }, ExprKind::Apply { op: p, args: ys }) => o == p && all(xs, ys),
        (ExprKind::Builtin { op: o, args: xs }, ExprKind::Builtin { op: p, args: ys }) => o == p && all(xs, ys),
        (ExprKind::And(xs), ExprKind::And(ys)) | (ExprKind::Or(xs), ExprKind::Or(ys)) => all(xs, ys),
        (ExprKind::SetEnum(xs), ExprKind::SetEnum(ys)) | (ExprKind::Tuple(xs), ExprKind::Tuple(ys)) => all(xs, ys),
        (ExprKind::FnApp { func: f, args: xs }, ExprKind::FnApp { func: g, args: ys }) => same(f, g, r) && all(xs, ys),
        (ExprKind::Field { record: x, field: f }, ExprKind::Field { record: y, field: g }) => f == g && same(x, y, r),
        _ => false,
    }
}

fn flatten_and<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    match &*e.kind {
        ExprKind::And(es) => es.iter().for_each(|x| flatten_and(x, out)),
        _ => out.push(e),
    }
}

fn mentions_local(e: &Expr, x: LocalId) -> bool {
    if matches!(&*e.kind, ExprKind::Local(l) if *l == x) {
        return true;
    }
    let mut found = false;
    tlx_sema::visit::children(e, &mut |c| found = found || mentions_local(c, x));
    found
}

/// The CHOOSE expressions (by the address of their `ExprKind`) that pick
/// the next element of a fold written as a recursion over a set,
///
/// ```text
/// f[S \in T] == ... LET x == CHOOSE y \in S : TRUE IN e (+) f[S \ {x}] ...
/// F(.., S, ..) == ... LET x == CHOOSE y \in S : TRUE IN e (+) F(.., S \ {x}, ..) ...
/// ```
///
/// with `(+)` commutative and associative (`+`, `*`, `\cup`, `\cap`) and `e`
/// mentioning neither `S` nor the recursion: the result is the fold of `e`
/// over the elements of S whichever element CHOOSE picks, so the choice
/// does not depend on the elements' order.
pub(super) fn fold_chooses(ip: &Interp) -> HashSet<usize> {
    let mut out = HashSet::new();
    for (i, def) in ip.prog.defs.iter().enumerate() {
        let d = DefId(i as u32);
        match &def.kind {
            DefKind::Fn { bounds, body } => {
                if let [Bound { vars: BoundVars::Many(v), .. }] = bounds.as_slice() {
                    if let [s] = v.as_slice() {
                        find_folds(ip, d, &Recursion::Fn(*s), body, &mut out);
                    }
                }
            }
            DefKind::Op(body) => {
                for (k, s) in def.params.iter().enumerate() {
                    find_folds(ip, d, &Recursion::Op(&def.params, k, *s), body, &mut out);
                }
            }
            DefKind::Pending => {}
        }
    }
    out
}

/// How a definition applies itself to the rest of the set.
enum Recursion<'a> {
    /// `f[S \ {x}]`, S the function's bound variable.
    Fn(LocalId),
    /// `F(p1, .., S \ {x}, .., pn)`: the parameters, S's position, S.
    Op(&'a [LocalId], usize, LocalId),
}

impl Recursion<'_> {
    fn set(&self) -> LocalId {
        match self {
            Recursion::Fn(s) | Recursion::Op(_, _, s) => *s,
        }
    }

    /// Whether `e` applies definition `d` to the set minus `{x}`.
    fn is_call(&self, d: DefId, x: DefId, e: &Expr) -> bool {
        let minus = |a: &Expr| match &*a.kind {
            ExprKind::Builtin { op: B::SetMinus, args } if args.len() == 2 => {
                matches!(&*args[0].kind, ExprKind::Local(l) if *l == self.set())
                    && matches!(&*args[1].kind, ExprKind::SetEnum(es) if matches!(es.as_slice(), [y] if matches!(&*y.kind, ExprKind::Apply { op: OpRef::Def(p), args } if *p == x && args.is_empty())))
            }
            _ => false,
        };
        match (self, &*e.kind) {
            (Recursion::Fn(_), ExprKind::FnApp { func, args }) => {
                matches!(&*func.kind, ExprKind::Apply { op: OpRef::Def(f), args: a } if *f == d && a.is_empty())
                    && matches!(args.as_slice(), [a] if minus(a))
            }
            (Recursion::Op(params, k, _), ExprKind::Apply { op: OpRef::Def(f), args }) => {
                *f == d
                    && args.len() == params.len()
                    && args.iter().enumerate().all(|(j, a)| {
                        if j == *k {
                            minus(a)
                        } else {
                            matches!(&*a.kind, ExprKind::Local(l) if *l == params[j])
                        }
                    })
            }
            _ => false,
        }
    }
}

fn find_folds(ip: &Interp, d: DefId, rec: &Recursion, e: &Expr, out: &mut HashSet<usize>) {
    if let ExprKind::Let { defs, body } = &*e.kind {
        if let ExprKind::Builtin { op: B::Plus | B::Mul | B::Cup | B::Cap, args } = &*body.kind {
            for &x in defs {
                let DefKind::Op(c) = &ip.prog.defs[x.idx()].kind else { continue };
                let ExprKind::Choose { vars: BoundVars::Many(v), set: Some(set), body: pred } = &*c.kind else { continue };
                let picks_any = v.len() == 1 && matches!(&*pred.kind, ExprKind::Bool(true));
                if !picks_any || !matches!(&*set.kind, ExprKind::Local(l) if *l == rec.set()) || !ip.prog.defs[x.idx()].params.is_empty() {
                    continue;
                }
                let [a, b] = args.as_slice() else { continue };
                for (call, elem) in [(a, b), (b, a)] {
                    // (`elem` applies `x`, whose CHOOSE mentions S.)
                    if rec.is_call(d, x, call) && !mentions(ip, elem, rec.set(), d, &mut vec![x]) {
                        out.insert(Arc::as_ptr(&c.kind) as *const () as usize);
                    }
                }
            }
        }
    }
    tlx_sema::visit::children(e, &mut |c| find_folds(ip, d, rec, c, out));
}

/// Whether `e` (with the bodies of the LET definitions it applies)
/// mentions local `s` or applies definition `d`.
fn mentions(ip: &Interp, e: &Expr, s: LocalId, d: DefId, seen: &mut Vec<DefId>) -> bool {
    match &*e.kind {
        ExprKind::Local(l) if *l == s => return true,
        ExprKind::Apply { op: OpRef::Def(f), .. } | ExprKind::OpArg(OpRef::Def(f)) => {
            if *f == d {
                return true;
            }
            if ip.prog.def_is_let(*f) && !seen.contains(f) {
                seen.push(*f);
                match &ip.prog.defs[f.idx()].kind {
                    DefKind::Op(b) | DefKind::Fn { body: b, .. } if mentions(ip, b, s, d, seen) => return true,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    let mut found = false;
    tlx_sema::visit::children(e, &mut |c| found = found || mentions(ip, c, s, d, seen));
    found
}
