//! Operator applications: plain and instance-qualified names, and the
//! substitution of instantiated modules' declarations.

use std::sync::Arc;

use tlx_front::ast::{self, Span};

use super::{CtxId, Env, LocalBinding, Program, Sym, ROOT_CTX};
use crate::builtins::Builtin;
use crate::hir::*;

/// `Local` references to the given locals.
fn local_refs(ls: &[LocalId], span: Span) -> Vec<Expr> {
    ls.iter().map(|l| Expr::new(ExprKind::Local(*l), span)).collect()
}

impl Program {
    /// What an unqualified `name` denotes in `env`: a local binding (`Ok`) or a
    /// module-scope symbol (`Err`).
    fn lookup(&mut self, env: &Env, name: &str) -> Option<Result<LocalBinding, Sym>> {
        if let Some(b) = env.local(name) {
            return Some(Ok(b.clone()));
        }
        let sc = self.scope(env.ctx, env.module);
        self.scopes[sc].names.get(name).map(|s| Err(*s))
    }

    /// Resolve `prefix!name(args)` followed by subexpression selectors `path`.
    #[allow(clippy::too_many_arguments)] // the argument set of the hot-path/dispatch signature; a bundling struct would only rename them
    pub(super) fn op_appl(
        &mut self,
        env: &mut Env,
        prefix: &[ast::NamePrefix],
        name: &ast::Ident,
        args: &[ast::Expr],
        path: &[ast::Selector],
        op_arg: bool,
        span: Span,
    ) -> Result<Expr, ()> {
        if !prefix.is_empty() {
            return self.qualified_appl(env, prefix, name, args, path, op_arg, span);
        }
        // `ENABLED A` in an instance: A's primed variables of the
        // instantiated module are not substituted (see `enabled_ctx`).
        let fresh = match &*name.name {
            "ENABLED" if self.lookup(env, &name.name).is_none() => self.enabled_ctx(env.ctx),
            _ => None,
        };
        let args_r = match fresh {
            Some(c) => {
                let outer = std::mem::replace(&mut env.ctx, c);
                let r = self.arg_exprs(env, args);
                env.ctx = outer;
                r
            }
            None => self.arg_exprs(env, args),
        };
        let found = self.lookup(env, &name.name);
        if !path.is_empty() {
            // `D!sel`: select a subexpression of definition D.
            let d = match found {
                Some(Ok(LocalBinding::LetDef(d))) | Some(Err(Sym::Def(d))) => d,
                _ => return self.fail(span, format!("`{}` is not a definition (in a subexpression reference)", name.name)),
            };
            let mut args = args_r?;
            let (sd, extra) = self.select(env, d, path, op_arg && args.is_empty(), span)?;
            args.extend(self.arg_exprs(env, &extra)?);
            return self.apply_def(env, sd, args, span);
        }
        match found {
            Some(Ok(LocalBinding::Local(l, arity))) => {
                let args = args_r?;
                if arity == 0 {
                    if !args.is_empty() {
                        return self.fail(span, format!("{} takes no arguments", name.name));
                    }
                    Ok(Expr::new(ExprKind::Local(l), span))
                } else {
                    Ok(apply_op(OpRef::Param(l), args, span))
                }
            }
            Some(Ok(LocalBinding::LetDef(d))) => {
                let args = args_r?;
                self.apply_def(env, d, args, span)
            }
            Some(Ok(LocalBinding::Instance(_))) => self.fail(span, format!("instance `{}` used as an expression", name.name)),
            Some(Ok(LocalBinding::Value(v))) => {
                if !args_r?.is_empty() {
                    return self.fail(span, format!("{} takes no arguments", name.name));
                }
                Ok(v)
            }
            Some(Err(sym)) => {
                let args = args_r?;
                self.apply_sym(env, sym, &name.name, args, span)
            }
            None => {
                let args = args_r?;
                match Builtin::core(&name.name) {
                    Some(b) => self.apply_builtin(b, args, span),
                    None if matches!(&*name.name, "/\\" | "\\/") => {
                        // `/\` / `\/` as an operator (argument).
                        let is_and = &*name.name == "/\\";
                        let junction = |es| if is_and { ExprKind::And(es) } else { ExprKind::Or(es) };
                        if !args.is_empty() {
                            return Ok(Expr::new(junction(args), span));
                        }
                        let params: Vec<LocalId> = ["a", "b"].iter().map(|n| self.new_local((*n).into(), 0, span)).collect();
                        let body = Expr::new(junction(local_refs(&params, span)), span);
                        Ok(Expr::new(ExprKind::Lambda { params, body }, span))
                    }
                    None => self.fail(span, format!("unknown identifier `{}`", name.name)),
                }
            }
        }
    }

    pub(super) fn apply_builtin(&mut self, b: Builtin, args: Vec<Expr>, span: Span) -> Result<Expr, ()> {
        let ar = b.arity();
        if args.is_empty() && ar > 0 {
            return Ok(Expr::new(ExprKind::OpArg(OpRef::Builtin(b)), span));
        }
        if ar >= 0 && args.len() != ar as usize {
            return self.fail(span, format!("`{}` expects {} argument(s), got {}", b.name(), ar, args.len()));
        }
        Ok(Expr::new(ExprKind::Builtin { op: b, args }, span))
    }

    /// Apply definition `d` from within `env` (passing on `env`'s instance params).
    fn apply_def(&mut self, env: &Env, d: DefId, args: Vec<Expr>, span: Span) -> Result<Expr, ()> {
        if let Some(b) = self.defs[d.idx()].builtin {
            if args.is_empty() && self.def_arity(d) > 0 {
                return Ok(Expr::new(ExprKind::OpArg(OpRef::Builtin(b)), span));
            }
            return self.apply_builtin(b, args, span);
        }
        let inst_args = local_refs(&env.inst_locals, span);
        self.apply_user_def(d, inst_args, args, span)
    }

    /// Apply definition `d` (or subexpression `path` of it) reached through an
    /// instance path; `inst_args` are the arguments of the innermost instance
    /// context's params.
    #[allow(clippy::too_many_arguments)] // the argument set of the hot-path/dispatch signature; a bundling struct would only rename them
    fn apply_qualified_def(
        &mut self,
        env: &mut Env,
        d: DefId,
        inst_args: Vec<Expr>,
        args: Vec<Expr>,
        path: &[ast::Selector],
        op_arg: bool,
        span: Span,
    ) -> Result<Expr, ()> {
        let (d, extra) = self.select(env, d, path, op_arg && args.is_empty(), span)?;
        let mut args = args;
        args.extend(self.arg_exprs(env, &extra)?);
        if let Some(b) = self.defs[d.idx()].builtin {
            return self.apply_builtin(b, args, span);
        }
        self.apply_user_def(d, inst_args, args, span)
    }

    /// Apply a non-builtin definition. `inst_args` start with the values of
    /// `d`'s instance params (extra trailing ones are dropped).
    fn apply_user_def(&mut self, d: DefId, mut inst_args: Vec<Expr>, args: Vec<Expr>, span: Span) -> Result<Expr, ()> {
        self.enqueue(d);
        let arity = self.def_arity(d);
        let n = self.def_inst[d.idx()];
        if args.is_empty() && arity > 0 {
            if n > 0 {
                // `I(x)!Op` as an argument: `LAMBDA p1, ..., pk : I(x)!Op(p1, ..., pk)`.
                let arities: Vec<usize> = match &self.def_src[d.idx()].unit {
                    super::DefUnit::Op(o) => o.params.iter().map(|p| p.arity).collect(),
                    super::DefUnit::Fn(_) => Vec::new(),
                };
                let params: Vec<LocalId> =
                    arities.iter().enumerate().map(|(i, &a)| self.new_local(format!("p{}", i + 1).into(), a, span)).collect();
                inst_args.truncate(n);
                inst_args.extend(params.iter().zip(&arities).map(|(&l, &a)| {
                    if a == 0 {
                        Expr::new(ExprKind::Local(l), span)
                    } else {
                        apply_op(OpRef::Param(l), vec![], span)
                    }
                }));
                let body = Expr::new(ExprKind::Apply { op: OpRef::Def(d), args: inst_args }, span);
                return Ok(Expr::new(ExprKind::Lambda { params, body }, span));
            }
            return Ok(Expr::new(ExprKind::OpArg(OpRef::Def(d)), span));
        }
        if args.len() != arity {
            let name = self.defs[d.idx()].name.clone();
            return self.fail(span, format!("`{name}` expects {arity} argument(s), got {}", args.len()));
        }
        debug_assert!(inst_args.len() >= n, "missing instance arguments for `{}`", self.defs[d.idx()].name);
        inst_args.truncate(n);
        inst_args.extend(args);
        Ok(Expr::new(ExprKind::Apply { op: OpRef::Def(d), args: inst_args }, span))
    }

    fn apply_sym(&mut self, env: &mut Env, sym: Sym, name: &str, args: Vec<Expr>, span: Span) -> Result<Expr, ()> {
        match sym {
            Sym::Var(v) => {
                if !args.is_empty() {
                    return self.fail(span, format!("variable `{name}` applied to arguments"));
                }
                Ok(Expr::new(ExprKind::Var(v), span))
            }
            Sym::Const(c) => {
                if self.consts[c.idx()].arity == 0 {
                    if !args.is_empty() {
                        return self.fail(span, format!("constant `{name}` applied to arguments"));
                    }
                    Ok(Expr::new(ExprKind::Const(c), span))
                } else {
                    Ok(apply_op(OpRef::Const(c), args, span))
                }
            }
            Sym::Def(d) => self.apply_def(env, d, args, span),
            Sym::Decl { ctx, arity, var } => {
                let n = self.ctxs[ctx].inst_params.len();
                if env.inst_locals.len() < n {
                    return self.fail(span, "internal: instance parameter mismatch");
                }
                let vals = local_refs(&env.inst_locals[..n], span);
                let subst = self.substitute(ctx, &vals, name, arity, args, span)?;
                if var && self.ctxs[ctx].copy_of.is_some() {
                    let slot = self.inst_var_slot(ctx, name, span);
                    return Ok(Expr::new(ExprKind::InstVar { slot, subst }, span));
                }
                Ok(subst)
            }
            Sym::Instance(_) => self.fail(span, format!("instance `{name}` used as an expression")),
        }
    }

    /// Replace a reference to declaration `name` of instance context `ctx` by
    /// its substitution, resolved in the instantiating module.
    /// `inst_vals` are the values of all of `ctx`'s instance params as seen
    /// from the reference site.
    pub(super) fn substitute(
        &mut self,
        ctx: CtxId,
        inst_vals: &[Expr],
        name: &str,
        arity: usize,
        args: Vec<Expr>,
        span: Span,
    ) -> Result<Expr, ()> {
        let (outer_ctx, outer_mod, outer_inst, sub, captured) = {
            let s = self.ctxs[ctx].subst.as_ref().expect("Decl only exists in instance contexts");
            (s.outer_ctx, s.outer_mod, s.outer_inst, s.explicit.get(name).cloned(), s.captured.clone())
        };
        if inst_vals.len() != self.ctxs[ctx].inst_params.len() {
            return self.fail(span, "internal: instance parameter mismatch");
        }
        // The substitution sees the outer context's instance params as locals...
        let mut senv = Env::new(outer_ctx, outer_mod);
        // ... the names bound around a LET INSTANCE ...
        if let Some(cap) = &captured {
            senv.names = cap.to_vec();
            senv.captured = senv.names.len();
        }
        for v in &inst_vals[..outer_inst] {
            match &*v.kind {
                ExprKind::Local(l) => senv.inst_locals.push(*l),
                _ => return self.fail(span, "unsupported: declaration reference through nested parameterized instances"),
            }
        }
        // ... and the instance's own params by name.
        let own_params: Vec<(Arc<str>, usize)> = self.ctxs[ctx].inst_params[outer_inst..].to_vec();
        for ((n, a), v) in own_params.into_iter().zip(&inst_vals[outer_inst..]) {
            let b = match &*v.kind {
                ExprKind::Local(l) => LocalBinding::Local(*l, a),
                _ if a == 0 => LocalBinding::Value(v.clone()),
                _ => return self.fail(span, "unsupported: operator-valued instance parameter"),
            };
            senv.names.push((n, b));
        }
        // Without an explicit `WITH name <- e`, the substitution is `name <- name`.
        let sub = sub.unwrap_or_else(|| ast::Expr {
            kind: Box::new(ast::ExprKind::OpAppl { prefix: Vec::new(), name: ast::Ident { name: name.into(), span }, args: Vec::new() }),
            span,
        });
        if arity == 0 {
            if !args.is_empty() {
                return self.fail(span, format!("`{name}` applied to arguments"));
            }
            let r = self.expr(&mut senv, &sub);
            if senv.used_capture.get() {
                self.ctxs[ctx].uses_capture = true;
            }
            return r;
        }
        // Operator declaration: the substitution must denote an operator.
        senv.in_arg = true;
        let op = self.expr(&mut senv, &sub);
        if senv.used_capture.get() {
            self.ctxs[ctx].uses_capture = true;
        }
        let op = op?;
        match (*op.kind).clone() {
            ExprKind::OpArg(r) => {
                if args.is_empty() {
                    return Ok(Expr::new(ExprKind::OpArg(r), span));
                }
                match r {
                    OpRef::Builtin(b) => self.apply_builtin(b, args, span),
                    _ => Ok(Expr::new(ExprKind::Apply { op: r, args }, span)),
                }
            }
            ExprKind::Lambda { params, mut body } => {
                if args.is_empty() {
                    return Ok(Expr::new(ExprKind::Lambda { params, body }, span));
                }
                if args.len() != params.len() {
                    return self.fail(span, format!("`{name}` expects {} argument(s), got {}", params.len(), args.len()));
                }
                // Beta-reduce.
                let map = params.into_iter().zip(args).collect();
                crate::visit::subst_locals(&mut body, &map);
                Ok(body)
            }
            _ => self.fail(span, format!("substitution for operator `{name}` is not an operator")),
        }
    }

    /// Resolve `I1(a1)!...!In(an)!name(args)` (plus selectors `path`).
    #[allow(clippy::too_many_arguments)] // the argument set of the hot-path/dispatch signature; a bundling struct would only rename them
    fn qualified_appl(
        &mut self,
        env: &mut Env,
        prefix: &[ast::NamePrefix],
        name: &ast::Ident,
        args: &[ast::Expr],
        path: &[ast::Selector],
        op_arg: bool,
        span: Span,
    ) -> Result<Expr, ()> {
        // Walk the instance path, collecting instance arguments.
        let mut inst_args = local_refs(&env.inst_locals, span);
        let (mut ctx, mut module) = (env.ctx, env.module);
        for (pi, p) in prefix.iter().enumerate() {
            // Only the first component can be a LET-bound name.
            let local = if pi == 0 { env.local(&p.name.name).cloned() } else { None };
            let found = match local {
                Some(LocalBinding::Instance(iid)) => Ok(iid),
                Some(LocalBinding::LetDef(d)) => Err(Some(d)),
                Some(_) => Err(None),
                None => {
                    let sc = self.scope(ctx, module);
                    match self.scopes[sc].names.get(&p.name.name).copied() {
                        Some(Sym::Instance(iid)) => Ok(iid),
                        Some(Sym::Def(d)) => Err(Some(d)),
                        _ => Err(None),
                    }
                }
            };
            let iid = match found {
                Ok(iid) => iid,
                Err(Some(d)) => {
                    // `D(a)!lab1!lab2...`: the remaining names are labels in D.
                    let last = ast::NamePrefix { name: name.clone(), args: args.to_vec() };
                    let mut sels: Vec<ast::Selector> = Vec::new();
                    for q in prefix[pi + 1..].iter().chain(std::iter::once(&last)) {
                        sels.push(ast::Selector::Label(q.name.clone(), q.args.clone()));
                    }
                    sels.extend(path.iter().cloned());
                    let a = self.arg_exprs(env, &p.args)?;
                    let d = if pi > 0 { self.qualified_def(d) } else { d };
                    return self.apply_qualified_def(env, d, inst_args, a, &sels, op_arg, span);
                }
                Err(None) => return self.fail(p.name.span, format!("`{}` is not a module instance", p.name.name)),
            };
            let nparams = self.insts[iid.0].ast.params.len();
            if p.args.len() != nparams {
                return self.fail(p.name.span, format!("instance `{}` expects {} argument(s)", p.name.name, nparams));
            }
            let a = self.arg_exprs(env, &p.args)?;
            let Some((c, m)) = self.named_instance(iid) else { return Err(()) };
            // Arguments for the new context: those of the context the instance was declared in, plus `a`.
            let keep = self.ctxs[self.insts[iid.0].ctx].inst_params.len();
            inst_args.truncate(keep);
            inst_args.extend(a);
            if c == ROOT_CTX {
                inst_args.clear();
            }
            ctx = c;
            module = m;
        }
        let args = self.arg_exprs(env, args)?;
        let sc = self.scope(ctx, module);
        let Some(sym) = self.scopes[sc].names.get(&name.name).copied() else {
            return self.fail(name.span, format!("unknown identifier `{}` in instance", name.name));
        };
        if !path.is_empty() && !matches!(sym, Sym::Def(_)) {
            return self.fail(name.span, format!("`{}` is not a definition (in a subexpression reference)", name.name));
        }
        match sym {
            Sym::Def(d) => {
                let d = self.qualified_def(d);
                self.apply_qualified_def(env, d, inst_args, args, path, op_arg, span)
            }
            Sym::Decl { .. } => {
                // `I!x`: INSTANCE does not import the declarations (SANY).
                let mut full: Vec<&str> = prefix.iter().map(|p| &*p.name.name).collect();
                full.push(&name.name);
                self.fail(span, format!("Unknown operator: `{}'.", full.join("!")))
            }
            other => self.apply_sym(env, other, &name.name, args, span),
        }
    }
}

/// Application of an operator-valued callee; an operator argument if `args`
/// is empty.
fn apply_op(op: OpRef, args: Vec<Expr>, span: Span) -> Expr {
    if args.is_empty() {
        Expr::new(ExprKind::OpArg(op), span)
    } else {
        Expr::new(ExprKind::Apply { op, args }, span)
    }
}
