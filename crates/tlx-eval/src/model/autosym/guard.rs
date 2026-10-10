//! Runtime obligations for choices observed only through a record field.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use tlx_sema::hir::*;
use tlx_value::{ops, Value};

use crate::env::Env;
use crate::error::EResult;
use crate::eval::Ctx;
use crate::{EvalError, Interp};

#[derive(Clone, Debug)]
pub struct Choice {
    pub vars: BoundVars,
    pub set: Expr,
    pub pred: Expr,
    pub field: Arc<str>,
}

/// Recognize a projection without allowing the chosen record to escape.
/// A LET definition is lexical and has no parameters or cfg redirection.
pub(super) fn projection(ip: &Interp, e: &Expr) -> Option<Choice> {
    let ExprKind::Field { record, field } = &*e.kind else { return None };
    let mut record = record;
    if let ExprKind::Apply { op: OpRef::Def(d), args } = &*record.kind {
        if !args.is_empty() || !ip.prog.def_is_let(*d) || ip.redirect_def(*d) != *d {
            return None;
        }
        let def = &ip.prog.defs[d.idx()];
        if !def.params.is_empty() {
            return None;
        }
        let DefKind::Op(body) = &def.kind else { return None };
        record = body;
    }
    let ExprKind::Choose { vars, set: Some(set), body } = &*record.kind else { return None };
    Some(Choice { vars: vars.clone(), set: set.clone(), pred: body.clone(), field: field.clone() })
}

pub(super) fn key(e: &Expr) -> usize {
    Arc::as_ptr(&e.kind) as usize
}

/// Installed once, before any reduced search. Separate code slots prevent
/// code cached during model loading from bypassing newly installed guards.
pub struct Guards {
    choices: HashMap<usize, Choice>,
    pub(crate) codes: Box<[OnceLock<crate::code::Code>]>,
    failed: AtomicBool,
    calls: tlx_base::counter::Striped,
    candidates: tlx_base::counter::Striped,
}

impl Interp {
    pub fn install_symmetry_guards(&self, choices: HashMap<usize, Choice>) {
        if choices.is_empty() {
            return;
        }
        let guards = Guards {
            choices,
            codes: (0..self.prog.arena_len()).map(|_| OnceLock::new()).collect(),
            failed: AtomicBool::new(false),
            calls: Default::default(),
            candidates: Default::default(),
        };
        assert!(self.symmetry_guards.set(guards).is_ok(), "symmetry guards installed twice");
    }

    pub fn symmetry_choice(&self, e: &Expr) -> Option<&Choice> {
        self.symmetry_guards.get()?.choices.get(&key(e))
    }

    pub fn symmetry_guard_failed(&self) -> bool {
        self.symmetry_guards.get().is_some_and(|g| g.failed.load(Ordering::Relaxed))
    }

    pub fn symmetry_guard_counts(&self) -> Option<(u64, u64)> {
        self.symmetry_guards.get().map(|g| (g.calls.get(), g.candidates.get()))
    }

    pub(crate) fn eval_symmetry_choice(self: &Arc<Self>, choice: &Choice, env: &Env, cx: &Ctx) -> EResult<Value> {
        let guards = self.symmetry_guards.get().expect("installed guard");
        guards.calls.add(1);
        let result = (|| {
            let set = self.eval(&choice.set, env, cx)?;
            let mut answer = None;
            for x in ops::enumerate_normalized(&set)? {
                guards.candidates.add(1);
                let env2 = self.bind_bound_vars(&choice.vars, x.clone(), env)?;
                if self.eval_bool(&choice.pred, &env2, cx)? {
                    let value = ops::field(&x, self.syms.get(&choice.field))?;
                    if let Some(old) = &answer {
                        if !tlx_value::order::eq_tlc(old, &value)? {
                            return Err(EvalError::new("automatic symmetry choice guard failed"));
                        }
                    }
                    answer = Some(value);
                }
            }
            answer.ok_or_else(|| EvalError::new("automatic symmetry choice has no satisfying candidate"))
        })();
        // Also covers errors in candidates TLC's ordered CHOOSE skips.
        // The fresh unreduced run decides whether they are real spec errors.
        if result.is_err() {
            guards.failed.store(true, Ordering::Relaxed);
        }
        result
    }
}
