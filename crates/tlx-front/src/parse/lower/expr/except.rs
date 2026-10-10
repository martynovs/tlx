//! Lowering of `EXCEPT` and its update paths.
//!
//! `expr` (in `super`) dispatches the `except` node to `except` here from the
//! same arm, so diagnostics, recovery and update order are unchanged
//! (CODE-03). The node walks the update specifiers left to right, exactly as
//! the arm did.

use tree_sitter::Node;

use super::super::{kids, Lower};
use crate::ast::*;

impl Lower<'_> {
    /// `[f EXCEPT ![a] = e, ...]`
    pub(super) fn except(&mut self, n: Node) -> Expr {
        let base = self.field_expr(n, "expr_to_update");
        let mut updates = Vec::new();
        for u in kids(n).into_iter().filter(|k| k.kind() == "except_update") {
            let mut path = Vec::new();
            if let Some(spec) = kids(u).into_iter().find(|k| k.kind() == "except_update_specifier") {
                for step in kids(spec) {
                    match step.kind() {
                        "except_update_record_field" => match kids(step).into_iter().next() {
                            Some(f) => path.push(ExceptKey::Field(self.ident(f))),
                            None => self.err(step, "EXCEPT field without a name"),
                        },
                        "except_update_fn_appl" => {
                            let args = kids(step).into_iter().map(|a| self.expr(a)).collect();
                            path.push(ExceptKey::Index(args))
                        }
                        k => self.err(step, format!("unexpected `{k}` in EXCEPT path")),
                    }
                }
            }
            let val = self.field_expr(u, "new_val");
            updates.push((path, val));
        }
        self.mk(n, ExprKind::Except { base, updates })
    }
}
