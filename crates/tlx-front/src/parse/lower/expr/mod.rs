//! Lowering of expressions.
//!
//! `expr` is the dispatcher: one arm per tree-sitter node kind, in the same
//! order, and no wildcard over an expression family, so a new kind stays a
//! compile error until it is handled (CODE-03). The long handlers of the three
//! families that only dispatch from here live next to it, split by family
//! (CODE-02, CODE-23): `binders` holds the forms that introduce bound
//! variables in a nested scope, `records` the record, tuple and function
//! forms, and `except` EXCEPT with its update paths. The bound-variable
//! primitives `bound_vars` and `bound` stay here because the rest of the
//! module (`definition`'s function definitions, the proof binders) uses them
//! too, and `postfix_operand` is shared by the function and record handlers.

mod binders;
mod except;
mod records;

use std::sync::Arc;

use tree_sitter::Node;

use super::{decode_string, field, field_kids, kids, number_set_name, Lower};
use crate::ast::*;

impl Lower<'_> {
    /// An operator argument: a bare operator symbol, a LAMBDA, or an expression.
    pub(super) fn op_or_expr(&mut self, n: Node) -> Expr {
        match n.kind() {
            "prefix_op_symbol" | "infix_op_symbol" | "postfix_op_symbol" => {
                let name = self.sym_ident(n);
                self.appl(n, name, vec![])
            }
            "lambda" => self.lambda(n),
            _ => self.expr(n),
        }
    }

    pub fn expr(&mut self, n: Node) -> Expr {
        let ks = || kids(n);
        match n.kind() {
            // ---- literals
            "nat_number" => self.number(n, self.text(n), 10),
            "real_number" => self.mk(n, ExprKind::Decimal(Arc::from(self.text(n)))),
            "binary_number" | "octal_number" | "hex_number" => self.radix_number(n),
            "string" => self.string(n),
            "boolean" => self.mk(n, ExprKind::Bool(self.text(n) == "TRUE")),
            "string_set" => self.appl(n, self.named_ident("STRING", n), vec![]),
            "boolean_set" => self.appl(n, self.named_ident("BOOLEAN", n), vec![]),
            k if number_set_name(k).is_some() => self.appl(n, self.named_ident(number_set_name(k).unwrap(), n), vec![]),

            // ---- transparent wrappers
            "parentheses" => match ks().into_iter().next() {
                Some(inner) => self.expr(inner),
                None => self.err_expr(n, "empty parentheses"),
            },
            "label" => self.label(n),

            // ---- names and application
            "identifier_ref" => self.appl(n, self.ident(n), vec![]),
            "bound_op" => {
                let (name, args) = self.bound_op_parts(n);
                self.appl(n, name, args)
            }
            "bound_nonfix_op" => self.bound_nonfix_op(n),
            "prefixed_op" => self.prefixed_op(n),
            "subexpression" => self.subexpression(n),
            // A proof step name (`<1>a`), only meaningful in proofs.
            "proof_step_ref" => self.appl(n, self.named_ident(&self.text(n).replace(char::is_whitespace, ""), n), vec![]),
            "bound_prefix_op" => {
                self.check_precedence(n);
                let Some(sym) = field(n, "symbol") else {
                    return self.err_expr(n, "prefix operator without a symbol");
                };
                let name = self.sym_ident(sym);
                let arg = self.field_expr(n, "rhs");
                self.appl(n, name, vec![arg])
            }
            "bound_infix_op" => {
                self.check_precedence(n);
                self.bound_infix_op(n)
            }
            "bound_postfix_op" => {
                self.check_precedence(n);
                let Some(sym) = field(n, "symbol") else {
                    return self.err_expr(n, "postfix operator without a symbol");
                };
                let name = self.sym_ident(sym);
                if let Some(lhs) = field(n, "lhs") {
                    if let Some((p, x)) = super::prec::nonfix_prefix(lhs) {
                        if super::prec::prefix_looser(p, &name.name, true) {
                            // `~(x)'` is `~((x)')` (SANY).
                            let arg = self.expr(x);
                            let inner = self.appl(n, name, vec![arg]);
                            let pname = self.named_ident(p, lhs);
                            return self.appl(n, pname, vec![inner]);
                        }
                    }
                }
                let arg = self.field_expr(n, "lhs");
                self.appl(n, name, vec![arg])
            }

            // ---- logic
            "conj_list" | "disj_list" => {
                let is_and = n.kind() == "conj_list";
                let items: Vec<Node> = ks()
                    .into_iter()
                    .filter_map(|item| kids(item).into_iter().find(|k| k.kind() != "bullet_conj" && k.kind() != "bullet_disj"))
                    .collect();
                self.junction(n, is_and, &items)
            }
            "bounded_quantification" => self.bounded_quant(n),
            "unbounded_quantification" => self.unbounded_quant(n),
            "choose" => self.choose(n),

            // ---- control
            "if_then_else" => {
                let cond = self.field_expr(n, "if");
                let then = self.field_expr(n, "then");
                let els = self.field_expr(n, "else");
                self.mk(n, ExprKind::If { cond, then, els })
            }
            "case" => self.case(n),
            "let_in" => self.let_expr(n),

            // ---- sets
            "finite_set_literal" => {
                let elems = ks().into_iter().map(|e| self.expr(e)).collect();
                self.mk(n, ExprKind::SetEnum(elems))
            }
            "set_filter" => self.set_filter(n),
            "set_map" => self.set_map(n),

            // ---- functions, tuples, records
            "tuple_literal" => self.tuple(n),
            "function_literal" => self.function_literal(n),
            "function_evaluation" => self.function_evaluation(n),
            "set_of_functions" => self.set_of_functions(n),
            "record_literal" | "set_of_records" => self.record(n),
            "record_value" => self.record_value(n),
            "except" => self.except(n),
            "prev_func_val" => self.mk(n, ExprKind::At),

            // ---- actions / temporal
            "step_expr_or_stutter" | "step_expr_no_stutter" => self.step_expr(n),
            "fairness" => {
                let kind = match n.child(0).map(|c| c.kind()) {
                    Some("SF_") => FairnessKind::Strong,
                    _ => FairnessKind::Weak,
                };
                let parts = ks();
                let [s, a] = parts.as_slice() else {
                    return self.err_expr(n, "malformed fairness expression");
                };
                let sub = self.expr(*s);
                let action = self.expr(*a);
                self.mk(n, ExprKind::Fairness { kind, sub, action })
            }

            // Operator symbols / LAMBDA can only appear as operator arguments,
            // but accept them wherever the grammar produced them.
            "prefix_op_symbol" | "infix_op_symbol" | "postfix_op_symbol" | "lambda" => self.op_or_expr(n),

            k => self.err_expr(n, format!("unsupported expression `{k}`")),
        }
    }

    /// Apply `wrap` (a function application or record field access) to operand `x`. Like SANY (where `[` and `.` bind tighter
    /// than any prefix operator), `~(a).f` is `~((a).f)`: tree-sitter makes
    /// the prefix operator the operand when its own operand is parenthesized.
    fn postfix_operand(&mut self, x: Node, wrap: &mut dyn FnMut(&mut Self, Expr) -> Expr) -> Expr {
        if let Some((p, operand)) = super::prec::nonfix_prefix(x) {
            let inner = self.postfix_operand(operand, wrap);
            let pname = self.named_ident(p, x);
            return self.appl(x, pname, vec![inner]);
        }
        if x.kind() == "bound_prefix_op" {
            if let (Some(sym), Some(rhs)) = (field(x, "symbol"), field(x, "rhs")) {
                let name = self.sym_ident(sym);
                let inner = self.postfix_operand(rhs, wrap);
                return self.appl(x, name, vec![inner]);
            }
        }
        let e = self.expr(x);
        wrap(self, e)
    }

    // ---------------------------------------------------------------- binders

    fn bound_vars(&mut self, intro: &[Node]) -> BoundVars {
        if let [t] = intro {
            if t.kind() == "tuple_of_identifiers" {
                let ids = kids(*t).into_iter().filter(|k| k.kind() == "identifier").map(|i| self.ident(i)).collect();
                return BoundVars::Tuple(ids);
            }
        }
        BoundVars::Many(intro.iter().map(|i| self.ident(*i)).collect())
    }

    /// `quantifier_bound`: `x, y \in S` or `<<x, y>> \in S`.
    pub(super) fn bound(&mut self, n: Node) -> Bound {
        let intro = field_kids(n, "intro");
        let vars = self.bound_vars(&intro);
        let set = match field(n, "set") {
            Some(s) => self.expr(s),
            None => self.err_expr(n, "bound without a set"),
        };
        Bound { vars, set }
    }

    // ----------------------------------------------------------------- pieces

    fn field_expr(&mut self, n: Node, name: &str) -> Expr {
        match field(n, name) {
            Some(e) => self.expr(e),
            None => self.err_expr(n, format!("missing `{name}` in {}", n.kind())),
        }
    }

    fn number(&mut self, n: Node, digits: &str, radix: u32) -> Expr {
        match i128::from_str_radix(digits, radix) {
            Ok(v) => self.mk(n, ExprKind::Num(v)),
            Err(_) => self.err_expr(n, format!("number literal `{}` out of range", self.text(n))),
        }
    }

    /// `\b...`, `\o...`, `\h...` number literals.
    fn radix_number(&mut self, n: Node) -> Expr {
        let radix = match n.kind() {
            "binary_number" => 2,
            "octal_number" => 8,
            _ => 16,
        };
        match kids(n).into_iter().find(|k| k.kind() == "value") {
            Some(v) => self.number(n, self.text(v), radix),
            None => self.err_expr(n, "malformed number literal"),
        }
    }

    fn string(&mut self, n: Node) -> Expr {
        let (out, bad) = decode_string(self.text(n));
        if let Some(o) = bad {
            self.err(n, format!("unknown string escape `\\{o}`"));
        }
        self.mk(n, ExprKind::Str(Arc::from(out)))
    }

    fn label(&mut self, n: Node) -> Expr {
        let name = match field(n, "name") {
            Some(nm) if nm.kind() == "identifier" => self.ident(nm),
            Some(nm) => match field(nm, "name") {
                Some(i) => self.ident(i),
                None => self.ident(nm),
            },
            None => self.named_ident("<label>", n),
        };
        let params = field_kids(n, "parameter").into_iter().map(|p| self.ident(p)).collect();
        let body = self.field_expr(n, "expression");
        self.mk(n, ExprKind::Label { name, params, body })
    }

    // -------------------------------------------------------------- operators

    /// Name and arguments of a `bound_op` (`F(a, b)`).
    pub(super) fn bound_op_parts(&mut self, n: Node) -> (Ident, Vec<Expr>) {
        let name = self.name_field(n);
        let args = field_kids(n, "parameter").into_iter().map(|a| self.op_or_expr(a)).collect();
        (name, args)
    }

    /// `bound_nonfix_op`: an operator symbol applied in prefix form (`+(a, b)`).
    fn bound_nonfix_op(&mut self, n: Node) -> Expr {
        let Some(sym) = field(n, "symbol") else {
            return self.err_expr(n, "operator application without a symbol");
        };
        let operands: Vec<Node> = kids(n).into_iter().filter(|k| *k != sym).collect();
        match kids(sym).first().map(|k| k.kind()) {
            Some("land") => return self.junction(n, true, &operands),
            Some("lor") => return self.junction(n, false, &operands),
            _ => {}
        }
        let name = self.sym_ident(sym);
        let args = operands.into_iter().map(|a| self.expr(a)).collect();
        self.appl(n, name, args)
    }

    fn bound_infix_op(&mut self, n: Node) -> Expr {
        let Some(sym) = field(n, "symbol") else {
            return self.err_expr(n, "infix operator without a symbol");
        };
        let (Some(lhs), Some(rhs)) = (field(n, "lhs"), field(n, "rhs")) else {
            return self.err_expr(n, "infix operator without operands");
        };
        match sym.kind() {
            "land" => self.junction(n, true, &[lhs, rhs]),
            "lor" => self.junction(n, false, &[lhs, rhs]),
            "times" => {
                let name = self.sym_ident(sym);
                let args = product_operands(lhs, rhs).into_iter().map(|o| self.expr(o)).collect();
                self.appl(n, name, args)
            }
            _ => {
                let name = self.sym_ident(sym);
                if let Some((p, x)) = super::prec::nonfix_prefix(lhs) {
                    if super::prec::prefix_looser(p, &name.name, false) {
                        // `~(x) = y` is `~((x) = y)` (SANY).
                        let l = self.expr(x);
                        let r = self.expr(rhs);
                        let inner = self.appl(n, name, vec![l, r]);
                        let pname = self.named_ident(p, lhs);
                        return self.appl(n, pname, vec![inner]);
                    }
                }
                let l = self.expr(lhs);
                let r = self.expr(rhs);
                self.appl(n, name, vec![l, r])
            }
        }
    }

    /// A conjunction (`is_and`) / disjunction of `operands` (not flattened:
    /// subexpression selectors see SANY's structure).
    fn junction(&mut self, n: Node, is_and: bool, operands: &[Node]) -> Expr {
        let out = operands.iter().map(|o| self.expr(*o)).collect();
        self.mk(n, if is_and { ExprKind::And(out) } else { ExprKind::Or(out) })
    }

    // -------------------------------------------------- control and actions

    fn case(&mut self, n: Node) -> Expr {
        let mut arms = Vec::new();
        let mut other = None;
        for k in kids(n) {
            let parts: Vec<Node> = kids(k).into_iter().filter(|p| p.kind() != "case_arrow").collect();
            match (k.kind(), parts.as_slice()) {
                ("case_arm", [p, e]) => {
                    let p = self.expr(*p);
                    let e = self.expr(*e);
                    arms.push((p, e))
                }
                ("other_arm", [e]) => other = Some(self.expr(*e)),
                ("case_box", _) => {}
                _ => self.err(k, "malformed CASE arm"),
            }
        }
        self.mk(n, ExprKind::Case { arms, other })
    }

    /// `[A]_v` / `<<A>>_v`
    fn step_expr(&mut self, n: Node) -> Expr {
        let parts: Vec<Node> = kids(n).into_iter().filter(|k| k.kind() != "langle_bracket" && k.kind() != "rangle_bracket_sub").collect();
        let [a, s] = parts.as_slice() else {
            return self.err_expr(n, "malformed action expression");
        };
        let action = self.expr(*a);
        let sub = self.expr(*s);
        if n.kind() == "step_expr_or_stutter" {
            self.mk(n, ExprKind::BoxAction { action, sub })
        } else {
            self.mk(n, ExprKind::AngleAction { action, sub })
        }
    }
}

/// Operands of an unparenthesized `A \X B \X C`, which is one n-ary product.
/// The grammar is left-associative, so walk down the lhs spine.
fn product_operands<'t>(lhs: Node<'t>, rhs: Node<'t>) -> Vec<Node<'t>> {
    let mut operands = vec![rhs];
    let mut cur = lhs;
    loop {
        let is_times = cur.kind() == "bound_infix_op" && field(cur, "symbol").map(|s| s.kind()) == Some("times");
        if !is_times {
            operands.push(cur);
            break;
        }
        match (field(cur, "lhs"), field(cur, "rhs")) {
            (Some(l), Some(r)) => {
                operands.push(r);
                cur = l;
            }
            _ => {
                operands.push(cur);
                break;
            }
        }
    }
    operands.reverse();
    operands
}
