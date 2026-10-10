//! Syntax-error diagnostics from tree-sitter ERROR / MISSING nodes.

use tree_sitter::Node;

use super::{node_span, SrcMap};
use crate::ast::FileId;
use crate::Diagnostic;

/// The first line of `n`'s text (at most 40 characters, `...` if cut).
fn snippet(src: &str, n: Node) -> String {
    let text = &src[n.byte_range()];
    let line = text.lines().next().unwrap_or("").trim();
    let mut s: String = line.chars().take(40).collect();
    if s.len() < line.len() || text.trim_end().contains('\n') {
        s.push_str("...");
    }
    s
}

/// " (in <kind>)" for the nearest enclosing non-ERROR node, if informative.
fn context(n: Node) -> String {
    let mut p = n.parent();
    while let Some(q) = p {
        if !q.is_error() {
            return match q.kind() {
                "source_file" | "module" => String::new(),
                k => format!(" (in {k})"),
            };
        }
        p = q.parent();
    }
    String::new()
}

/// Append diagnostics for the ERROR / MISSING nodes under `n` that start
/// at or before byte `limit`.
pub(super) fn collect_syntax_errors(n: Node, src: &str, file: FileId, map: &SrcMap, limit: usize, out: &mut Vec<Diagnostic>) {
    // Comments (including PlusCal algorithms inside them) are irrelevant:
    // errors there are not reported.
    let relevant = n.is_error() || n.has_error() || n.is_missing();
    if n.start_byte() > limit || matches!(n.kind(), "comment" | "block_comment") || !relevant {
        return;
    }
    if n.is_missing() {
        let what = if n.is_named() { n.kind().to_string() } else { format!("`{}`", n.kind()) };
        out.push(Diagnostic { span: node_span(n, file, map), message: format!("syntax error: missing {what}{}", context(n)) });
        return;
    }
    // Report the innermost ERROR nodes: error recovery often wraps a whole
    // module in one ERROR, which is useless as a location.
    let before = out.len();
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        collect_syntax_errors(ch, src, file, map, limit, out);
    }
    if n.is_error() && out.len() == before {
        let at = error_anchor(n);
        let msg = if at.byte_range().is_empty() {
            format!("syntax error{}", context(n))
        } else if at.start_byte() == n.start_byte() {
            format!("syntax error: unexpected `{}`{}", snippet(src, n), context(n))
        } else {
            let line_end = src[at.start_byte()..].find('\n').map_or(src.len(), |i| at.start_byte() + i);
            let text: String = src[at.start_byte()..line_end].trim().chars().take(40).collect();
            format!("syntax error at or after `{text}`{}", context(n))
        };
        out.push(Diagnostic { span: node_span(at, file, map), message: msg });
    }
}

/// Where to report an innermost ERROR node. Error recovery may swallow a
/// whole module (header and the units that parsed fine) into one ERROR; point
/// at the first child that is not part of the header or a complete unit
/// instead.
fn error_anchor(n: Node) -> Node {
    let mut c = n.walk();
    let mut after_module_kw = false;
    for ch in n.children(&mut c) {
        let skip = matches!(
            ch.kind(),
            "single_line"
                | "header_line"
                | "MODULE"
                | "extends"
                | "comment"
                | "block_comment"
                | "variable_declaration"
                | "constant_declaration"
                | "recursive_declaration"
                | "operator_definition"
                | "function_definition"
                | "module_definition"
                | "instance"
                | "local_definition"
                | "assumption"
                | "theorem"
                | "use_or_hide"
                | "module"
        ) || (after_module_kw && ch.kind() == "identifier");
        after_module_kw = ch.kind() == "MODULE";
        if !skip {
            return ch;
        }
    }
    n
}
