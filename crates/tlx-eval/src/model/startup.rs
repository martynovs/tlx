//! Checks of constant-valued configuration entries, before state generation.

use super::{cfg_true_const, def_ref, spec};
use crate::{
    env::Env,
    error::{EResult, EvalError},
    eval::Ctx,
    interp::Interp,
};
use std::sync::Arc;
use tlx_sema::hir::{ExprKind, OpRef};
use tlx_value::Value;

/// TLC's SpecProcessor replaces successfully evaluated constant definitions
/// with values. Failed evaluations remain definitions and are checked later
/// on states; in particular, do not turn their errors into startup errors.
pub(super) fn check(ip: &Arc<Interp>) -> EResult<()> {
    for (kind, names) in
        [("invariant", &ip.cfg.invariants), ("constraint", &ip.cfg.constraints), ("action constraint", &ip.cfg.action_constraints)]
    {
        for name in names {
            if cfg_true_const(ip, name) {
                continue;
            }
            let e = def_ref(ip, name)?;
            if let ExprKind::Apply { op: OpRef::Def(d), .. } = &*e.kind {
                if spec::tlc_has_value(ip, *d) {
                    if let Ok(v) = ip.eval(&e, &Env::default(), &Ctx::NONE) {
                        if v != Value::Bool(true) {
                            return Err(EvalError::new(format!("The {kind} of {name} is equal to {v}.")));
                        }
                    }
                }
            }
        }
    }
    // Spec.getViewSpec rejects a value, but allows a constant-level
    // definition whose evaluation failed (it has not become a value).
    if let Some(name) = &ip.cfg.view {
        let e = def_ref(ip, name)?;
        if let ExprKind::Apply { op: OpRef::Def(d), .. } = &*e.kind {
            if spec::tlc_has_value(ip, *d) && ip.eval(&e, &Env::default(), &Ctx::NONE).is_ok() {
                return Err(EvalError::new(format!("The view function {name} cannot be a constant.")));
            }
        }
    }
    Ok(())
}
