//! Surface AST: the output of CST lowering, before name resolution.
//!
//! Conventions:
//! - Every operator application (user-defined, standard-module or TLA+ core) is
//!   `ExprKind::OpAppl` with a canonical operator *name* (see `opnames`). Infix /
//!   prefix / postfix symbols are normalized: `\union` -> `\cup`, `=<`/`\leq` -> `<=`,
//!   `#` -> `/=`, unary minus -> `-.`, `x'` -> `'`, etc.
//! - Constructs that bind variables or have special syntax get dedicated variants.
//! - Parentheses are dropped. Labels `l(x, y):: e` are kept
//!   (`ExprKind::Label`) so that subexpression selectors (`Def!l`) can find
//!   them.
//! - Junction lists and binary `/\` `\/` both become `And` / `Or`, not
//!   flattened (`a /\ b /\ c` is `And[And[a, b], c]`, as SANY sees it for
//!   subexpression selectors); the resolver flattens them.

use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct FileId(pub u32);

/// Source location. `start`/`end` are byte offsets; `line`/`col` are 1-based
/// and refer to `start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ident {
    pub name: Arc<str>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub name: Ident,
    pub extends: Vec<Ident>,
    pub units: Vec<Unit>,
    /// Theorems (with their proofs) and USE / HIDE, kept only for SANY's
    /// checks: `(k, unit)` follows `units[..k]`. Named theorems whose
    /// statement is an expression are also in `units` (`Unit::Theorem`).
    pub proofs: Vec<(usize, ProofUnit)>,
    pub span: Span,
}

/// A theorem or a module-level USE / HIDE (see `Module::proofs`).
#[derive(Clone, Debug)]
pub enum ProofUnit {
    /// `THEOREM [Name ==] stmt [proof]` (also LEMMA, PROPOSITION, COROLLARY).
    Theorem {
        name: Option<Ident>,
        stmt: Stmt,
        proof: Option<Proof>,
        span: Span,
    },
    UseOrHide(UseOrHide),
}

/// A theorem's (or proof step's) assertion.
#[derive(Clone, Debug)]
pub enum Stmt {
    Expr(Expr),
    AssumeProve(AssumeProve),
}

/// `ASSUME a1, ..., an PROVE e`
#[derive(Clone, Debug)]
pub struct AssumeProve {
    pub assumes: Vec<Assumption>,
    pub prove: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Assumption {
    Expr(Expr),
    New(NewDecl),
    AssumeProve(AssumeProve),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewKind {
    Constant,
    Variable,
    State,
    Action,
    Temporal,
}

/// `NEW [kind] x [\in S]` / `NEW CONSTANT F(_, _)`.
#[derive(Clone, Debug)]
pub struct NewDecl {
    pub kind: NewKind,
    pub decl: OpDecl,
    pub set: Option<Expr>,
    pub span: Span,
}

/// `USE` / `HIDE` (also the body of `BY`).
#[derive(Clone, Debug)]
pub struct UseOrHide {
    pub hide: bool,
    pub body: UseBody,
    pub span: Span,
}

/// `facts DEF defs`
#[derive(Clone, Debug, Default)]
pub struct UseBody {
    pub facts: Vec<Fact>,
    pub defs: Vec<Fact>,
}

#[derive(Clone, Debug)]
pub enum Fact {
    Expr(Expr),
    /// `MODULE M`
    Module(Ident),
}

#[derive(Clone, Debug)]
pub enum Proof {
    /// `BY ...`, `OBVIOUS`, `OMITTED`.
    Leaf {
        by: Option<UseBody>,
        span: Span,
    },
    Steps(Vec<Step>),
}

/// Identifiers introduced by TAKE / PICK.
#[derive(Clone, Debug)]
pub enum Binders {
    Bounded(Vec<Bound>),
    Unbounded(Vec<Ident>),
}

/// A proof step `<level>name kind [proof]`.
#[derive(Clone, Debug)]
pub struct Step {
    /// `<1>a` (`<1>` for an unnamed step; `<*>` / `<+>` levels as written).
    pub id: Ident,
    pub kind: StepKind,
    pub proof: Option<Proof>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum StepKind {
    /// An assertion (`e` or `ASSUME ... PROVE ...`).
    Assert(Stmt),
    Suffices(Stmt),
    /// `DEFINE` (OpDef / FnDef / Instance units).
    Define(Vec<Unit>),
    Have(Expr),
    Witness(Vec<Expr>),
    Take(Binders),
    Case(Expr),
    Pick(Binders, Expr),
    UseOrHide(UseOrHide),
    Instance(Instance),
    Qed,
}

#[derive(Clone, Debug)]
pub enum Unit {
    /// `CONSTANT a, F(_, _)`
    Constants(Vec<OpDecl>),
    /// `VARIABLE x, y`
    Variables(Vec<Ident>),
    OpDef(OpDef),
    FnDef(FnDef),
    Instance(Instance),
    /// `RECURSIVE F(_), G`
    Recursive(Vec<OpDecl>),
    /// `ASSUME e` / `ASSUME Name == e`
    Assume {
        name: Option<Ident>,
        body: Expr,
        span: Span,
    },
    /// Named `THEOREM Name == e` (also LEMMA, PROPOSITION, COROLLARY) whose
    /// statement is an expression; it can be referenced like a definition.
    /// All theorems, with their proofs, are also in `Module::proofs`.
    Theorem {
        name: Ident,
        body: Expr,
        span: Span,
    },
    /// Nested `---- MODULE Inner ----`.
    Module(Module),
}

/// A declared operator: `c` (arity 0) or `F(_, _)` (arity 2).
#[derive(Clone, Debug)]
pub struct OpDecl {
    pub name: Ident,
    pub arity: usize,
}

/// Formal parameter of an operator definition: `x` or `F(_, _)`.
pub type OpParam = OpDecl;

/// `Name(params) == body`, including infix/prefix/postfix definitions
/// (`a ++ b == ...` is `OpDef { name: "++", params: [a, b] }`).
#[derive(Clone, Debug)]
pub struct OpDef {
    pub name: Ident,
    pub params: Vec<OpParam>,
    pub body: Expr,
    pub local: bool,
    pub span: Span,
}

/// `f[x \in S, y \in T] == body`
#[derive(Clone, Debug)]
pub struct FnDef {
    pub name: Ident,
    pub bounds: Vec<Bound>,
    pub body: Expr,
    pub local: bool,
    pub span: Span,
}

/// `[LOCAL] [Name[(params)] ==] INSTANCE M [WITH a <- e, ...]`
#[derive(Clone, Debug)]
pub struct Instance {
    /// `None` for an anonymous `INSTANCE M` (its definitions are imported directly).
    pub name: Option<Ident>,
    pub params: Vec<OpParam>,
    pub module: Ident,
    pub substs: Vec<(Ident, Expr)>,
    pub local: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: Box<ExprKind>,
    pub span: Span,
}

/// Bound variables of a quantifier/set-former: `x, y \in S` or `<<x, y>> \in S`.
#[derive(Clone, Debug)]
pub enum BoundVars {
    /// `x, y \in S` binds each of x, y independently to S.
    Many(Vec<Ident>),
    /// `<<x, y>> \in S` destructures each element of S.
    Tuple(Vec<Ident>),
}

#[derive(Clone, Debug)]
pub struct Bound {
    pub vars: BoundVars,
    pub set: Expr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuantKind {
    Forall,
    Exists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FairnessKind {
    Weak,
    Strong,
}

/// One component of a module-instance reference path: `I(a)!` or `M!`.
#[derive(Clone, Debug)]
pub struct NamePrefix {
    pub name: Ident,
    pub args: Vec<Expr>,
}

/// One component of an EXCEPT path: `.f` or `[e1, e2]`.
#[derive(Clone, Debug)]
pub enum ExceptKey {
    Field(Ident),
    Index(Vec<Expr>),
}

/// One step of a subexpression selector path (`Def!2!lab!:`).
#[derive(Clone, Debug)]
pub enum Selector {
    /// `!n` (1-based): the n-th operand.
    Index(usize),
    /// `!<<`: the first operand.
    Left,
    /// `!>>`: the second operand of a binary operator.
    Right,
    /// `!@`: the body of a binding construct, its bound identifiers free.
    At,
    /// `!(e1, ..., en)`: the body of a binding construct (or LAMBDA) with
    /// its bound identifiers replaced by the arguments.
    Args(Vec<Expr>),
    /// `!lab` / `!lab(e1, ..., en)`: the subexpression labeled `lab`, its
    /// parameters replaced by the arguments.
    Label(Ident, Vec<Expr>),
    /// `!:`: the body of the definition / theorem itself.
    Colon,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    // ---- literals ----
    /// Natural number literal (decimal, `\b`, `\o`, `\h` forms all normalized).
    Num(i128),
    /// Decimal literal `1.5`, kept as text.
    Decimal(Arc<str>),
    /// String literal, escapes already decoded.
    Str(Arc<str>),
    Bool(bool),

    // ---- names and application ----
    /// Operator application, including plain identifiers (args empty),
    /// built-in symbols (`+`, `\in`, `'`, `UNCHANGED`, `SUBSET`, `DOMAIN`, ...),
    /// and instance-qualified names (`I!Op(x)` -> prefix [I], name Op).
    OpAppl {
        prefix: Vec<NamePrefix>,
        name: Ident,
        args: Vec<Expr>,
    },
    /// Subexpression reference `base!sel!sel...`: `base` is the `OpAppl`
    /// naming the definition (with its instance prefix and arguments, e.g.
    /// `I!Op(a)` in `I!Op(a)!2`). References of the form `Def!lab` without
    /// any positional selector stay plain `OpAppl`s with prefix `[Def]`; the
    /// resolver tells them apart from instance references.
    Subexpr {
        base: Expr,
        path: Vec<Selector>,
    },
    /// `name(params):: body`.
    Label {
        name: Ident,
        params: Vec<Ident>,
        body: Expr,
    },
    /// `LAMBDA x, y : body` (only as an operator argument).
    Lambda {
        params: Vec<Ident>,
        body: Expr,
    },

    // ---- logic ----
    And(Vec<Expr>),
    Or(Vec<Expr>),
    /// `\A x \in S : P` / `\E x \in S : P`
    Quant {
        kind: QuantKind,
        bounds: Vec<Bound>,
        body: Expr,
    },
    /// `\A x : P` / `\E x : P`
    QuantUnbounded {
        kind: QuantKind,
        vars: Vec<Ident>,
        body: Expr,
    },
    /// `\AA x : P` / `\EE x : P` (temporal; parsed but unsupported).
    TemporalQuant {
        kind: QuantKind,
        vars: Vec<Ident>,
        body: Expr,
    },
    /// `CHOOSE x \in S : P` (set = None for unbounded CHOOSE).
    Choose {
        vars: BoundVars,
        set: Option<Expr>,
        body: Expr,
    },

    // ---- control ----
    If {
        cond: Expr,
        then: Expr,
        els: Expr,
    },
    /// `CASE p1 -> e1 [] p2 -> e2 [] OTHER -> e`
    Case {
        arms: Vec<(Expr, Expr)>,
        other: Option<Expr>,
    },
    /// `LET defs IN body`; defs are OpDef / FnDef / Recursive units.
    Let {
        defs: Vec<Unit>,
        body: Expr,
    },

    // ---- sets ----
    SetEnum(Vec<Expr>),
    /// `{x \in S : P}`
    SetFilter {
        bound: Bound,
        pred: Expr,
    },
    /// `{e : x \in S, y \in T}`
    SetMap {
        expr: Expr,
        bounds: Vec<Bound>,
    },

    // ---- functions, tuples, records ----
    Tuple(Vec<Expr>),
    /// `[x \in S, y \in T |-> e]`
    FnConstruct {
        bounds: Vec<Bound>,
        body: Expr,
    },
    /// `f[a]` or `f[a, b]` (args has length >= 1; `f[a, b]` means `f[<<a, b>>]`).
    FnApp {
        func: Expr,
        args: Vec<Expr>,
    },
    /// `[S -> T]`
    FnSet {
        domain: Expr,
        range: Expr,
    },
    /// `[a |-> e, b |-> f]`
    Record(Vec<(Ident, Expr)>),
    /// `[a : S, b : T]`
    RecordSet(Vec<(Ident, Expr)>),
    /// `r.field`
    Field {
        record: Expr,
        field: Ident,
    },
    /// `[f EXCEPT ![a].b = e, !.c = d]`
    Except {
        base: Expr,
        updates: Vec<(Vec<ExceptKey>, Expr)>,
    },
    /// `@` inside an EXCEPT update value.
    At,

    // ---- actions / temporal ----
    /// `[A]_v`
    BoxAction {
        action: Expr,
        sub: Expr,
    },
    /// `<<A>>_v`
    AngleAction {
        action: Expr,
        sub: Expr,
    },
    /// `WF_v(A)` / `SF_v(A)`
    Fairness {
        kind: FairnessKind,
        sub: Expr,
        action: Expr,
    },
}

/// Canonical operator names used in `ExprKind::OpAppl` for built-in symbols.
/// The CST lowering must map every alias to exactly one of these.
pub mod opnames {
    // TLA+ core (always in scope)
    pub const EQ: &str = "=";
    pub const NEQ: &str = "/="; // also `#`
    pub const IN: &str = "\\in";
    pub const NOTIN: &str = "\\notin";
    pub const NOT: &str = "~"; // also `\lnot`, `\neg`
    pub const IMPLIES: &str = "=>";
    pub const EQUIV: &str = "<=>"; // also `\equiv`
    pub const PRIME: &str = "'";
    pub const UNCHANGED: &str = "UNCHANGED";
    pub const ENABLED: &str = "ENABLED";
    pub const SUBSET: &str = "SUBSET"; // powerset
    pub const UNION: &str = "UNION"; // big union
    pub const DOMAIN: &str = "DOMAIN";
    pub const CUP: &str = "\\cup"; // also `\union`
    pub const CAP: &str = "\\cap"; // also `\intersect`
    pub const SETMINUS: &str = "\\"; // set difference
    pub const SUBSETEQ: &str = "\\subseteq";
    pub const TIMES: &str = "\\X"; // also `\times`; n-ary when unparenthesized
    pub const ALWAYS: &str = "[]";
    pub const EVENTUALLY: &str = "<>";
    pub const LEADSTO: &str = "~>";
    pub const COMPOSE: &str = "\\cdot";
    // Standard modules (resolved only if the module is in scope)
    pub const PLUS: &str = "+";
    pub const MINUS: &str = "-";
    pub const NEG: &str = "-."; // unary minus
    pub const MUL: &str = "*";
    pub const POW: &str = "^";
    pub const DIV: &str = "\\div";
    pub const MOD: &str = "%";
    pub const LT: &str = "<";
    pub const GT: &str = ">";
    pub const LE: &str = "<="; // also `=<`, `\leq`
    pub const GE: &str = ">="; // also `\geq`
    pub const DOTDOT: &str = "..";
    pub const CONCAT: &str = "\\o"; // also `\circ`
    pub const COLONGT: &str = ":>";
    pub const ATAT: &str = "@@";
}

/// Visit `e` and all of its subexpressions (pre-order), including those of
/// LET definitions, instance-path arguments, EXCEPT paths and subexpression
/// selector arguments.
pub fn walk(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    let mut go = |x: &Expr| walk(x, f);
    match &*e.kind {
        ExprKind::Num(_) | ExprKind::Decimal(_) | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::At => {}
        ExprKind::OpAppl { prefix, args, .. } => {
            prefix.iter().flat_map(|p| &p.args).for_each(&mut go);
            args.iter().for_each(&mut go);
        }
        ExprKind::Subexpr { base, path } => {
            go(base);
            // Selector argument expressions (`!lab(e, ...)`, `!(e, ...)`) are
            // subexpressions of the reference too, in source order.
            for sel in path {
                match sel {
                    Selector::Args(es) | Selector::Label(_, es) => es.iter().for_each(&mut go),
                    Selector::Index(_) | Selector::Left | Selector::Right | Selector::At | Selector::Colon => {}
                }
            }
        }
        ExprKind::Label { body, .. }
        | ExprKind::Lambda { body, .. }
        | ExprKind::QuantUnbounded { body, .. }
        | ExprKind::TemporalQuant { body, .. } => go(body),
        ExprKind::And(es) | ExprKind::Or(es) | ExprKind::SetEnum(es) | ExprKind::Tuple(es) => es.iter().for_each(&mut go),
        ExprKind::Quant { bounds, body, .. } | ExprKind::FnConstruct { bounds, body } => {
            bounds.iter().for_each(|b| go(&b.set));
            go(body);
        }
        ExprKind::Choose { set, body, .. } => {
            set.iter().for_each(&mut go);
            go(body);
        }
        ExprKind::If { cond, then, els } => {
            go(cond);
            go(then);
            go(els);
        }
        ExprKind::Case { arms, other } => {
            for (g, v) in arms {
                go(g);
                go(v);
            }
            other.iter().for_each(&mut go);
        }
        ExprKind::Let { defs, body } => {
            for d in defs {
                match d {
                    Unit::OpDef(od) => go(&od.body),
                    Unit::FnDef(fd) => {
                        fd.bounds.iter().for_each(|b| go(&b.set));
                        go(&fd.body);
                    }
                    Unit::Instance(inst) => inst.substs.iter().for_each(|(_, v)| go(v)),
                    _ => {}
                }
            }
            go(body);
        }
        ExprKind::SetFilter { bound, pred } => {
            go(&bound.set);
            go(pred);
        }
        ExprKind::SetMap { expr, bounds } => {
            bounds.iter().for_each(|b| go(&b.set));
            go(expr);
        }
        ExprKind::FnApp { func, args } => {
            go(func);
            args.iter().for_each(&mut go);
        }
        ExprKind::FnSet { domain, range } => {
            go(domain);
            go(range);
        }
        ExprKind::Record(fs) | ExprKind::RecordSet(fs) => fs.iter().for_each(|(_, v)| go(v)),
        ExprKind::Field { record, .. } => go(record),
        ExprKind::Except { base, updates } => {
            go(base);
            for (keys, v) in updates {
                for k in keys {
                    if let ExceptKey::Index(ix) = k {
                        ix.iter().for_each(&mut go);
                    }
                }
                go(v);
            }
        }
        ExprKind::BoxAction { action, sub } | ExprKind::AngleAction { action, sub } => {
            go(action);
            go(sub);
        }
        ExprKind::Fairness { sub, action, .. } => {
            go(sub);
            go(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(name: &str) -> Ident {
        Ident { name: Arc::from(name), span: Span::default() }
    }

    /// A leaf `OpAppl` named `name`: its name identifies it in a visit list.
    fn leaf(name: &str) -> Expr {
        Expr { kind: Box::new(ExprKind::OpAppl { prefix: Vec::new(), name: ident(name), args: Vec::new() }), span: Span::default() }
    }

    /// The names of the `OpAppl` nodes `walk` visits, in visit order.
    fn visited(e: &Expr) -> Vec<String> {
        let mut out = Vec::new();
        walk(e, &mut |x| {
            if let ExprKind::OpAppl { name, .. } = &*x.kind {
                out.push(name.name.to_string());
            }
        });
        out
    }

    #[test]
    fn walk_visits_selector_arguments() {
        // `I!F(pa)(ba)!<<`-shaped: a base with an instance prefix and
        // arguments, and a path whose `Args` and `Label` selectors carry
        // argument expressions.
        let root = Expr {
            kind: Box::new(ExprKind::Subexpr {
                base: Expr {
                    kind: Box::new(ExprKind::OpAppl {
                        prefix: vec![NamePrefix { name: ident("I"), args: vec![leaf("pa")] }],
                        name: ident("F"),
                        args: vec![leaf("ba")],
                    }),
                    span: Span::default(),
                },
                path: vec![Selector::Args(vec![leaf("a1"), leaf("a2")]), Selector::Label(ident("L"), vec![leaf("l1")]), Selector::Index(2)],
            }),
            span: Span::default(),
        };
        assert_eq!(visited(&root), ["F", "pa", "ba", "a1", "a2", "l1"]);
        let mut n = 0;
        walk(&root, &mut |_| n += 1);
        assert_eq!(n, 7, "the subexpression, its base and five argument expressions");
    }
}
