//! SANY's operator precedence conflicts.
//!
//! tree-sitter-tlaplus resolves every combination of operators, but SANY
//! (an operator-precedence parser, `OperatorStack`) rejects expressions in
//! which two operators with overlapping precedence ranges compete for an
//! operand (`a /\ b \/ c`, `a = b = c`, `UNCHANGED x'`, `[]x = 1`), unless
//! they are the same left-associative operator. Given the tree, the
//! operators that competed with an operator node are those on the inner
//! spines of its operands: for infix `a P b`, the infix/prefix operators on
//! the right spine of `a` and the infix/postfix operators on the left spine
//! of `b`.

use tree_sitter::Node;

use super::{field, ops, Lower};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fix {
    Prefix,
    Infix,
    Postfix,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Assoc {
    None,
    Left,
}

/// SANY's precedence range and associativity (tla2sany/parser/Operators.java)
/// of the operator with tlx canonical name `name`.
fn sany_prec(name: &str, fix: Fix) -> Option<(u8, u8, Assoc)> {
    use Assoc::*;
    Some(match (fix, name) {
        (Fix::Postfix, "'" | "^+" | "^*" | "^#") => (150, 150, None),
        (Fix::Prefix, "-.") => (120, 120, None),
        (Fix::Prefix, "~") => (40, 40, Left),
        (Fix::Prefix, "[]" | "<>" | "ENABLED" | "UNCHANGED") => (40, 150, None),
        (Fix::Prefix, "SUBSET" | "UNION" | "DOMAIN") => (100, 130, None),
        (Fix::Infix, n) => match n {
            "^" | "^^" => (140, 140, None),
            "/" | "//" | "\\oslash" | "\\div" => (130, 130, None),
            "*" | "&&" | "&" | "**" | "\\odot" | "\\otimes" | "\\star" | "\\o" | "\\bigcirc" | "\\bullet" => (130, 130, Left),
            "-" | "--" | "\\ominus" => (110, 110, Left),
            "+" | "++" | "\\oplus" => (100, 100, Left),
            "/\\" | "\\/" => (30, 30, Left),
            "~>" | "<=>" | "-+->" => (20, 20, None),
            "=>" => (10, 10, None),
            "\\cdot" => (50, 140, Left),
            "\\X" => (100, 130, Left),
            "\\" => (80, 80, None),
            "\\cap" | "\\cup" => (80, 80, Left),
            "..." | ".." => (90, 90, None),
            "|" | "||" | "%%" => (100, 110, Left),
            "%" => (100, 110, None),
            "$$" | "$" | "??" | "##" | "\\uplus" | "\\sqcap" | "\\sqcup" => (90, 130, Left),
            "!!" => (90, 130, None),
            "\\wr" => (90, 140, None),
            "@@" => (60, 60, Left),
            "<:" | ":>" => (70, 70, None),
            "=" | "/=" | "<" | ">" | "<=" | ">=" | "\\in" | "\\notin" | "\\subseteq" | "|-" | "|=" | "-|" | "=|" | ":=" | "::="
            | "\\prec" | "\\succ" | "\\preceq" | "\\succeq" | "\\sim" | "\\simeq" | "\\ll" | "\\gg" | "\\asymp" | "\\subset"
            | "\\supset" | "\\supseteq" | "\\approx" | "\\cong" | "\\sqsubset" | "\\sqsubseteq" | "\\sqsupset" | "\\sqsupseteq"
            | "\\doteq" | "\\propto" => (50, 50, None),
            _ => return Option::None,
        },
        _ => return Option::None,
    })
}

/// An operator node: its fixity, canonical name and operand(s).
struct OpNode<'t> {
    fix: Fix,
    name: &'static str,
    lhs: Option<Node<'t>>,
    rhs: Option<Node<'t>>,
    /// A prefix operator applied like a function: `~(x)`.
    nonfix: bool,
}

/// A prefix operator symbol applied like a function to one (parenthesized)
/// operand, `~(x)`: tree-sitter parses `~(x) = y` as `(~x) = y`, but for
/// SANY it is `~ (x) = y`, i.e. `~((x) = y)`. Returns the canonical name
/// and the operand.
pub(super) fn nonfix_prefix(n: Node<'_>) -> Option<(&'static str, Node<'_>)> {
    if n.kind() != "bound_nonfix_op" {
        return None;
    }
    let sym = field(n, "symbol")?;
    if sym.kind() != "prefix_op_symbol" {
        return None;
    }
    let name = ops::prefix_name(super::kids(sym).first()?.kind())?;
    let operands: Vec<Node> = super::kids(n).into_iter().filter(|k| *k != sym).collect();
    match operands[..] {
        [x] => Some((name, x)),
        _ => None,
    }
}

/// Whether prefix operator `p` binds less tightly than the infix (or, with
/// `postfix`, postfix) operator `op`: SANY then applies `p` to the whole
/// infix / postfix expression.
pub(super) fn prefix_looser(p: &str, op: &str, postfix: bool) -> bool {
    let fix = if postfix { Fix::Postfix } else { Fix::Infix };
    match (sany_prec(p, Fix::Prefix), sany_prec(op, fix)) {
        (Some(a), Some(b)) => a.1 < b.0,
        _ => false,
    }
}

fn op_node(n: Node<'_>) -> Option<OpNode<'_>> {
    if let Some((name, _)) = nonfix_prefix(n) {
        // The operand is parenthesized: no operator inside competes.
        return Some(OpNode { fix: Fix::Prefix, name, lhs: None, rhs: None, nonfix: true });
    }
    let sym = field(n, "symbol")?;
    let (fix, name) = match n.kind() {
        "bound_infix_op" => (Fix::Infix, ops::infix_name(sym.kind())?),
        "bound_prefix_op" => (Fix::Prefix, ops::prefix_name(sym.kind())?),
        "bound_postfix_op" => (Fix::Postfix, ops::postfix_name(sym.kind())?),
        _ => return None,
    };
    Some(OpNode { fix, name, lhs: field(n, "lhs"), rhs: field(n, "rhs"), nonfix: false })
}

/// Operators that were on SANY's stack when the operator after `n` came:
/// the infix and prefix operators on the right spine of `n`.
fn right_spine<'t>(n: Node<'t>, out: &mut Vec<OpNode<'t>>) {
    if let Some(op) = op_node(n) {
        if op.fix != Fix::Postfix {
            let next = op.rhs;
            out.push(op);
            if let Some(r) = next {
                right_spine(r, out);
            }
        }
    }
}

/// Operators that came after the operator before `n`: the infix and
/// postfix operators on the left spine of `n`.
fn left_spine<'t>(n: Node<'t>, out: &mut Vec<OpNode<'t>>) {
    if let Some(op) = op_node(n) {
        if op.fix != Fix::Prefix {
            let next = op.lhs;
            out.push(op);
            if let Some(l) = next {
                left_spine(l, out);
            }
        }
    }
}

impl Lower<'_> {
    /// Report SANY's precedence conflicts between the operator node `n` and
    /// the operators of its operands.
    pub(super) fn check_precedence(&mut self, n: Node) {
        let Some(p) = op_node(n) else { return };
        let Some(pp) = sany_prec(p.name, p.fix) else { return };
        let mut conflicts: Vec<(&'static str, &'static str)> = Vec::new();
        // Operators to the left of p's operand: (left, right) must satisfy
        // left.Low > right.High, or be the same left-associative operator.
        let mut left_ops = Vec::new();
        if p.fix != Fix::Prefix {
            if let Some(l) = p.lhs {
                right_spine(l, &mut left_ops);
            }
        }
        for c in &left_ops {
            let Some(cp) = sany_prec(c.name, c.fix) else { continue };
            if c.nonfix && cp.1 < pp.0 {
                // `~(x) = y`: lowered as `~((x) = y)`.
                continue;
            }
            let same = c.name == p.name && c.fix == p.fix;
            if !(cp.0 > pp.1 || (same && cp.2 == Assoc::Left)) {
                conflicts.push((c.name, p.name));
            }
        }
        // Operators to the right: (p, c) must satisfy p.High < c.Low.
        let mut right_ops = Vec::new();
        if p.fix != Fix::Postfix {
            if let Some(r) = p.rhs {
                left_spine(r, &mut right_ops);
            }
        }
        for c in &right_ops {
            let Some(cp) = sany_prec(c.name, c.fix) else { continue };
            if pp.1 >= cp.0 {
                conflicts.push((p.name, c.name));
            }
        }
        if let Some((l, r)) = conflicts.first() {
            self.err(n, format!("Precedence conflict between ops {l} and {r}."));
        }
    }
}
