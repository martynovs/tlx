//! Mapping from tree-sitter-tlaplus operator-symbol node kinds to canonical
//! operator names (see `ast::opnames`).
//!
//! The grammar gives every operator symbol its own node kind (`cup`, `leq`,
//! `times`, ...) covering all ASCII and Unicode spellings, so canonicalization
//! is keyed on the node kind rather than the source text.

use crate::ast::opnames as op;

/// Canonical name of a prefix operator symbol node kind.
pub fn prefix_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "lnot" => op::NOT,
        "union" => op::UNION,
        "powerset" => op::SUBSET,
        "domain" => op::DOMAIN,
        // `-` in `bound_prefix_op` and `-.` (aliased to `negative`) in
        // `prefix_op_symbol` both denote unary minus.
        "negative" | "negative_dot" => op::NEG,
        "enabled" => op::ENABLED,
        "unchanged" => op::UNCHANGED,
        "always" => op::ALWAYS,
        "eventually" => op::EVENTUALLY,
        _ => return None,
    })
}

/// Canonical name of an infix operator symbol node kind. `land`/`lor` map to
/// `/\` and `\/`; callers turn applications of those into `And`/`Or`.
pub fn infix_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "implies" => op::IMPLIES,
        "plus_arrow" => "-+->",
        "equiv" | "iff" => op::EQUIV,
        "leads_to" => op::LEADSTO,
        "land" => "/\\",
        "lor" => "\\/",
        "assign" => ":=",
        "bnf_rule" => "::=",
        "eq" => op::EQ,
        "neq" => op::NEQ,
        "lt" => op::LT,
        "gt" => op::GT,
        "leq" => op::LE,
        "geq" => op::GE,
        "approx" => "\\approx",
        "rs_ttile" => "|-",
        "rd_ttile" => "|=",
        "ls_ttile" => "-|",
        "ld_ttile" => "=|",
        "asymp" => "\\asymp",
        "cong" => "\\cong",
        "doteq" => "\\doteq",
        "gg" => "\\gg",
        "ll" => "\\ll",
        "in" => op::IN,
        "notin" => op::NOTIN,
        "prec" => "\\prec",
        "succ" => "\\succ",
        "preceq" => "\\preceq",
        "succeq" => "\\succeq",
        "propto" => "\\propto",
        "sim" => "\\sim",
        "simeq" => "\\simeq",
        "sqsubset" => "\\sqsubset",
        "sqsupset" => "\\sqsupset",
        "sqsubseteq" => "\\sqsubseteq",
        "sqsupseteq" => "\\sqsupseteq",
        "subset" => "\\subset",
        "supset" => "\\supset",
        "subseteq" => op::SUBSETEQ,
        "supseteq" => "\\supseteq",
        "compose" => op::ATAT,
        "map_to" => op::COLONGT,
        "map_from" => "<:",
        "setminus" => op::SETMINUS,
        "cap" => op::CAP,
        "cup" => op::CUP,
        "dots_2" => op::DOTDOT,
        "dots_3" => "...",
        "plus" => op::PLUS,
        "plusplus" => "++",
        "oplus" => "\\oplus",
        "ominus" => "\\ominus",
        "mod" => op::MOD,
        "modmod" => "%%",
        "vert" => "|",
        "vertvert" => "||",
        "minus" => op::MINUS,
        "minusminus" => "--",
        "amp" => "&",
        "ampamp" => "&&",
        "odot" => "\\odot",
        "oslash" => "\\oslash",
        "otimes" => "\\otimes",
        "mul" => op::MUL,
        "mulmul" => "**",
        "slash" => "/",
        "slashslash" => "//",
        "bigcirc" => "\\bigcirc",
        "bullet" => "\\bullet",
        "div" => op::DIV,
        "circ" => op::CONCAT,
        "star" => "\\star",
        "excl" => "!!",
        "hashhash" => "##",
        "dol" => "$",
        "doldol" => "$$",
        "qq" => "??",
        "sqcap" => "\\sqcap",
        "sqcup" => "\\sqcup",
        "uplus" => "\\uplus",
        "times" => op::TIMES,
        "wr" => "\\wr",
        "cdot" => op::COMPOSE,
        "pow" => op::POW,
        "powpow" => "^^",
        _ => return None,
    })
}

/// Canonical name of a postfix operator symbol node kind.
pub fn postfix_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "sup_plus" => "^+",
        "asterisk" => "^*",
        "sup_hash" => "^#",
        "prime" => op::PRIME,
        _ => return None,
    })
}

/// Canonical name for a bare symbol kind of any fixity.
pub fn any_name(kind: &str) -> Option<&'static str> {
    prefix_name(kind).or_else(|| infix_name(kind)).or_else(|| postfix_name(kind))
}
