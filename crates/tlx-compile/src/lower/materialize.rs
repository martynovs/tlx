//! Definition materialization in predicates: a 0-ary state-level
//! definition whose encoding was inferred (`Typing::def_sid`) is computed
//! once per predicate evaluation and bound to a temp shared by all its uses
//! (separately for primed and unprimed uses). The bindings are hoisted
//! around the predicate body in dependency order.

use std::collections::{HashMap, HashSet};

use tlx_eval::env::Env;
use tlx_eval::Interp;
use tlx_sema::hir::{self, DefKind, OpRef};

use super::{let_in, temp_ref, to_val, LEnv, Lowerer, St};
use crate::binding::{Flow, Policy, Scope, State};
use crate::ir::*;

/// Materialized definitions of the predicate being lowered.
#[derive(Default, Clone)]
pub(super) struct Materialized {
    /// (definition, primed) -> temp holding its value.
    temps: HashMap<(hir::DefId, bool), Temp>,
    /// Bindings in dependency order.
    lets: Vec<(Temp, Expr)>,
}

impl Lowerer<'_> {
    /// Hoist the materialized definitions of a predicate around its body.
    /// The interpreter fallbacks in the body that may evaluate one of them
    /// get its values (current and next state), so that the interpreter
    /// does not compute them again (its `DefCache` hits).
    pub(super) fn wrap_materialized(&mut self, mut body: Expr) -> Expr {
        let Some(m) = self.mat.take() else { return body };
        let mut seeds: Vec<(hir::DefId, bool, Expr)> = Vec::new();
        for ((d, primed), t) in &m.temps {
            if self.ip.is_state_cacheable(*d) {
                if let Some((_, x)) = m.lets.iter().find(|(u, _)| u == t) {
                    seeds.push((*d, *primed, temp_ref(x.ty, *t)));
                }
            }
        }
        if !seeds.is_empty() {
            seeds.sort_by_key(|(d, primed, _)| (d.0, *primed));
            self.seed_interps(&mut body, &seeds);
        }
        let_in(m.lets.into_iter().rev(), body)
    }

    /// Add to each interpreter fallback in `e` the values of the definitions
    /// of `seeds` (current or next state) that it may evaluate (see
    /// `def_value_key`).
    fn seed_interps(&self, e: &mut Expr, seeds: &[(hir::DefId, bool, Expr)]) {
        e.for_each_child_mut(&mut |c| self.seed_interps(c, seeds));
        if let ExprKind::Interp { expr, binds, deferred, env } = &mut *e.kind {
            let mut targets: Vec<hir::DefId> = seeds.iter().map(|(d, _, _)| *d).collect();
            targets.dedup();
            let found = reachable(self.ip, expr, env, deferred, &targets);
            for (d, primed, x) in seeds {
                if found.contains(d) {
                    binds.push((tlx_eval::env::def_value_key(*d, *primed), to_val(x.clone())));
                }
            }
        }
    }

    /// Whether `t` holds a materialized definition (which the interpreter
    /// computes as well, memoized per state).
    pub(super) fn is_materialized_temp(&self, t: Temp) -> bool {
        self.mat.as_ref().is_some_and(|m| m.temps.values().any(|x| *x == t))
    }

    /// A reference to a materialized 0-ary definition: its value is
    /// computed once per predicate evaluation (in the encoding inferred for
    /// it) and bound to a temp.
    pub(super) fn materialized(&mut self, d: hir::DefId, st: &St) -> Option<Expr> {
        self.mat.as_ref()?;
        let d = self.ip.redirect_def(d);
        let sid = *self.typing.def_sid.get(&d)?;
        let key = (d, self.primed);
        if let Some(t) = self.mat.as_ref()?.temps.get(&key) {
            return Some(temp_ref(Ty::Enc(sid), *t));
        }
        let DefKind::Op(body) = &self.ip.prog.defs[d.idx()].kind else { return None };
        let body = body.clone();
        let saved_at = self.at.take();
        self.inline_stack.push(d);
        let v = self.expr_into(&body, &LEnv::new(Env::default()), st, sid);
        self.inline_stack.pop();
        self.at = saved_at;
        let v = self.coerce_enc(v, sid);
        let t = self.temp(Ty::Enc(sid));
        let mat = self.mat.as_mut()?;
        mat.temps.insert(key, t);
        mat.lets.push((t, v));
        Some(temp_ref(Ty::Enc(sid), t))
    }
}

/// Which of `targets` (redirected definitions) `e` — evaluated in `env`, with
/// the fallback's `deferred` arguments — may evaluate, through the definitions
/// it calls, the operators and by-name arguments bound to its parameters and
/// locals (in the fallback's environment: a LAMBDA or an operator passed in,
/// as the walk follows them), and the definitions constants are bound to.
fn reachable(ip: &Interp, e: &hir::Expr, env: &Env, deferred: &[Deferred], targets: &[hir::DefId]) -> Vec<hir::DefId> {
    let mut scan = DefReach { targets, seen: HashSet::new(), locals: HashSet::new(), found: Vec::new() };
    let mut s = State::new(ip, Scope::Single, env.clone());
    s.walk(&mut scan, e, 0);
    s.walk_deferred(&mut scan, deferred, 0);
    scan.found
}

/// The reachability policy of the shared environment-aware walk
/// (`crate::binding`): which of `targets` the expression may evaluate. It
/// counts structural depth (`depth > 64` gives up), replaces the environment
/// at a closure or a definition body (a single current environment), and stops
/// the walk as soon as every target was found.
struct DefReach<'t> {
    targets: &'t [hir::DefId],
    seen: HashSet<hir::DefId>,
    /// Locals already followed.
    locals: HashSet<hir::LocalId>,
    found: Vec<hir::DefId>,
}

impl DefReach<'_> {
    fn done(&self) -> bool {
        self.found.len() == self.targets.len()
    }
}

impl Policy for DefReach<'_> {
    fn node(&mut self, _s: &mut State<'_>, _e: &hir::Expr, _depth: usize) -> Flow {
        if self.done() {
            Flow::Done
        } else {
            Flow::Children
        }
    }

    fn def(&mut self, _s: &mut State<'_>, d: hir::DefId) -> bool {
        if !self.seen.insert(d) {
            return false;
        }
        if self.targets.contains(&d) {
            self.found.push(d);
        }
        true
    }

    fn local(&mut self, _s: &mut State<'_>, l: hir::LocalId, depth: usize) -> bool {
        depth <= 64 && self.locals.insert(l)
    }

    /// A constant bound by the cfg to a definition (`C <- Def`).
    fn constant(&mut self, s: &mut State<'_>, c: hir::ConstId, _depth: usize) {
        if let Some(OpRef::Def(d)) = s.ip.const_op_ref(c) {
            s.def(self, d);
        } else if let Some(d) = s.ip.const_def(c) {
            s.def(self, d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Typing;
    use std::sync::Arc;

    fn lowerer(model: &tlx_eval::model::InterpModel) -> Lowerer<'_> {
        Lowerer::with_typing(&model.ip, Typing::new(Arc::new(tlx_layout::Layout::new(vec![tlx_layout::Shape::Dyn]))))
    }

    /// The fallback `Expr` of `name`, as `seed_interps` sees one.
    fn fallback(model: &tlx_eval::model::InterpModel, name: &str) -> Expr {
        let def = model.ip.prog.defs.iter().find(|d| &*d.name == name).unwrap();
        let DefKind::Op(body) = &def.kind else { panic!("operator") };
        Expr::new(Ty::Bool, ExprKind::Interp { expr: body.clone(), binds: vec![], deferred: vec![], env: Env::default() })
    }

    fn def(model: &tlx_eval::model::InterpModel, name: &str) -> hir::DefId {
        let d = model.ip.prog.defs.iter().position(|x| &*x.name == name).unwrap();
        hir::DefId(d as u32)
    }

    fn shared(lw: &Lowerer<'_>, e: &Expr, targets: &[hir::DefId]) -> Vec<hir::DefId> {
        match &*e.kind {
            ExprKind::Interp { expr, deferred, env, .. } => reachable(lw.ip, expr, env, deferred, targets),
            _ => unreachable!("fallback"),
        }
    }

    /// Definition reachability the shared environment-aware walk
    /// (`crate::binding`) must keep: `LET` and recursion, an operator
    /// argument applied inside a closure, and a `CONSTANT` the cfg binds to
    /// a definition.
    #[test]
    fn reachability_covers_let_recursion_operator_arguments_and_overrides() {
        let source = tlx_testkit::module(
            "MaterializeHIR",
            r#"
EXTENDS Naturals
VARIABLE x, y
CONSTANT C
RECURSIVE Rec(_)
Rec(n) == IF n = 0 THEN x ELSE Rec(n - 1)
Higher(Op(_)) == Op(x)
Override == y
Main == LET F(n) == Rec(n) IN Higher(LAMBDA n: F(n)) = x
LetConst == LET K == C IN K = x
Init == x = 0
Next == x' = x + 1
"#,
        );
        let model =
            tlx_testkit::spec::load(&source, "CONSTANT C <- Override\nINIT Init\nNEXT Next\nINVARIANT Main LetConst Higher Rec Override\n");
        let lw = lowerer(&model);
        // The fixtures reach the paths the batch names: recursion and a LET
        // operator applied inside a closure, and `C <- Override`.
        let rec = def(&model, "Rec");
        assert!(shared(&lw, &fallback(&model, "Main"), &[rec]).contains(&rec));
        let over = def(&model, "Override");
        assert!(shared(&lw, &fallback(&model, "LetConst"), &[over]).contains(&over));
        // An unreachable target stays unreached.
        let absent = def(&model, "Override");
        assert!(!shared(&lw, &fallback(&model, "Rec"), &[absent]).contains(&absent));
    }
}
