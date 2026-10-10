//! Operator application: resolving an operator reference to the
//! definition or builtin to analyze, memoizing definition applications
//! and analyzing their bodies (recursion to a fixpoint).

use tlx_sema::hir::*;

use super::super::shape::{self, OpVal, Sh, ALL};
use super::{Analyzer, Arg, AtomKey, Frame, DEPTH, MAX_FRAMES};
use crate::interp::ConstVal;

impl<'a> Analyzer<'a> {
    pub(super) fn apply(&mut self, op: &OpRef, args: &[Expr], span: Span) -> Sh {
        let args: Vec<Arg> = args.iter().map(|a| Arg { sh: self.expr(a), var: self.target(a) }).collect();
        self.apply_ref(op, args, span)
    }

    pub(super) fn apply_ref(&mut self, op: &OpRef, args: Vec<Arg>, span: Span) -> Sh {
        let ip = self.ip;
        match op {
            OpRef::Def(d) => self.apply_def(*d, args, span),
            OpRef::Builtin(b) => match ip.builtin_override(*b) {
                Some(d) => self.apply_def(d, args, span),
                None => self.builtin(*b, &args.into_iter().map(|a| a.sh).collect::<Vec<_>>(), span),
            },
            OpRef::Param(l) => {
                let ops = self.local(*l).map(|a| a.sh.ops()).unwrap_or_default();
                if ops.is_empty() {
                    return Sh::any(ALL);
                }
                let mut r = Sh::default();
                for o in ops {
                    r = r.join(&self.apply_op(&o, args.clone(), span));
                }
                r
            }
            OpRef::Const(c) => match &ip.consts[c.idx()] {
                Some(ConstVal::Op(r)) => self.apply_ref(r, args, span),
                Some(ConstVal::Def(d)) => self.apply_def(*d, args, span),
                Some(ConstVal::Table(rows)) => self.table(*c, rows, span),
                Some(ConstVal::Value(v)) => {
                    let name = ip.prog.consts[c.idx()].name.to_string();
                    self.atom(AtomKey::Const(*c), v, &name, span)
                }
                None => Sh::any(ALL),
            },
        }
    }

    pub(super) fn apply_op(&mut self, o: &OpVal, args: Vec<Arg>, span: Span) -> Sh {
        match o {
            OpVal::Def(d) => self.apply_def(*d, args, span),
            OpVal::Builtin(b) => self.builtin(*b, &args.into_iter().map(|a| a.sh).collect::<Vec<_>>(), span),
            OpVal::Lambda(e) => {
                let Some((params, body)) = shape::lambda_parts(e) else { return Sh::any(ALL) };
                let mark = self.locals.len();
                for (p, a) in params.iter().zip(args) {
                    self.locals.push((*p, a));
                }
                let r = self.expr(body);
                self.locals.truncate(mark);
                r
            }
        }
    }

    pub(super) fn apply_def(&mut self, d: DefId, args: Vec<Arg>, span: Span) -> Sh {
        let ip = self.ip;
        let d = ip.redirect_def(d);
        let def = &ip.prog.defs[d.idx()];
        if let Some(v) = ip.def_value.get(&d) {
            return self.atom(AtomKey::Def(d), v, &def.name, span);
        }
        if let Some(b) = def.builtin {
            return self.builtin(b, &args.into_iter().map(|a| a.sh).collect::<Vec<_>>(), span);
        }
        if args.is_empty() && def.params.is_empty() && ip.is_const_def(d) {
            if let Some(sh) = self.const_def(d) {
                return sh;
            }
        }
        self.call(d, args)
    }

    /// Analyze the body of `d` applied to `args` (memoized; a recursive
    /// application is analyzed to a fixpoint).
    fn call(&mut self, d: DefId, args: Vec<Arg>) -> Sh {
        let ip = self.ip;
        // LET definitions see the locals of their context, and so do LAMBDAs
        // and LET operators passed as arguments: such applications are not
        // memoized.
        let closure = |o: &OpVal| match o {
            OpVal::Lambda(_) => true,
            OpVal::Def(x) => ip.prog.def_is_let(*x),
            OpVal::Builtin(_) => false,
        };
        let local = ip.prog.def_is_let(d) || args.iter().any(|a| a.sh.ops().iter().any(closure));
        let key = (d, args);
        if !local {
            if let Some(r) = self.memo.get(&key) {
                return r.clone();
            }
        }
        if let Some(i) = self.stack.iter().position(|f| f.key == key) {
            self.stack[i].used = true;
            self.used_below = self.used_below.min(i);
            return self.stack[i].approx.clone();
        }
        if self.stack.len() >= MAX_FRAMES {
            self.overflow = true;
            return Sh::any(ALL);
        }
        let j = self.stack.len();
        self.stack.push(Frame { key: key.clone(), approx: Sh::default(), used: false });
        let saved = std::mem::replace(&mut self.used_below, usize::MAX);
        let mut round = 0;
        let result = loop {
            self.stack[j].used = false;
            let r = self.body(d, &key.1);
            let f = &mut self.stack[j];
            if !f.used {
                break r;
            }
            if r.le(&f.approx) {
                break f.approx.clone();
            }
            round += 1;
            f.approx = if round > 8 { Sh::any(f.approx.join(&r).mask()) } else { f.approx.join(&r).limit(DEPTH) };
            if self.overflow {
                break Sh::any(ALL);
            }
        };
        self.stack.pop();
        let below = self.used_below;
        if below >= j && !local {
            self.memo.insert(key, result.clone());
        }
        self.used_below = saved.min(if below < j { below } else { usize::MAX });
        result
    }

    fn body(&mut self, d: DefId, args: &[Arg]) -> Sh {
        let ip = self.ip;
        let def = &ip.prog.defs[d.idx()];
        let mark = self.locals.len();
        for (p, a) in def.params.iter().zip(args) {
            self.locals.push((*p, a.clone()));
        }
        for p in def.params.iter().skip(args.len()) {
            self.locals.push((*p, Arg::of(Sh::any(ALL))));
        }
        let r = match &def.kind {
            DefKind::Op(body) => self.expr(body),
            DefKind::Fn { bounds, body } => {
                let dom = self.bind_domain(bounds);
                let r = self.expr(body);
                Sh::fcn(dom, r)
            }
            DefKind::Pending => {
                self.breaker(ALL, format!("the unresolved definition {}", def.name), def.span);
                Sh::any(ALL)
            }
        };
        self.locals.truncate(mark);
        r
    }
}
