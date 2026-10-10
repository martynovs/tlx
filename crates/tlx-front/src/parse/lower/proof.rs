//! Theorems, proofs and USE / HIDE. They are not needed for model checking;
//! they are lowered only for SANY's checks (scopes, levels) in tlx-sema.

use tree_sitter::Node;

use super::{field, field_kids, kids, Lower};
use crate::ast::*;

/// Whether `n` has an anonymous child token `tok` (a keyword).
fn has_token(n: Node, tok: &str) -> bool {
    let mut c = n.walk();
    let found = n.children(&mut c).any(|k| !k.is_named() && k.kind() == tok);
    found
}

impl Lower<'_> {
    /// `theorem`: pushed to `out` (named, with an expression statement) and
    /// `proofs`.
    pub(super) fn theorem(&mut self, n: Node, out: &mut Vec<Unit>, proofs: &mut Vec<(usize, ProofUnit)>) {
        let Some(stmt_node) = field(n, "statement") else { return };
        let name = field(n, "name").map(|nm| self.ident(nm));
        let span = self.span(n);
        // Theorems are only needed when referenced or for SANY's checks: drop
        // (rather than reject) ones we cannot lower.
        let ndiags = self.diags.len();
        let stmt = self.stmt(stmt_node);
        if let (Some(name), Stmt::Expr(body)) = (&name, &stmt) {
            if self.diags.len() == ndiags {
                out.push(Unit::Theorem { name: name.clone(), body: body.clone(), span });
            }
        }
        let proof = field(n, "proof").map(|p| self.proof(p));
        let checks = self.proof_diags.split_off(0);
        if self.diags.len() == ndiags {
            proofs.push((out.len(), ProofUnit::Theorem { name, stmt, proof, span }));
        } else {
            self.diags.truncate(ndiags);
        }
        self.diags.extend(checks);
    }

    /// A module-level `use_or_hide`.
    pub(super) fn module_use_or_hide(&mut self, n: Node, at: usize, proofs: &mut Vec<(usize, ProofUnit)>) {
        let ndiags = self.diags.len();
        let u = self.use_or_hide(n);
        if self.diags.len() == ndiags {
            proofs.push((at, ProofUnit::UseOrHide(u)));
        } else {
            self.diags.truncate(ndiags);
        }
    }

    fn stmt(&mut self, n: Node) -> Stmt {
        match n.kind() {
            "assume_prove" => Stmt::AssumeProve(self.assume_prove(n)),
            _ => Stmt::Expr(self.expr(n)),
        }
    }

    fn assume_prove(&mut self, n: Node) -> AssumeProve {
        let assumes = field_kids(n, "assumption")
            .into_iter()
            .map(|a| match a.kind() {
                "new" => Assumption::New(self.new_decl(a)),
                "inner_assume_prove" => match kids(a).into_iter().find(|k| k.kind() == "assume_prove") {
                    Some(ap) => Assumption::AssumeProve(self.assume_prove(ap)),
                    None => Assumption::Expr(self.err_expr(a, "malformed ASSUME/PROVE")),
                },
                _ => Assumption::Expr(self.expr(a)),
            })
            .collect();
        let prove = match field(n, "conclusion") {
            Some(c) => self.expr(c),
            None => self.err_expr(n, "ASSUME without PROVE"),
        };
        AssumeProve { assumes, prove, span: self.span(n) }
    }

    fn new_decl(&mut self, n: Node) -> NewDecl {
        let ks = kids(n);
        let kind = match ks.iter().find(|k| k.kind() == "statement_level").map(|k| self.text(*k)) {
            Some("VARIABLE") => NewKind::Variable,
            Some("STATE") => NewKind::State,
            Some("ACTION") => NewKind::Action,
            Some("TEMPORAL") => NewKind::Temporal,
            _ => NewKind::Constant,
        };
        let decl = match ks.iter().find(|k| matches!(k.kind(), "identifier" | "operator_declaration")) {
            Some(d) => self.op_decl(*d),
            None => OpDecl { name: self.named_ident("<error>", n), arity: 0 },
        };
        let set = match ks.iter().position(|k| k.kind() == "set_in") {
            Some(i) => ks.get(i + 1).map(|s| self.expr(*s)),
            None => None,
        };
        NewDecl { kind, decl, set, span: self.span(n) }
    }

    fn use_or_hide(&mut self, n: Node) -> UseOrHide {
        let hide = has_token(n, "HIDE");
        let body = match kids(n).into_iter().find(|k| k.kind() == "use_body") {
            Some(b) => self.use_body(b),
            None => UseBody::default(),
        };
        UseOrHide { hide, body, span: self.span(n) }
    }

    fn use_body(&mut self, n: Node) -> UseBody {
        let mut body = UseBody::default();
        for part in kids(n) {
            let facts: Vec<Fact> = kids(part)
                .into_iter()
                .map(|f| match f.kind() {
                    "module_ref" => match kids(f).into_iter().next() {
                        Some(m) => Fact::Module(self.ident(m)),
                        None => Fact::Expr(self.err_expr(f, "MODULE without a name")),
                    },
                    _ => Fact::Expr(self.op_or_expr(f)),
                })
                .collect();
            match part.kind() {
                "use_body_def" => body.defs.extend(facts),
                _ => body.facts.extend(facts),
            }
        }
        body
    }

    fn proof(&mut self, n: Node) -> Proof {
        match n.kind() {
            "terminal_proof" => {
                let by = kids(n).into_iter().find(|k| k.kind() == "use_body").map(|b| self.use_body(b));
                Proof::Leaf { by, span: self.span(n) }
            }
            _ => {
                Proof::Steps(kids(n).into_iter().filter(|k| matches!(k.kind(), "proof_step" | "qed_step")).map(|s| self.step(s)).collect())
            }
        }
    }

    fn binders(&mut self, ks: &[Node]) -> Binders {
        if ks.iter().any(|k| k.kind() == "quantifier_bound") {
            Binders::Bounded(ks.iter().filter(|k| k.kind() == "quantifier_bound").map(|b| self.bound(*b)).collect())
        } else {
            Binders::Unbounded(ks.iter().filter(|k| k.kind() == "identifier").map(|i| self.ident(*i)).collect())
        }
    }

    fn step(&mut self, n: Node) -> Step {
        let ks = kids(n);
        let id = match ks.iter().find(|k| k.kind() == "proof_step_id") {
            Some(idn) => {
                let level = kids(*idn).into_iter().find(|k| k.kind() == "level").map_or("", |l| self.text(l));
                let name = kids(*idn).into_iter().find(|k| k.kind() == "name").map_or("", |l| self.text(l));
                if matches!(level, "*" | "+") && !name.is_empty() {
                    self.proof_diags
                        .push(crate::Diagnostic { span: self.span(*idn), message: "<*> and <+> cannot be used for a named step.".into() });
                }
                self.named_ident(&format!("<{level}>{name}"), *idn)
            }
            None => self.named_ident("<error>", n),
        };
        let mut proof = None;
        let kind = if n.kind() == "qed_step" {
            proof = ks.iter().find(|k| matches!(k.kind(), "terminal_proof" | "non_terminal_proof")).map(|p| self.proof(*p));
            StepKind::Qed
        } else {
            let Some(body) = ks.iter().find(|k| k.kind() != "proof_step_id").copied() else {
                return Step { id, kind: StepKind::Qed, proof: None, span: self.span(n) };
            };
            let bks = kids(body);
            let sub = |s: &mut Self| -> Option<Proof> {
                bks.iter().find(|k| matches!(k.kind(), "terminal_proof" | "non_terminal_proof")).map(|p| s.proof(*p))
            };
            match body.kind() {
                "suffices_proof_step" => {
                    let suffices = has_token(body, "SUFFICES");
                    let stmt = match bks.iter().find(|k| !matches!(k.kind(), "terminal_proof" | "non_terminal_proof")) {
                        Some(s) => self.stmt(*s),
                        None => Stmt::Expr(self.err_expr(body, "proof step without an assertion")),
                    };
                    proof = sub(self);
                    if suffices {
                        StepKind::Suffices(stmt)
                    } else {
                        StepKind::Assert(stmt)
                    }
                }
                "definition_proof_step" => StepKind::Define(bks.iter().map(|d| self.definition(*d, false, self.span(*d))).collect()),
                "have_proof_step" => match bks.first() {
                    Some(e) => StepKind::Have(self.expr(*e)),
                    None => StepKind::Have(self.err_expr(body, "HAVE without an expression")),
                },
                "witness_proof_step" => StepKind::Witness(bks.iter().map(|e| self.expr(*e)).collect()),
                "take_proof_step" => StepKind::Take(self.binders(&bks)),
                "case_proof_step" => {
                    let e = match bks.first() {
                        Some(e) => self.expr(*e),
                        None => self.err_expr(body, "CASE without an expression"),
                    };
                    proof = sub(self);
                    StepKind::Case(e)
                }
                "pick_proof_step" => {
                    let rest: Vec<Node> =
                        bks.iter().filter(|k| !matches!(k.kind(), "terminal_proof" | "non_terminal_proof")).copied().collect();
                    let Some((pred, vars)) = rest.split_last() else {
                        return Step { id, kind: StepKind::Qed, proof: None, span: self.span(n) };
                    };
                    let binders = self.binders(vars);
                    let pred = self.expr(*pred);
                    proof = sub(self);
                    StepKind::Pick(binders, pred)
                }
                "use_or_hide" => StepKind::UseOrHide(self.use_or_hide(body)),
                "instance" => StepKind::Instance(self.instance(body, None, vec![], false, self.span(body))),
                k => {
                    self.err(body, format!("unsupported proof step `{k}`"));
                    StepKind::Qed
                }
            }
        };
        Step { id, kind, proof, span: self.span(n) }
    }
}
