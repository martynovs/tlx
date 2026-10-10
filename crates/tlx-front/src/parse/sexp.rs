//! Compact s-expression rendering of the surface AST (for tests and the
//! `parse` example). Spans are omitted.
//!
//! Examples: `(+ a 1)`, `(/\ p q r)`, `(\E [<<x y>> \in S] [z \in T] P)`,
//! `(app f a b)`, `(except f ([1] .b) (+ @ 1))`, `(I(1)!Op 2)`.
//!
//! `write_expr` is the dispatcher: one arm per `ExprKind` variant, in the same
//! order, and no wildcard, so a new variant stays a compile error until it is
//! rendered. The long handlers of the three families that only dispatch from
//! here live below it, split by family (CODE-02, CODE-23): `binders` holds the
//! forms that introduce bound variables in a nested scope, `records` the
//! record, tuple and function forms, and `except` the update paths.

use std::fmt::Write;

use crate::ast::*;

pub fn expr(e: &Expr) -> String {
    let mut s = String::new();
    write_expr(&mut s, e);
    s
}

pub fn unit(u: &Unit) -> String {
    let mut s = String::new();
    write_unit(&mut s, u, 0);
    s
}

pub fn module(m: &Module) -> String {
    let mut s = String::new();
    write_module(&mut s, m, 0);
    s
}

fn indent(s: &mut String, depth: usize) {
    for _ in 0..depth {
        s.push_str("  ");
    }
}

fn write_module(s: &mut String, m: &Module, depth: usize) {
    let _ = write!(s, "(module {}", m.name.name);
    if !m.extends.is_empty() {
        s.push_str(" (extends");
        for e in &m.extends {
            let _ = write!(s, " {}", e.name);
        }
        s.push(')');
    }
    for u in &m.units {
        s.push('\n');
        indent(s, depth + 1);
        write_unit(s, u, depth + 1);
    }
    s.push(')');
}

fn decl(d: &OpDecl) -> String {
    if d.arity == 0 {
        d.name.name.to_string()
    } else {
        format!("{}/{}", d.name.name, d.arity)
    }
}

fn decls(ds: &[OpDecl]) -> String {
    ds.iter().map(decl).collect::<Vec<_>>().join(" ")
}

fn write_unit(s: &mut String, u: &Unit, depth: usize) {
    match u {
        Unit::Constants(ds) => {
            let _ = write!(s, "(constants {})", decls(ds));
        }
        Unit::Variables(vs) => {
            let names: Vec<&str> = vs.iter().map(|v| &*v.name).collect();
            let _ = write!(s, "(variables {})", names.join(" "));
        }
        Unit::Recursive(ds) => {
            let _ = write!(s, "(recursive {})", decls(ds));
        }
        Unit::OpDef(d) => {
            let _ = write!(s, "({} {} ({}) ", if d.local { "local-def" } else { "def" }, d.name.name, decls(&d.params));
            write_expr(s, &d.body);
            s.push(')');
        }
        Unit::FnDef(d) => {
            let _ = write!(s, "({} {} ", if d.local { "local-fndef" } else { "fndef" }, d.name.name);
            write_bounds(s, &d.bounds);
            s.push(' ');
            write_expr(s, &d.body);
            s.push(')');
        }
        Unit::Instance(i) => {
            s.push_str(if i.local { "(local-instance" } else { "(instance" });
            if let Some(n) = &i.name {
                let _ = write!(s, " {} ({})", n.name, decls(&i.params));
            }
            let _ = write!(s, " {}", i.module.name);
            for (a, e) in &i.substs {
                let _ = write!(s, " ({} <- ", a.name);
                write_expr(s, e);
                s.push(')');
            }
            s.push(')');
        }
        Unit::Assume { name, body, .. } => {
            s.push_str("(assume ");
            if let Some(n) = name {
                let _ = write!(s, "{} ", n.name);
            }
            write_expr(s, body);
            s.push(')');
        }
        Unit::Theorem { name, body, .. } => {
            let _ = write!(s, "(theorem {} ", name.name);
            write_expr(s, body);
            s.push(')');
        }
        Unit::Module(m) => write_module(s, m, depth),
    }
}

fn write_vars(s: &mut String, v: &BoundVars) {
    match v {
        BoundVars::Many(ids) => {
            let names: Vec<&str> = ids.iter().map(|i| &*i.name).collect();
            s.push_str(&names.join(" "));
        }
        BoundVars::Tuple(ids) => {
            let names: Vec<&str> = ids.iter().map(|i| &*i.name).collect();
            let _ = write!(s, "<<{}>>", names.join(" "));
        }
    }
}

fn write_bound(s: &mut String, b: &Bound) {
    s.push('[');
    write_vars(s, &b.vars);
    s.push_str(" \\in ");
    write_expr(s, &b.set);
    s.push(']');
}

fn write_bounds(s: &mut String, bs: &[Bound]) {
    for (i, b) in bs.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        write_bound(s, b);
    }
}

fn write_list(s: &mut String, head: &str, es: &[Expr]) {
    let _ = write!(s, "({head}");
    for e in es {
        s.push(' ');
        write_expr(s, e);
    }
    s.push(')');
}

fn quant(k: QuantKind) -> &'static str {
    match k {
        QuantKind::Forall => "\\A",
        QuantKind::Exists => "\\E",
    }
}

fn write_expr(s: &mut String, e: &Expr) {
    match &*e.kind {
        ExprKind::Num(n) => {
            let _ = write!(s, "{n}");
        }
        ExprKind::Decimal(d) => s.push_str(d),
        ExprKind::Str(t) => {
            let _ = write!(s, "{t:?}");
        }
        ExprKind::Bool(b) => s.push_str(if *b { "TRUE" } else { "FALSE" }),
        ExprKind::OpAppl { prefix, name, args } => {
            let mut head = String::new();
            for p in prefix {
                head.push_str(&p.name.name);
                if !p.args.is_empty() {
                    head.push('(');
                    for (i, a) in p.args.iter().enumerate() {
                        if i > 0 {
                            head.push_str(", ");
                        }
                        write_expr(&mut head, a);
                    }
                    head.push(')');
                }
                head.push('!');
            }
            head.push_str(&name.name);
            if args.is_empty() {
                s.push_str(&head);
            } else {
                write_list(s, &head, args);
            }
        }
        ExprKind::Subexpr { base, path } => {
            s.push_str("(subexpr ");
            write_expr(s, base);
            for sel in path {
                match sel {
                    Selector::Index(i) => {
                        let _ = write!(s, " !{i}");
                    }
                    Selector::Label(l, args) if args.is_empty() => {
                        let _ = write!(s, " !{}", l.name);
                    }
                    Selector::Label(l, args) => {
                        s.push(' ');
                        write_list(s, &format!("!{}", l.name), args);
                    }
                    Selector::Left => s.push_str(" !<<"),
                    Selector::Right => s.push_str(" !>>"),
                    Selector::At => s.push_str(" !@"),
                    Selector::Args(args) => {
                        s.push(' ');
                        write_list(s, "!", args);
                    }
                    Selector::Colon => s.push_str(" !:"),
                }
            }
            s.push(')');
        }
        ExprKind::Label { name, params, body } => {
            let _ = write!(s, "(label {}", name.name);
            if !params.is_empty() {
                let names: Vec<&str> = params.iter().map(|i| &*i.name).collect();
                let _ = write!(s, "({})", names.join(", "));
            }
            s.push(' ');
            write_expr(s, body);
            s.push(')');
        }
        ExprKind::Lambda { params, body } => {
            let names: Vec<&str> = params.iter().map(|i| &*i.name).collect();
            let _ = write!(s, "(LAMBDA [{}] ", names.join(" "));
            write_expr(s, body);
            s.push(')');
        }
        ExprKind::And(es) => write_list(s, "/\\", es),
        ExprKind::Or(es) => write_list(s, "\\/", es),
        ExprKind::Quant { kind, bounds, body } => write_expr_quant(s, kind, bounds, body),
        ExprKind::QuantUnbounded { kind, vars, body } | ExprKind::TemporalQuant { kind, vars, body } => {
            write_expr_quant_unbounded(s, e, kind, vars, body)
        }
        ExprKind::Choose { vars, set, body } => write_expr_choose(s, vars, set, body),
        ExprKind::If { cond, then, els } => write_list(s, "IF", &[cond.clone(), then.clone(), els.clone()]),
        ExprKind::Case { arms, other } => {
            s.push_str("(CASE");
            for (p, v) in arms {
                s.push_str(" (");
                write_expr(s, p);
                s.push(' ');
                write_expr(s, v);
                s.push(')');
            }
            if let Some(o) = other {
                s.push_str(" (OTHER ");
                write_expr(s, o);
                s.push(')');
            }
            s.push(')');
        }
        ExprKind::Let { defs, body } => write_expr_let(s, defs, body),
        ExprKind::SetEnum(es) => write_list(s, "set", es),
        ExprKind::SetFilter { bound, pred } => write_expr_set_filter(s, bound, pred),
        ExprKind::SetMap { expr, bounds } => write_expr_set_map(s, expr, bounds),
        ExprKind::Tuple(es) => write_expr_tuple(s, es),
        ExprKind::FnConstruct { bounds, body } => write_expr_fn_construct(s, bounds, body),
        ExprKind::FnApp { func, args } => write_expr_fn_app(s, func, args),
        ExprKind::FnSet { domain, range } => write_list(s, "->", &[domain.clone(), range.clone()]),
        ExprKind::Record(fs) | ExprKind::RecordSet(fs) => write_expr_record(s, e, fs),
        ExprKind::Field { record, field } => write_expr_field(s, record, field),
        ExprKind::Except { base, updates } => write_expr_except(s, base, updates),
        ExprKind::At => s.push('@'),
        ExprKind::BoxAction { action, sub } => write_list(s, "[]_", &[action.clone(), sub.clone()]),
        ExprKind::AngleAction { action, sub } => write_list(s, "<>_", &[action.clone(), sub.clone()]),
        ExprKind::Fairness { kind, sub, action } => {
            let head = match kind {
                FairnessKind::Weak => "WF",
                FairnessKind::Strong => "SF",
            };
            write_list(s, head, &[sub.clone(), action.clone()])
        }
    }
}

// ---- binders: the forms that introduce bound variables in a nested scope ----

fn write_expr_quant(s: &mut String, kind: &QuantKind, bounds: &[Bound], body: &Expr) {
    let _ = write!(s, "({} ", quant(*kind));
    write_bounds(s, bounds);
    s.push(' ');
    write_expr(s, body);
    s.push(')');
}

fn write_expr_quant_unbounded(s: &mut String, e: &Expr, kind: &QuantKind, vars: &[Ident], body: &Expr) {
    let temporal = matches!(&*e.kind, ExprKind::TemporalQuant { .. });
    let names: Vec<&str> = vars.iter().map(|i| &*i.name).collect();
    let q = if temporal { format!("{}{}", quant(*kind), &quant(*kind)[1..]) } else { quant(*kind).into() };
    let _ = write!(s, "({q} [{}] ", names.join(" "));
    write_expr(s, body);
    s.push(')');
}

fn write_expr_choose(s: &mut String, vars: &BoundVars, set: &Option<Expr>, body: &Expr) {
    s.push_str("(CHOOSE [");
    write_vars(s, vars);
    if let Some(set) = set {
        s.push_str(" \\in ");
        write_expr(s, set);
    }
    s.push_str("] ");
    write_expr(s, body);
    s.push(')');
}

fn write_expr_let(s: &mut String, defs: &[Unit], body: &Expr) {
    s.push_str("(LET [");
    for (i, d) in defs.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        write_unit(s, d, 0);
    }
    s.push_str("] ");
    write_expr(s, body);
    s.push(')');
}

// ---- records: set builders, tuples, functions and records ----

fn write_expr_set_filter(s: &mut String, bound: &Bound, pred: &Expr) {
    s.push_str("(filter ");
    write_bound(s, bound);
    s.push(' ');
    write_expr(s, pred);
    s.push(')');
}

fn write_expr_set_map(s: &mut String, expr: &Expr, bounds: &[Bound]) {
    s.push_str("(map ");
    write_expr(s, expr);
    s.push(' ');
    write_bounds(s, bounds);
    s.push(')');
}

fn write_expr_tuple(s: &mut String, es: &[Expr]) {
    s.push_str("<<");
    for (i, x) in es.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        write_expr(s, x);
    }
    s.push_str(">>");
}

fn write_expr_fn_construct(s: &mut String, bounds: &[Bound], body: &Expr) {
    s.push_str("(fn ");
    write_bounds(s, bounds);
    s.push(' ');
    write_expr(s, body);
    s.push(')');
}

fn write_expr_fn_app(s: &mut String, func: &Expr, args: &[Expr]) {
    s.push_str("(app ");
    write_expr(s, func);
    for a in args {
        s.push(' ');
        write_expr(s, a);
    }
    s.push(')');
}

fn write_expr_record(s: &mut String, e: &Expr, fs: &[(Ident, Expr)]) {
    let head = if matches!(&*e.kind, ExprKind::Record(_)) { "rec" } else { "recset" };
    let _ = write!(s, "({head}");
    for (f, v) in fs {
        let _ = write!(s, " ({} ", f.name);
        write_expr(s, v);
        s.push(')');
    }
    s.push(')');
}

fn write_expr_field(s: &mut String, record: &Expr, field: &Ident) {
    s.push_str("(. ");
    write_expr(s, record);
    let _ = write!(s, " {})", field.name);
}

// ---- EXCEPT ----

fn write_expr_except(s: &mut String, base: &Expr, updates: &[(Vec<ExceptKey>, Expr)]) {
    s.push_str("(except ");
    write_expr(s, base);
    for (path, v) in updates {
        s.push_str(" ((");
        for (i, k) in path.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            match k {
                ExceptKey::Field(f) => {
                    let _ = write!(s, ".{}", f.name);
                }
                ExceptKey::Index(args) => {
                    s.push('[');
                    for (j, a) in args.iter().enumerate() {
                        if j > 0 {
                            s.push(' ');
                        }
                        write_expr(s, a);
                    }
                    s.push(']');
                }
            }
        }
        s.push_str(") ");
        write_expr(s, v);
        s.push(')');
    }
    s.push(')');
}
