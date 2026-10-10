//! SANY's level checks on theorems and proofs (`TheoremNode.levelCheck`,
//! `NewSymbNode.levelCheck`): in the proof of a temporal-level goal, HAVE,
//! TAKE, WITNESS and CASE must be constant; the bounds of a temporal PICK
//! must be constant; the set of `NEW x \in S` cannot be temporal.
//!
//! Proof expressions are resolved only for their levels: identifiers
//! introduced by NEW, TAKE and PICK are taken as constants, and expressions
//! that cannot be resolved (e.g. ones using DEFINE or step names) are not
//! checked.

use tlx_front::ast::{self, Span};

use super::{err, Env, LocalBinding, Program, ROOT_CTX};

const TEMPORAL: u8 = 3;

impl Program {
    /// Check the theorems of the root module and the modules it extends.
    pub(super) fn check_proof_levels(&mut self) {
        let mut mods = vec![self.root_mod];
        let mut i = 0;
        while i < mods.len() {
            let ast = self.mods[mods[i]].ast.clone();
            for e in &ast.extends {
                if let Some(&em) = self.mod_by_name.get(&e.name) {
                    if !mods.contains(&em) {
                        mods.push(em);
                    }
                }
            }
            i += 1;
        }
        for m in mods {
            let ast = self.mods[m].ast.clone();
            for (_, pu) in &ast.proofs {
                if let ast::ProofUnit::Theorem { stmt, proof, .. } = pu {
                    let mut env = Env::new(ROOT_CTX, m);
                    let level = self.stmt_level(&mut env, stmt);
                    if let Some(p) = proof {
                        self.proof_levels(&mut env, p, level);
                    }
                }
            }
        }
    }

    /// The level of `e` (`None` if it cannot be resolved).
    fn level_of(&mut self, env: &mut Env, e: &ast::Expr) -> Option<u8> {
        let nerr = self.errors.len();
        let r = self.expr(env, e);
        self.drain_worklist();
        if self.errors.len() > nerr {
            self.errors.truncate(nerr);
            return None;
        }
        r.ok().map(|h| self.expr_level(&h))
    }

    fn bind_const(&mut self, env: &mut Env, id: &ast::Ident, arity: usize) {
        let l = self.new_local(id.name.clone(), arity, id.span);
        env.names.push((id.name.clone(), LocalBinding::Local(l, arity)));
    }

    /// The level of a statement (`None`: unknown); the NEW declarations of
    /// an ASSUME/PROVE stay bound in `env`.
    fn stmt_level(&mut self, env: &mut Env, s: &ast::Stmt) -> Option<u8> {
        match s {
            ast::Stmt::Expr(e) => self.level_of(env, e),
            ast::Stmt::AssumeProve(ap) => self.assume_prove_level(env, ap),
        }
    }

    fn assume_prove_level(&mut self, env: &mut Env, ap: &ast::AssumeProve) -> Option<u8> {
        let mut level = Some(0u8);
        let mut join = |l: Option<u8>| level = level.zip(l).map(|(a, b)| a.max(b));
        for a in &ap.assumes {
            match a {
                ast::Assumption::Expr(e) => join(self.level_of(env, e)),
                ast::Assumption::New(nd) => {
                    if let Some(s) = &nd.set {
                        let l = self.level_of(env, s);
                        if l == Some(TEMPORAL) {
                            self.errors.push(err(nd.span, "Level error:\nTemporal formula used as set."));
                        }
                        join(l);
                    }
                    self.bind_const(env, &nd.decl.name, nd.decl.arity);
                }
                ast::Assumption::AssumeProve(inner) => {
                    let mark = env.names.len();
                    join(self.assume_prove_level(env, inner));
                    env.names.truncate(mark);
                }
            }
        }
        join(self.level_of(env, &ap.prove));
        level
    }

    /// The maximum level of the bounds of `b` (binding its identifiers).
    fn binders_level(&mut self, env: &mut Env, b: &ast::Binders) -> (Option<u8>, Vec<(Span, Option<u8>)>) {
        let mut sets = Vec::new();
        match b {
            ast::Binders::Bounded(bs) => {
                for x in bs {
                    let l = self.level_of(env, &x.set);
                    sets.push((x.set.span, l));
                }
                for x in bs {
                    let (ast::BoundVars::Many(vs) | ast::BoundVars::Tuple(vs)) = &x.vars;
                    for v in vs {
                        self.bind_const(env, v, 0);
                    }
                }
            }
            ast::Binders::Unbounded(vs) => {
                for v in vs {
                    self.bind_const(env, v, 0);
                }
            }
        }
        let level = sets.iter().try_fold(0u8, |a, (_, l)| l.map(|l| a.max(l)));
        (level, sets)
    }

    /// Check a proof whose goal has level `goal`.
    fn proof_levels(&mut self, env: &mut Env, p: &ast::Proof, goal: Option<u8>) {
        let ast::Proof::Steps(steps) = p else { return };
        let mark = env.names.len();
        let temporal = goal == Some(TEMPORAL);
        for st in steps {
            use ast::StepKind as S;
            let smark = env.names.len();
            let non_constant = |c: &mut Self, l: Option<u8>, msg: &str| {
                if temporal && l.is_some_and(|l| l > 0) {
                    c.errors.push(err(st.span, msg));
                }
            };
            match &st.kind {
                S::Assert(s) => {
                    let l = self.stmt_level(env, s);
                    if let Some(p) = &st.proof {
                        self.proof_levels(env, p, l);
                    }
                    env.names.truncate(smark);
                }
                S::Suffices(s) => {
                    let l = self.stmt_level(env, s);
                    let keep = env.names.split_off(smark);
                    if let Some(p) = &st.proof {
                        self.proof_levels(env, p, l);
                    }
                    env.names.extend(keep);
                }
                S::Have(e) => {
                    let l = self.level_of(env, e);
                    non_constant(self, l, "Non-constant TAKE, WITNESS, or HAVE for temporal goal.");
                }
                S::Witness(es) => {
                    let l = es.iter().try_fold(0u8, |a, e| self.level_of(env, e).map(|l| a.max(l)));
                    non_constant(self, l, "Non-constant TAKE, WITNESS, or HAVE for temporal goal.");
                }
                S::Take(b) => {
                    let (l, _) = self.binders_level(env, b);
                    non_constant(self, l, "Non-constant TAKE, WITNESS, or HAVE for temporal goal.");
                }
                S::Case(e) => {
                    let l = self.level_of(env, e);
                    non_constant(self, l, "Non-constant CASE for temporal goal.");
                    if let Some(p) = &st.proof {
                        // The goal of a CASE proof is temporal if the CASE or
                        // the current goal is.
                        let g = if temporal { Some(TEMPORAL) } else { l };
                        self.proof_levels(env, p, g);
                    }
                }
                S::Pick(b, e) => {
                    let (bl, sets) = self.binders_level(env, b);
                    let el = self.level_of(env, e);
                    let l = bl.zip(el).map(|(a, b)| a.max(b));
                    if l == Some(TEMPORAL) {
                        for (span, sl) in sets {
                            if sl.is_some_and(|sl| sl > 0) {
                                self.errors.push(err(span, "Non-constant bound of temporal PICK."));
                            }
                        }
                    }
                    let keep = env.names.split_off(smark);
                    if let Some(p) = &st.proof {
                        self.proof_levels(env, p, l);
                    }
                    env.names.extend(keep);
                }
                S::Qed => {
                    if let Some(p) = &st.proof {
                        // SANY checks the QED proof as part of the enclosing goal.
                        self.proof_levels(env, p, if temporal { goal } else { Some(0) });
                    }
                }
                S::Define(_) | S::UseOrHide(_) | S::Instance(_) => {}
            }
        }
        env.names.truncate(mark);
    }
}
