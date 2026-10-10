//! tlx-front: TLA+ front end (see docs/design.md §3).
//!
//! Pipeline: source text --(tree-sitter-tlaplus)--> CST --(parse)--> surface AST
//! (name resolution to HIR happens in tlx-sema). `cfg` parses TLC model
//! configuration files.

pub mod ast;
pub mod cfg;
mod escape;
pub mod parse;

use ast::Span;

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.message)
    }
}
