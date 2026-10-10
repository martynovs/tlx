//! Lowering of the record, tuple and function forms.
//!
//! `expr` (in `super`) dispatches them here from the same arms, in the same
//! order, so diagnostics, recovery and evaluation order are unchanged
//! (CODE-03). The function application and record access share
//! `postfix_operand` with `super`, so it stays there.

use tree_sitter::Node;

use super::super::{kids, Lower};
use crate::ast::*;

impl Lower<'_> {
    /// `<<a, b, ...>>`.
    pub(super) fn tuple(&mut self, n: Node) -> Expr {
        let elems =
            kids(n).into_iter().filter(|k| k.kind() != "langle_bracket" && k.kind() != "rangle_bracket").map(|e| self.expr(e)).collect();
        self.mk(n, ExprKind::Tuple(elems))
    }

    /// `[x \in S |-> e]`
    pub(super) fn function_literal(&mut self, n: Node) -> Expr {
        let mut bounds = Vec::new();
        let mut body = None;
        for k in kids(n) {
            match k.kind() {
                "quantifier_bound" => bounds.push(self.bound(k)),
                "all_map_to" => {}
                _ => body = Some(self.expr(k)),
            }
        }
        let body = body.unwrap_or_else(|| self.err_expr(n, "function constructor without a body"));
        self.mk(n, ExprKind::FnConstruct { bounds, body })
    }

    /// `f[a, b]`
    pub(super) fn function_evaluation(&mut self, n: Node) -> Expr {
        let all = kids(n);
        let Some((f, args)) = all.split_first() else {
            return self.err_expr(n, "malformed function application");
        };
        let args: Vec<Expr> = args.iter().map(|a| self.expr(*a)).collect();
        self.postfix_operand(*f, &mut |s, func| s.mk(n, ExprKind::FnApp { func, args: args.clone() }))
    }

    /// `[S -> T]`
    pub(super) fn set_of_functions(&mut self, n: Node) -> Expr {
        let parts: Vec<Node> = kids(n).into_iter().filter(|k| k.kind() != "maps_to").collect();
        let [d, r] = parts.as_slice() else {
            return self.err_expr(n, "malformed function set");
        };
        let domain = self.expr(*d);
        let range = self.expr(*r);
        self.mk(n, ExprKind::FnSet { domain, range })
    }

    /// `[a |-> e, ...]` / `[a : S, ...]`
    pub(super) fn record(&mut self, n: Node) -> Expr {
        let parts: Vec<Node> = kids(n).into_iter().filter(|k| k.kind() != "all_map_to").collect();
        if !parts.len().is_multiple_of(2) {
            return self.err_expr(n, "malformed record");
        }
        let fields = parts.chunks(2).map(|c| (self.ident(c[0]), self.expr(c[1]))).collect();
        if n.kind() == "record_literal" {
            self.mk(n, ExprKind::Record(fields))
        } else {
            self.mk(n, ExprKind::RecordSet(fields))
        }
    }

    /// `r.f`
    pub(super) fn record_value(&mut self, n: Node) -> Expr {
        let parts = kids(n);
        let [r, f] = parts.as_slice() else {
            return self.err_expr(n, "malformed record field access");
        };
        let field = self.ident(*f);
        self.postfix_operand(*r, &mut |s, record| s.mk(n, ExprKind::Field { record, field: field.clone() }))
    }
}
