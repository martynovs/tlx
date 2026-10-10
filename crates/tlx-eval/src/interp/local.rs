//! Per-thread copies of constant values.
//!
//! Constants (CONSTANT values, cfg-assigned definitions and cached
//! constant-level definitions) are read all the time by every worker. Cloning
//! a shared compound value touches its reference count, and with several
//! workers that cache line bounces between cores. Each thread therefore
//! reads constants from its own deep copy (separate reference counts).

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tlx_sema::hir::{DefKind, ExceptKey, Expr, ExprKind};
use tlx_value::{Sym, Value};

/// Source of unique interpreter ids (keys of the per-thread tables; unlike
/// addresses they are never reused).
static NEXT_UID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn fresh_uid() -> u64 {
    NEXT_UID.fetch_add(1, Ordering::Relaxed)
}

/// Which table of an interpreter a constant lives in.
#[derive(Clone, Copy)]
pub(crate) enum Slot {
    /// `ConstId` index.
    Const(usize),
    /// `DefId` index (cfg-assigned or cached constant-level definition).
    Def(usize),
    /// Enumerated closed constant domain of a bound variable
    /// (`Interp::bound_elems`).
    Bound(usize),
    /// Value of a closed constant expression (`code::Node::Hoist`,
    /// `code::Node::Shared`).
    Hoist(usize),
}

#[derive(Default)]
struct Tables {
    consts: Vec<Option<Value>>,
    defs: Vec<Option<Value>>,
    bounds: Vec<Option<Value>>,
    hoisted: Vec<Option<Value>>,
}

impl Tables {
    fn slot(&mut self, s: Slot) -> &mut Option<Value> {
        let (v, i) = match s {
            Slot::Const(i) => (&mut self.consts, i),
            Slot::Def(i) => (&mut self.defs, i),
            Slot::Bound(i) => (&mut self.bounds, i),
            Slot::Hoist(i) => (&mut self.hoisted, i),
        };
        if v.len() <= i {
            v.resize(i + 1, None);
        }
        &mut v[i]
    }
}

/// Interpreters with tables on this thread (few: usually one).
const MAX_INTERPS: usize = 4;

thread_local! {
    static LOCAL: RefCell<Vec<(u64, Tables)>> = const { RefCell::new(Vec::new()) };
}

/// This thread's copy of the constant in `slot` of interpreter `uid`;
/// `shared` gives the shared value on first use (errors are not kept).
#[inline]
pub(crate) fn get<E>(uid: u64, slot: Slot, shared: impl FnOnce() -> Result<Value, E>) -> Result<Value, E> {
    if let Some(v) = peek(uid, slot) {
        return Ok(v);
    }
    // Not under the borrow: computing the shared value may read constants.
    let v = shared()?;
    let mut budget = COPY_BUDGET;
    let copy = deep_copy(&v, &mut budget);
    put(uid, slot, copy.clone());
    Ok(copy)
}

/// This thread's value in `slot` of interpreter `uid`, if set.
#[inline]
pub(crate) fn peek(uid: u64, slot: Slot) -> Option<Value> {
    LOCAL.with(|l| {
        let mut l = l.borrow_mut();
        let t = l.iter_mut().find(|(u, _)| *u == uid)?;
        t.1.slot(slot).clone()
    })
}

/// Set this thread's value in `slot` of interpreter `uid` (computed by this
/// thread: not copied).
pub(crate) fn put(uid: u64, slot: Slot, v: Value) {
    LOCAL.with(|l| {
        let mut l = l.borrow_mut();
        let i = match l.iter().position(|(u, _)| *u == uid) {
            Some(i) => i,
            None => {
                if l.len() == MAX_INTERPS {
                    l.remove(0);
                }
                l.push((uid, Tables::default()));
                l.len() - 1
            }
        };
        *l[i].1.slot(slot) = Some(v);
    });
}

/// A map keyed by addresses.
pub(crate) type AddrMap<T> = std::collections::HashMap<usize, T, tlx_base::hash::IntegerBuildHasher>;

/// The symbols of the string literals, field names and record fields of a
/// program, keyed by the address of their text (interning the text again
/// on every evaluation hashes and compares it).
#[derive(Default)]
pub(crate) struct SymTable(AddrMap<Sym>);

impl SymTable {
    pub(crate) fn new(prog: &tlx_sema::Program) -> SymTable {
        let mut t = SymTable::default();
        for def in &prog.defs {
            match &def.kind {
                DefKind::Op(e) => t.add_expr(e),
                DefKind::Fn { bounds, body } => {
                    for b in bounds {
                        t.add_expr(&b.set);
                    }
                    t.add_expr(body);
                }
                DefKind::Pending => {}
            }
        }
        for e in &prog.assumes {
            t.add_expr(e);
        }
        t
    }

    fn add(&mut self, s: &Arc<str>) {
        self.0.insert(key(s), Sym::new(s));
    }

    fn add_expr(&mut self, e: &Expr) {
        match &*e.kind {
            ExprKind::Str(s) | ExprKind::Field { field: s, .. } => self.add(s),
            ExprKind::Record(fs) | ExprKind::RecordSet(fs) => {
                for (n, _) in fs {
                    self.add(n);
                }
            }
            ExprKind::Except { updates, .. } => {
                for (keys, _) in updates {
                    for k in keys {
                        if let ExceptKey::Field(f) = k {
                            self.add(f);
                        }
                    }
                }
            }
            _ => {}
        }
        tlx_sema::visit::children(e, &mut |c| self.add_expr(c));
    }

    /// The symbol of `s`.
    #[inline]
    pub(crate) fn get(&self, s: &Arc<str>) -> Sym {
        match self.0.get(&key(s)) {
            Some(&sym) => sym,
            None => Sym::new(s),
        }
    }
}

fn key(s: &Arc<str>) -> usize {
    Arc::as_ptr(s) as *const u8 as usize
}

/// Nodes copied per constant at most (larger constants share the rest).
const COPY_BUDGET: usize = 1 << 16;

/// A copy of `v` with fresh reference counts (up to `budget` nodes; lazy
/// values stay shared).
fn deep_copy(v: &Value, budget: &mut usize) -> Value {
    if *budget == 0 {
        return v.clone();
    }
    *budget -= 1;
    match v {
        Value::Set(es) => Value::Set(es.iter().map(|e| deep_copy(e, budget)).collect::<Arc<[Value]>>()),
        Value::Tuple(es) => Value::Tuple(es.iter().map(|e| deep_copy(e, budget)).collect::<Arc<[Value]>>()),
        Value::Record(fs) => Value::Record(fs.iter().map(|(k, e)| (*k, deep_copy(e, budget))).collect::<Arc<[_]>>()),
        Value::Fcn(ps, d) => Value::Fcn(ps.iter().map(|(k, e)| (deep_copy(k, budget), deep_copy(e, budget))).collect::<Arc<[_]>>(), *d),
        _ => v.clone(),
    }
}
