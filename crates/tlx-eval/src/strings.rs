//! Which definitions and expressions may create strings.
//!
//! Strings are ordered by creation, as in TLC (its `UniqueString` tokens are
//! assigned when a string is first created; tlc-semantics.md §1.3), so
//! evaluating such an expression ahead of the search (folding it at compile
//! time, probing it in the shape pre-run) would create its strings out of
//! TLC's order; and the constant definitions TLC evaluates at
//! startup must be evaluated then if they create strings.
//!
//! `Tail`, `SubSeq` and `\o` create strings only when applied to strings:
//! `StringMakers::for_states` knows which values the state variables can
//! take (a type inference over the initial predicate and the actions,
//! `types`), so that these operators on sequences do not count there;
//! `StringMakers::typed` does the same for expressions that read no
//! variable (constant folding), any variable being taken to hold strings.

mod types;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use tlx_sema::builtins::Builtin as B;
use tlx_sema::hir::{self, DefKind, Expr, ExprKind as H, OpRef};

use crate::env::{Binding, Env, OpVal};
use crate::Interp;
use types::Ty;

/// Builtins that may create a string.
pub fn makes_strings(op: B) -> bool {
    op.makes_strings()
}

/// Memoized analysis of the definitions of one interpreter.
pub struct StringMakers<'a> {
    ip: &'a Interp,
    memo: RefCell<HashMap<hir::DefId, bool>>,
    /// The values the state variables can take (`for_states`).
    vars: Option<Vec<Ty>>,
}

impl<'a> StringMakers<'a> {
    /// The syntactic analysis: `Tail`, `SubSeq` and `\o` always count.
    pub fn new(ip: &'a Interp) -> StringMakers<'a> {
        StringMakers { ip, memo: RefCell::new(HashMap::new()), vars: None }
    }

    /// The analysis of expressions evaluated on the states of a model with
    /// initial predicate `init` (evaluated with all variables unassigned)
    /// and `actions`: every other expression that may assign primed
    /// variables (the next-state actions, but also ENABLED in invariants or
    /// properties: all expressions the model evaluates). `Tail`, `SubSeq`
    /// and `\o` count only where their operand may be a string. Falls back
    /// to the syntactic analysis where the inference fails (see
    /// `types::Typer`).
    pub fn for_states(ip: &'a Interp, init: &Expr, actions: &[(&Expr, &Env)]) -> StringMakers<'a> {
        StringMakers { vars: types::infer_vars(ip, init, actions), ..StringMakers::new(ip) }
    }

    /// The type-aware analysis without knowing the values of the variables
    /// (any of them may be a string): for expressions that read none, such
    /// as the constant expressions the lowering folds.
    pub fn typed(ip: &'a Interp) -> StringMakers<'a> {
        StringMakers { vars: Some(vec![Ty::Top; ip.prog.vars.len()]), ..StringMakers::new(ip) }
    }

    /// Whether evaluating 0-ary definition `d` (in the states of the model
    /// for `for_states`) may create strings: the type-aware `def`.
    pub fn applied(&self, d: hir::DefId) -> bool {
        let d = self.ip.redirect_def(d);
        let call = Expr::new(H::Apply { op: OpRef::Def(d), args: Vec::new() }, self.ip.prog.defs[d.idx()].span);
        self.expr(&call)
    }

    /// Whether evaluating definition `d` may create strings. A constant-level
    /// definition whose value is cached creates none. (Syntactic.)
    pub fn def(&self, d: hir::DefId) -> bool {
        let d = self.ip.redirect_def(d);
        if self.ip.const_def_evaluated(d) {
            return false;
        }
        if let Some(v) = self.memo.borrow().get(&d) {
            return *v;
        }
        // Provisionally not (recursion through d).
        self.memo.borrow_mut().insert(d, false);
        let def = &self.ip.prog.defs[d.idx()];
        let v = def.builtin.is_some_and(makes_strings)
            || match &def.kind {
                DefKind::Op(b) => self.syntactic(b),
                DefKind::Fn { bounds, body } => self.syntactic(body) || bounds.iter().any(|b| self.syntactic(&b.set)),
                DefKind::Pending => true,
            };
        self.memo.borrow_mut().insert(d, v);
        v
    }

    /// Whether evaluating `e` may create strings. (The operators passed to
    /// an operator parameter are checked where they are passed: a LAMBDA is
    /// a subexpression, a definition an `OpArg`.)
    pub fn expr(&self, e: &hir::Expr) -> bool {
        self.expr_in(e, &Env::default())
    }

    /// Whether evaluating `e` in `env` may create strings.
    pub fn expr_in(&self, e: &hir::Expr, env: &Env) -> bool {
        // (The syntactic analysis covers the typed one: it is the cheap
        // first check.)
        if !self.syntactic_in(e, env) {
            return false;
        }
        if let Some(b) = self.vars.as_ref().and_then(|vars| types::creates_strings(self.ip, vars, e, env)) {
            return b;
        }
        true
    }

    /// The syntactic analysis of `e` in `env`: also of the arguments `env`
    /// binds by name and the LAMBDAs it binds (an action instance's body
    /// may see `Tail(q)` only through a parameter: `\E s \in S : A(Tail(q))`).
    fn syntactic_in(&self, e: &hir::Expr, env: &Env) -> bool {
        if self.syntactic(e) {
            return true;
        }
        let mut seen = HashSet::new();
        let mut todo = vec![env.clone()];
        while let Some(env) = todo.pop() {
            let mut found = false;
            env.for_each_binding(&mut |b| {
                let (x, xenv) = match b {
                    Binding::Op(OpVal::Lambda { body, env, .. }) => (body, env),
                    b => match b.by_name(&self.ip.prog) {
                        Some(n) => n,
                        None => return,
                    },
                };
                if seen.insert((std::sync::Arc::as_ptr(&x.kind) as usize, xenv.id())) {
                    found = found || self.syntactic(x);
                    todo.push(xenv.clone());
                }
            });
            if found {
                return true;
            }
        }
        false
    }

    fn syntactic(&self, e: &hir::Expr) -> bool {
        let here = match &*e.kind {
            H::Builtin { op, .. } => makes_strings(*op),
            H::Apply { op: OpRef::Def(d), .. } | H::OpArg(OpRef::Def(d)) => self.def(*d),
            H::Apply { op: OpRef::Const(c), .. } => matches!(self.ip.const_op_ref(*c), Some(OpRef::Def(d)) if self.def(d)),
            _ => false,
        };
        let mut found = here;
        if !found {
            tlx_sema::visit::children(e, &mut |c| found = found || self.syntactic(c));
        }
        found
    }
}
