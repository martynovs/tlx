//! The CST walk. Each method takes a tree-sitter node of a known kind and
//! produces the corresponding surface-AST value. Errors are pushed to
//! `diags`; lowering always continues (with placeholder expressions) so that
//! all problems in a file are reported at once.
//!
//! This module handles modules, units and definitions; `expr` lowers
//! expressions and `path` instance-qualified names / subexpression references.

mod expr;
mod path;
mod prec;
mod proof;

use std::sync::Arc;

use tree_sitter::Node;

use super::{node_span, ops, SrcMap};
use crate::ast::*;
use crate::escape;
use crate::Diagnostic;

/// Named, non-extra (i.e. not comment) children of `n`.
fn kids<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    let mut c = n.walk();
    n.named_children(&mut c).filter(|k| !k.is_extra()).collect()
}

/// Named, non-extra children of `n` carrying field `name`.
fn field_kids<'t>(n: Node<'t>, name: &str) -> Vec<Node<'t>> {
    let mut c = n.walk();
    n.children_by_field_name(name, &mut c).filter(|k| k.is_named() && !k.is_extra()).collect()
}

fn field<'t>(n: Node<'t>, name: &str) -> Option<Node<'t>> {
    field_kids(n, name).into_iter().next()
}

/// Node kinds of the grammar's built-in number sets (usable as operator names).
fn number_set_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "nat_number_set" => "Nat",
        "int_number_set" => "Int",
        "real_number_set" => "Real",
        _ => return None,
    })
}

pub struct Lower<'s> {
    src: &'s str,
    map: &'s SrcMap,
    file: FileId,
    pub diags: Vec<Diagnostic>,
    /// SANY's checks on the proof being lowered (reported even when the
    /// proof itself is dropped).
    proof_diags: Vec<Diagnostic>,
}

impl<'s> Lower<'s> {
    pub fn new(src: &'s str, file: FileId, map: &'s SrcMap) -> Self {
        Lower { src, map, file, diags: Vec::new(), proof_diags: Vec::new() }
    }

    // ---------------------------------------------------------------- helpers

    fn span(&self, n: Node) -> Span {
        node_span(n, self.file, self.map)
    }

    fn text(&self, n: Node) -> &'s str {
        &self.src[n.byte_range()]
    }

    fn ident(&self, n: Node) -> Ident {
        if let Some(op) = self.map.op_at(n.start_byte()) {
            return self.named_ident(op, n);
        }
        Ident { name: Arc::from(self.text(n)), span: self.span(n) }
    }

    fn named_ident(&self, name: &str, n: Node) -> Ident {
        Ident { name: Arc::from(name), span: self.span(n) }
    }

    /// The identifier in field `name` of `n`, or an `<error>` placeholder
    /// (the grammar guarantees the field; a missing one is a parse error
    /// reported elsewhere).
    fn name_field(&self, n: Node) -> Ident {
        match field(n, "name") {
            Some(nm) => self.ident(nm),
            None => self.named_ident("<error>", n),
        }
    }

    fn err(&mut self, n: Node, msg: impl Into<String>) {
        self.diags.push(Diagnostic { span: self.span(n), message: msg.into() });
    }

    /// Report an error and return a placeholder expression.
    fn err_expr(&mut self, n: Node, msg: impl Into<String>) -> Expr {
        self.err(n, msg);
        self.placeholder(n)
    }

    /// The `<error>` expression standing for a node already reported.
    fn placeholder(&self, n: Node) -> Expr {
        self.mk(n, ExprKind::OpAppl { prefix: vec![], name: self.named_ident("<error>", n), args: vec![] })
    }

    fn mk(&self, n: Node, kind: ExprKind) -> Expr {
        Expr { kind: Box::new(kind), span: self.span(n) }
    }

    fn appl(&self, n: Node, name: Ident, args: Vec<Expr>) -> Expr {
        self.mk(n, ExprKind::OpAppl { prefix: vec![], name, args })
    }

    /// Canonical identifier for an operator symbol node: either a wrapper
    /// (`prefix_op_symbol` / `infix_op_symbol` / `postfix_op_symbol`) or a
    /// specific symbol kind (`cup`, `lnot`, ...).
    fn sym_ident(&mut self, n: Node) -> Ident {
        let inner = match n.kind() {
            "prefix_op_symbol" | "infix_op_symbol" | "postfix_op_symbol" => kids(n).into_iter().next().unwrap_or(n),
            _ => n,
        };
        match ops::any_name(inner.kind()) {
            Some(name) => self.named_ident(name, n),
            None => {
                self.err(n, format!("unknown operator symbol `{}` ({})", self.text(n), inner.kind()));
                self.ident(n)
            }
        }
    }

    /// Name of something that can be defined / declared / referenced as an
    /// operator: identifier, number set, or operator symbol.
    fn op_name(&mut self, n: Node) -> Ident {
        match n.kind() {
            "identifier" | "identifier_ref" => self.ident(n),
            k => match number_set_name(k) {
                Some(name) => self.named_ident(name, n),
                None => self.sym_ident(n),
            },
        }
    }

    // ----------------------------------------------------------------- module

    pub fn module(&mut self, n: Node) -> Module {
        let name = match field(n, "name") {
            Some(id) => self.ident(id),
            None => {
                self.err(n, "module without a name");
                self.named_ident("<error>", n)
            }
        };
        let mut extends = Vec::new();
        let mut units = Vec::new();
        let mut proofs = Vec::new();
        for k in kids(n) {
            match k.kind() {
                "header_line" | "double_line" | "identifier" => {}
                "extends" => extends.extend(kids(k).into_iter().map(|i| self.ident(i))),
                "theorem" => self.theorem(k, &mut units, &mut proofs),
                "use_or_hide" => self.module_use_or_hide(k, units.len(), &mut proofs),
                _ => self.unit(k, &mut units),
            }
        }
        Module { name, extends, units, proofs, span: self.span(n) }
    }

    fn unit(&mut self, n: Node, out: &mut Vec<Unit>) {
        match n.kind() {
            "variable_declaration" => out.push(Unit::Variables(kids(n).into_iter().map(|i| self.ident(i)).collect())),
            "constant_declaration" => {
                let decls = kids(n).into_iter().map(|d| self.op_decl(d)).collect();
                out.push(Unit::Constants(decls))
            }
            "recursive_declaration" => {
                let decls = kids(n).into_iter().map(|d| self.op_decl(d)).collect();
                out.push(Unit::Recursive(decls))
            }
            "single_line" => {}
            "local_definition" => match kids(n).into_iter().next() {
                Some(d) => out.push(self.definition(d, true, self.span(n))),
                None => self.err(n, "empty LOCAL definition"),
            },
            "operator_definition" | "function_definition" | "instance" | "module_definition" => {
                out.push(self.definition(n, false, self.span(n)))
            }
            "assumption" => {
                let name = field(n, "name").map(|i| self.ident(i));
                let body = match kids(n).into_iter().last() {
                    Some(b) if b.kind() != "def_eq" && Some(b) != field(n, "name") => self.expr(b),
                    _ => self.err_expr(n, "ASSUME without a body"),
                };
                out.push(Unit::Assume { name, body, span: self.span(n) })
            }
            "module" => out.push(Unit::Module(self.module(n))),
            k => self.err(n, format!("unsupported module unit `{k}`")),
        }
    }

    /// `x` or `F(_, _)` / `_ + _` / `-. _` / `_ ^+`.
    fn op_decl(&mut self, n: Node) -> OpDecl {
        match n.kind() {
            "identifier" => OpDecl { name: self.ident(n), arity: 0 },
            "operator_declaration" => {
                let name = match field(n, "name") {
                    Some(nm) => self.op_name(nm),
                    None => {
                        self.err(n, "operator declaration without a name");
                        self.named_ident("<error>", n)
                    }
                };
                let arity = kids(n).iter().filter(|k| k.kind() == "placeholder").count();
                OpDecl { name, arity }
            }
            k => {
                self.err(n, format!("unexpected `{k}` in declaration"));
                OpDecl { name: self.named_ident("<error>", n), arity: 0 }
            }
        }
    }

    fn definition(&mut self, n: Node, local: bool, span: Span) -> Unit {
        match n.kind() {
            "operator_definition" => {
                let name = match field(n, "name") {
                    Some(nm) => self.op_name(nm),
                    None => self.named_ident("<error>", n),
                };
                let params = field_kids(n, "parameter").into_iter().map(|p| self.op_decl(p)).collect();
                let body = match field(n, "definition") {
                    Some(b) => self.expr(b),
                    None => self.err_expr(n, "definition without a body"),
                };
                Unit::OpDef(OpDef { name, params, body, local, span })
            }
            "function_definition" => {
                let name = self.name_field(n);
                let bounds = kids(n).into_iter().filter(|k| k.kind() == "quantifier_bound").map(|b| self.bound(b)).collect();
                let body = match field(n, "definition") {
                    Some(b) => self.expr(b),
                    None => self.err_expr(n, "function definition without a body"),
                };
                Unit::FnDef(FnDef { name, bounds, body, local, span })
            }
            "instance" => Unit::Instance(self.instance(n, None, vec![], local, span)),
            "module_definition" => {
                let name = field(n, "name").map(|nm| self.ident(nm));
                let params = field_kids(n, "parameter").into_iter().map(|p| self.op_decl(p)).collect();
                match field(n, "definition") {
                    Some(inst) => Unit::Instance(self.instance(inst, name, params, local, span)),
                    None => {
                        self.err(n, "module definition without INSTANCE");
                        Unit::Instance(Instance { name, params, module: self.named_ident("<error>", n), substs: vec![], local, span })
                    }
                }
            }
            k => {
                self.err(n, format!("unsupported definition `{k}`"));
                Unit::Variables(vec![])
            }
        }
    }

    fn instance(&mut self, n: Node, name: Option<Ident>, params: Vec<OpParam>, local: bool, span: Span) -> Instance {
        let ks = kids(n);
        let module = match ks.first() {
            Some(m) if m.kind() == "identifier_ref" => self.ident(*m),
            _ => {
                self.err(n, "INSTANCE without a module name");
                self.named_ident("<error>", n)
            }
        };
        let mut substs = Vec::new();
        for s in ks.iter().filter(|k| k.kind() == "substitution") {
            let parts: Vec<Node> = kids(*s).into_iter().filter(|k| k.kind() != "gets").collect();
            if parts.len() != 2 {
                self.err(*s, "malformed WITH substitution");
                continue;
            }
            let target = self.op_name(parts[0]);
            let value = self.op_or_expr(parts[1]);
            substs.push((target, value));
        }
        Instance { name, params, module, substs, local, span }
    }
}

/// Decode a string literal token (with quotes). Returns the first unknown
/// escape character, if any.
pub(crate) fn decode_string(text: &str) -> (String, Option<char>) {
    let inner = text.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(text);
    let mut out = String::with_capacity(inner.len());
    let mut bad = None;
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(esc) => match escape::unescape(esc) {
                Some(d) => out.push(d),
                None => {
                    bad.get_or_insert(esc);
                    out.push(esc);
                }
            },
            None => out.push('\\'),
        }
    }
    (out, bad)
}

#[cfg(test)]
mod tests {
    use super::decode_string;

    #[test]
    fn decodes_the_six_escapes() {
        let (s, bad) = decode_string(r#""a\"b\\c\nd\te\rf\fg""#);
        assert_eq!(s, "a\"b\\c\nd\te\rf\u{c}g");
        assert_eq!(bad, None);
    }

    #[test]
    fn reports_the_first_unknown_escape_and_keeps_its_character() {
        let (s, bad) = decode_string(r#""x\qy\z""#);
        assert_eq!(s, "xqyz");
        assert_eq!(bad, Some('q'));
    }

    #[test]
    fn a_trailing_escaped_backslash_is_decoded() {
        let (s, bad) = decode_string(r#""tail\\""#);
        assert_eq!((s.as_str(), bad), ("tail\\", None));
    }

    #[test]
    fn text_without_its_closing_quote_is_not_unwrapped() {
        let (s, bad) = decode_string("\"abc\\");
        assert_eq!((s.as_str(), bad), ("\"abc\\", None));
    }
}
