//! Diagnostics of the compiled tier: the coverage summary logged before the
//! first run and debugging dumps.

use std::collections::HashMap;

use tlx_base::diag::{emit, enabled, Topic};
use tlx_layout::Shape;
use tlx_runtime::Model;

use super::model::EncModel;
use crate::coverage::{boxed_histogram_typed, Coverage};
use crate::ir::{ActionFn, PredFn, Stmt};

/// `TLX_IR_DUMP=name`: print the IR of the actions / predicates called `name`
/// (compact; `raw` (`TLX_IR_DUMP_RAW`) for the `Debug` form), and of the
/// compiled functions of that name; `*` prints all of them.
pub(super) fn dump_ir<'a>(
    name: &str,
    raw: bool,
    actions: &[Option<ActionFn>],
    preds: impl Iterator<Item = &'a PredFn>,
    funcs: &[crate::ir::FnDef],
) {
    let all = name == "*";
    for (i, f) in funcs.iter().enumerate().filter(|(_, f)| all || &*f.name == name) {
        let mut out = String::new();
        crate::ir_fmt::expr(&f.body, 0, &mut out);
        emit(Topic::Ir, format_args!("function f{i} {} (params {:?}, temps {:?}, result {:?})\n{out}", f.name, f.params, f.temps, f.ret));
    }
    for f in actions.iter().flatten().filter(|f| all || &*f.name == name) {
        if raw {
            emit(Topic::Ir, format_args!("{:#?}", f.body));
        } else {
            let mut out = String::new();
            crate::ir_fmt::stmt(&f.body, 0, &mut out);
            emit(Topic::Ir, format_args!("action {} (temps {:?})\n{out}", f.name, f.temps));
        }
    }
    for p in preds.filter(|p| all || &*p.name == name) {
        if raw {
            emit(Topic::Ir, format_args!("{:#?}", p.body));
        } else {
            let mut out = String::new();
            crate::ir_fmt::expr(&p.body, 0, &mut out);
            emit(Topic::Ir, format_args!("predicate {}\n{out}", p.name));
        }
    }
}

/// Log the layout size, typed coverage of the actions, the model's notes
/// and (`TLX_COVERAGE`) the histogram of boxed / interpreted IR nodes.
pub(super) fn log_coverage(m: &EncModel) {
    let covs: Vec<Coverage> = m.actions.iter().flatten().map(|f| Coverage::of_stmt(&f.body)).collect();
    let typed = covs.iter().filter(|c| c.fully_typed()).count();
    let (nodes, boxed, interp) = covs.iter().fold((0, 0, 0), |a, c| (a.0 + c.nodes, a.1 + c.boxed, a.2 + c.interp));
    emit(
        Topic::Compile,
        format_args!(
            "layout: {} words; {}/{} actions lowered, {} fully typed; IR nodes {} ({} boxed, {} interpreter)",
            m.layout.words,
            m.lowered_actions(),
            m.action_names().len(),
            typed,
            nodes,
            boxed,
            interp
        ),
    );
    for (what, preds) in [("invariants", &m.invariants), ("action properties", &m.aprops)] {
        let (n, b, i) = preds.iter().fold((0, 0, 0), |a, p| {
            let mut c = Coverage::default();
            c.expr(&p.body);
            (a.0 + c.nodes, a.1 + c.boxed, a.2 + c.interp)
        });
        emit(Topic::Compile, format_args!("IR {what}: {} lowered; IR nodes {n} ({b} boxed, {i} interpreter)", preds.len()));
    }
    for n in &m.notes {
        emit(Topic::Compile, n);
    }
    if enabled(Topic::Coverage) {
        for (v, s) in m.base.ip.prog.vars.iter().zip(m.layout.vars.iter()) {
            let line = var_shape(&v.name, s);
            emit(Topic::Coverage, format_args!("{line}"));
        }
        let mut nodes = HashMap::new();
        for f in m.actions.iter().flatten() {
            crate::coverage::node_histogram(&f.body, &mut nodes, &m.typing);
        }
        for p in m.invariants.iter().chain(&m.aprops) {
            crate::coverage::node_histogram(&Stmt::Guard { cond: p.body.clone(), then: Box::new(Stmt::Stop) }, &mut nodes, &m.typing);
        }
        for f in &m.typing.funcs {
            crate::coverage::node_histogram(&Stmt::Guard { cond: f.body.clone(), then: Box::new(Stmt::Stop) }, &mut nodes, &m.typing);
        }
        let mut nodes: Vec<_> = nodes.into_iter().collect();
        nodes.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, count) in nodes {
            emit(Topic::Coverage, format_args!("node {} {count} {}", key.split_once(' ').unwrap().0, key.split_once(' ').unwrap().1));
        }
        for (k, n) in boxed_histogram(m) {
            emit(Topic::Coverage, format_args!("boxed {n:6} {k}"));
        }
    }
}

/// One `TLX_COVERAGE` line: the `Debug` text of a variable's layout shape,
/// cut to a readable length. That text carries arbitrary spec text (string
/// constants, model values), so the cut is taken at a character boundary
/// (CODE-17); every ASCII text is cut exactly where it was before.
fn var_shape(name: &str, s: &Shape) -> String {
    let d = format!("{s:?}");
    let head = crate::ir_fmt::cut(&d, 160);
    let ellipsis = if head.len() < d.len() { "..." } else { "" };
    format!("var {name} : {head}{ellipsis}")
}

/// Boxed / interpreted node kinds of all actions and predicates, most
/// frequent first.
fn boxed_histogram(m: &EncModel) -> Vec<(String, usize)> {
    let mut h = HashMap::new();
    for f in m.actions.iter().flatten() {
        boxed_histogram_typed(&f.body, &mut h, &m.typing);
    }
    for p in m.invariants.iter().chain(&m.aprops) {
        let g = Stmt::Guard { cond: p.body.clone(), then: Box::new(Stmt::Stop) };
        let mut hp = HashMap::new();
        boxed_histogram_typed(&g, &mut hp, &m.typing);
        for (k, n) in hp {
            *h.entry(format!("inv {}: {k}", p.name)).or_default() += n;
        }
    }
    let mut v: Vec<_> = h.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tlx_value::{Sym, Value};

    use super::*;

    /// A shape whose `Debug` text holds a run of multibyte characters, with
    /// `pad` extra ASCII bytes in front of it: the run starts at
    /// `prefix + pad`, so byte 160 falls inside a character for exactly one
    /// of the two paddings, whatever the shape's own prefix is.
    fn str_shape(pad: &str) -> Shape {
        let content = format!("{pad}{}", "é".repeat(200));
        Shape::Atom(Arc::from([Value::Str(Sym::new(&content))]))
    }

    /// The coverage line of a shape with a multibyte character across byte
    /// 160: the plain `&d[..160]` of the old line panicked there, the cut
    /// backs off to the start of the character and marks the truncation
    /// (CODE-17).
    #[test]
    fn var_shape_cuts_on_a_character_boundary() {
        let mut straddled = 0;
        for pad in ["", "a"] {
            let s = str_shape(pad);
            let d = format!("{s:?}");
            assert!(d.len() > 160, "pad {pad:?}: nothing to cut");
            if !d.is_char_boundary(160) {
                straddled += 1;
            }
            let line = var_shape("x", &s);
            let head = line.strip_prefix("var x : ").unwrap().strip_suffix("...").unwrap();
            assert!(d.starts_with(head), "pad {pad:?}");
            assert!(d.is_char_boundary(head.len()), "pad {pad:?}: the cut lands inside a character");
            assert!(head.len() <= 160, "pad {pad:?}: the cut is past the limit");
            assert!(head.len() == 160 || !d.is_char_boundary(head.len() + 1), "pad {pad:?}: the cut is not maximal");
        }
        // The premise of the test: without this, both paddings could pass
        // while never putting a multibyte character across the limit.
        assert_eq!(straddled, 1, "no padding puts a multibyte character across byte 160");
    }

    /// ASCII lines are what they were before the boundary fix: printed whole
    /// when short, cut at the same byte when long (CODE-17).
    #[test]
    fn var_shape_keeps_ascii_lines_unchanged() {
        assert_eq!(var_shape("x", &Shape::Bool), "var x : Bool");
        let s = Shape::Atom(Arc::from([Value::Str(Sym::new(&"a".repeat(200)))]));
        let d = format!("{s:?}");
        assert_eq!(var_shape("x", &s), format!("var x : {}...", &d[..160]));
    }
}
