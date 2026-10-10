//! Instance-qualified names (`I!J(a)!Op(b)`) and subexpression references
//! (`Def!2!lab`, `Def!:`).

use tree_sitter::Node;

use super::{field, kids, Lower};
use crate::ast::*;

/// An element of a subexpression / instance path: `Name(args)!` or a selector.
enum PathItem {
    Comp(NamePrefix),
    Sel(Selector),
}

impl Lower<'_> {
    /// Lower one element of a `subexpr_prefix` (or the trailing tree-navigation
    /// selector of a `subexpression`), appending to `out`. Returns false (after
    /// reporting a diagnostic) for unsupported forms.
    fn path_items(&mut self, n: Node, out: &mut Vec<PathItem>) -> bool {
        match n.kind() {
            "subexpr_prefix" => {
                let mut ok = true;
                for k in kids(n) {
                    ok &= self.path_items(k, out);
                }
                ok
            }
            "subexpr_component" => {
                let Some(c) = kids(n).into_iter().next() else {
                    return true;
                };
                match c.kind() {
                    "identifier_ref" => {
                        out.push(PathItem::Comp(NamePrefix { name: self.ident(c), args: vec![] }));
                        true
                    }
                    "bound_op" => {
                        let (name, args) = self.bound_op_parts(c);
                        out.push(PathItem::Comp(NamePrefix { name, args }));
                        true
                    }
                    "prefix_op_symbol" | "infix_op_symbol" | "postfix_op_symbol" => {
                        let name = self.sym_ident(c);
                        out.push(PathItem::Comp(NamePrefix { name, args: vec![] }));
                        true
                    }
                    "bound_nonfix_op" => match *self.expr(c).kind {
                        ExprKind::OpAppl { name, args, .. } => {
                            out.push(PathItem::Comp(NamePrefix { name, args }));
                            true
                        }
                        _ => {
                            self.err(c, "unsupported operator in a subexpression reference");
                            false
                        }
                    },
                    k => {
                        self.err(c, format!("unexpected `{k}` in a subexpression reference"));
                        false
                    }
                }
            }
            "subexpr_tree_nav" => {
                let Some(c) = kids(n).into_iter().next() else {
                    return true;
                };
                match c.kind() {
                    "child_id" => match self.text(c).parse::<usize>() {
                        Ok(i) if i > 0 => {
                            out.push(PathItem::Sel(Selector::Index(i)));
                            true
                        }
                        _ => {
                            self.err(c, format!("invalid subexpression selector `!{}`", self.text(c)));
                            false
                        }
                    },
                    "colon" => {
                        out.push(PathItem::Sel(Selector::Colon));
                        true
                    }
                    "langle_bracket" => {
                        out.push(PathItem::Sel(Selector::Left));
                        true
                    }
                    "rangle_bracket" => {
                        out.push(PathItem::Sel(Selector::Right));
                        true
                    }
                    "address" => {
                        out.push(PathItem::Sel(Selector::At));
                        true
                    }
                    "operator_args" => {
                        let args = kids(c).into_iter().map(|a| self.expr(a)).collect();
                        out.push(PathItem::Sel(Selector::Args(args)));
                        true
                    }
                    k => {
                        self.err(c, format!("unsupported subexpression selector `{k}`"));
                        false
                    }
                }
            }
            "proof_step_ref" => {
                let name = self.named_ident(&self.text(n).replace(char::is_whitespace, ""), n);
                out.push(PathItem::Comp(NamePrefix { name, args: vec![] }));
                true
            }
            k => {
                self.err(n, format!("unexpected `{k}` in a subexpression reference"));
                false
            }
        }
    }

    /// Build an instance-qualified application or a `Subexpr` from a lowered
    /// path. `tail` is the final operator of a `prefixed_op` (`op` field).
    fn qualified(&mut self, n: Node, items: Vec<PathItem>, tail: Option<Expr>) -> Expr {
        let first_sel = items.iter().position(|i| matches!(i, PathItem::Sel(_)));
        let Some(fs) = first_sel else {
            // Pure name path: `I!J(a)!Op(b)` (or `Def!label`, told apart in sema).
            let Some(mut e) = tail else {
                return self.err_expr(n, "subexpression reference without an operator");
            };
            let prefix = items
                .into_iter()
                .map(|i| match i {
                    PathItem::Comp(c) => c,
                    PathItem::Sel(_) => unreachable!(),
                })
                .collect();
            match &mut *e.kind {
                ExprKind::OpAppl { prefix: p, .. } => *p = prefix,
                _ => return self.err_expr(n, "unsupported instance-qualified operator"),
            }
            e.span = self.span(n);
            return e;
        };
        if fs == 0 {
            return self.err_expr(n, "subexpression selector without a definition name");
        }
        let mut items = items.into_iter();
        let mut prefix: Vec<NamePrefix> = Vec::new();
        for _ in 0..fs {
            if let Some(PathItem::Comp(c)) = items.next() {
                prefix.push(c);
            }
        }
        let last = prefix.pop().expect("fs > 0");
        let base = self.mk(n, ExprKind::OpAppl { prefix, name: last.name, args: last.args });
        // Name components after the first selector are labels.
        let mut rest: Vec<PathItem> = items.collect();
        if let Some(t) = tail {
            match *t.kind {
                ExprKind::OpAppl { prefix, name, args } if prefix.is_empty() => rest.push(PathItem::Comp(NamePrefix { name, args })),
                _ => return self.err_expr(n, "unsupported subexpression selector"),
            }
        }
        let path = rest
            .into_iter()
            .map(|it| match it {
                PathItem::Sel(s) => s,
                PathItem::Comp(c) => Selector::Label(c.name, c.args),
            })
            .collect();
        self.mk(n, ExprKind::Subexpr { base, path })
    }

    /// `prefix!op`: an instance-qualified operator (possibly with selectors).
    pub(super) fn prefixed_op(&mut self, n: Node) -> Expr {
        let mut items = Vec::new();
        let mut ok = true;
        if let Some(p) = field(n, "prefix") {
            ok = self.path_items(p, &mut items);
        }
        let Some(op) = field(n, "op") else {
            return self.err_expr(n, "instance-qualified reference without an operator");
        };
        let e = self.expr(op);
        if !ok {
            return self.placeholder(n);
        }
        self.qualified(n, items, Some(e))
    }

    /// A subexpression reference ending in a selector (`Def!2`, `Def!:`).
    pub(super) fn subexpression(&mut self, n: Node) -> Expr {
        let mut items = Vec::new();
        let mut ok = true;
        for k in kids(n) {
            ok &= self.path_items(k, &mut items);
        }
        if !ok {
            return self.placeholder(n);
        }
        self.qualified(n, items, None)
    }
}
