//! Lowering of the binding forms that open a nested scope.
//!
//! `expr` (in `super`) dispatches the quantifiers, `CHOOSE`, the set
//! comprehension generators and `LET` here from the same arms, in the same
//! order, so diagnostics, recovery and evaluation order are unchanged
//! (CODE-03). The bound-variable primitives the rest of the module shares
//! (`bound_vars`, `bound`) stay in `super` (CODE-23).

use tree_sitter::Node;

use super::super::{field, field_kids, kids, Lower};
use crate::ast::*;

impl Lower<'_> {
    /// `LAMBDA x, y: body`.
    pub(super) fn lambda(&mut self, n: Node) -> Expr {
        let ks = kids(n);
        let Some((last, ids)) = ks.split_last() else {
            return self.err_expr(n, "empty LAMBDA");
        };
        let params = ids.iter().map(|i| self.ident(*i)).collect();
        let body = self.expr(*last);
        self.mk(n, ExprKind::Lambda { params, body })
    }

    /// `unbounded_quantification`: `\A x, y: body`.
    pub(super) fn unbounded_quant(&mut self, n: Node) -> Expr {
        let q = field(n, "quantifier");
        let kind = quant_kind(q);
        let temporal = matches!(q.map(|q| q.kind()), Some("temporal_forall" | "temporal_exists"));
        let vars = field_kids(n, "intro").into_iter().map(|i| self.ident(i)).collect();
        let body = self.field_expr(n, "expression");
        if temporal {
            self.mk(n, ExprKind::TemporalQuant { kind, vars, body })
        } else {
            self.mk(n, ExprKind::QuantUnbounded { kind, vars, body })
        }
    }

    /// `bounded_quantification`: `\A x, y \in S: body`.
    pub(super) fn bounded_quant(&mut self, n: Node) -> Expr {
        let kind = quant_kind(field(n, "quantifier"));
        let bounds = field_kids(n, "bound").into_iter().map(|b| self.bound(b)).collect();
        let body = self.field_expr(n, "expression");
        self.mk(n, ExprKind::Quant { kind, bounds, body })
    }

    /// `CHOOSE x \in S: body`.
    pub(super) fn choose(&mut self, n: Node) -> Expr {
        let intro = field_kids(n, "intro");
        let vars = self.bound_vars(&intro);
        let set = field(n, "set").map(|s| self.expr(s));
        let body = self.field_expr(n, "expression");
        self.mk(n, ExprKind::Choose { vars, set, body })
    }

    /// `{x \in S : pred}`.
    pub(super) fn set_filter(&mut self, n: Node) -> Expr {
        let bound = match field(n, "generator") {
            Some(g) => self.bound(g),
            None => Bound { vars: BoundVars::Many(vec![]), set: self.err_expr(n, "set filter without a bound") },
        };
        let pred = self.field_expr(n, "filter");
        self.mk(n, ExprKind::SetFilter { bound, pred })
    }

    /// `{expr : x \in S}`.
    pub(super) fn set_map(&mut self, n: Node) -> Expr {
        let expr = self.field_expr(n, "map");
        let bounds = field_kids(n, "generator").into_iter().map(|b| self.bound(b)).collect();
        self.mk(n, ExprKind::SetMap { expr, bounds })
    }

    /// `LET defs IN body`.
    pub(super) fn let_expr(&mut self, n: Node) -> Expr {
        let mut defs = Vec::new();
        for d in field_kids(n, "definitions") {
            match d.kind() {
                "recursive_declaration" => self.unit(d, &mut defs),
                _ => defs.push(self.definition(d, false, self.span(d))),
            }
        }
        let body = self.field_expr(n, "expression");
        self.mk(n, ExprKind::Let { defs, body })
    }
}

fn quant_kind(n: Option<Node>) -> QuantKind {
    match n.map(|q| q.kind()) {
        Some("exists") | Some("temporal_exists") => QuantKind::Exists,
        _ => QuantKind::Forall,
    }
}
