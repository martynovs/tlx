//! Memoization of constant-level operator applications in constant
//! contexts (ASSUME, constant definitions, the cfg): a pure operator's value
//! depends only on its arguments, and constant-level tests often apply the
//! same operator to the same arguments many times (MCReachabilityTest:
//! `ReachableFrom(T)` for every pair `S, T \in SUBSET Nodes`). TLC does not
//! memoize; the values are the same, so results are unchanged.
//!
//! Per thread, keyed by the interpreter, the definition, KeepLazy and the
//! argument values compared by representation (not TLC equality: `<<>>`
//! and the empty function of an enumerated domain behave differently).
//! Only small, fully built arguments (no lazy values) are keys, and only
//! small values are kept. Errors are not memoized. A definition whose lookups rarely hit is no longer
//! memoized on that thread; the table is cleared when it is full.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};

use tlx_base::hash::FxMap;
use tlx_sema::hir::DefId;
use tlx_value::Value;

/// Most entries per thread (cleared when full).
const MAX_ENTRIES: usize = 1 << 16;
/// Most nodes of the arguments of a memoized application.
const MAX_ARG_NODES: usize = 256;
/// Most nodes of a memoized value (a lazy value counts as one).
pub(crate) const MAX_VALUE_NODES: usize = 4096;
/// After this many lookups of a definition, it is no longer memoized if
/// fewer than 1 in `MIN_HIT_RATIO` hit.
const PROBATION: u32 = 1024;
const MIN_HIT_RATIO: u32 = 8;

struct Key {
    uid: u64,
    def: DefId,
    keep_lazy: bool,
    args: Vec<Value>,
}

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uid.hash(state);
        self.def.hash(state);
        self.keep_lazy.hash(state);
        for a in &self.args {
            hash_repr(a, state);
        }
    }
}

/// A hash of `v` consistent with `same`: by representation, so that an
/// interval costs O(1) (a fingerprint enumerates it, e.g. `1..2147483647`).
fn hash_repr<H: Hasher>(v: &Value, state: &mut H) {
    std::mem::discriminant(v).hash(state);
    match v {
        Value::Bool(b) => b.hash(state),
        Value::Int(i) => i.hash(state),
        Value::Str(s) | Value::ModelValue(s) => s.hash(state),
        Value::Interval(lo, hi) => (lo, hi).hash(state),
        Value::Set(es) | Value::Tuple(es) => {
            es.len().hash(state);
            es.iter().for_each(|e| hash_repr(e, state));
        }
        Value::Record(fs) => fs.iter().for_each(|(k, e)| {
            k.hash(state);
            hash_repr(e, state);
        }),
        Value::Fcn(ps, _) => ps.iter().for_each(|(k, e)| {
            hash_repr(k, state);
            hash_repr(e, state);
        }),
        // (Not keys: see `plain`.)
        Value::LazySet(_) | Value::LazyFcn(_) => {}
    }
}

impl PartialEq for Key {
    fn eq(&self, o: &Key) -> bool {
        self.uid == o.uid
            && self.def == o.def
            && self.keep_lazy == o.keep_lazy
            && self.args.len() == o.args.len()
            && self.args.iter().zip(&o.args).all(|(a, b)| same(a, b))
    }
}

impl Eq for Key {}

/// Equal values with the same representation.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Str(x), Value::Str(y)) | (Value::ModelValue(x), Value::ModelValue(y)) => x == y,
        (Value::Interval(a, b), Value::Interval(c, d)) => a == c && b == d,
        (Value::Set(x), Value::Set(y)) | (Value::Tuple(x), Value::Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| same(p, q))
        }
        (Value::Record(x), Value::Record(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| p.0 == q.0 && same(&p.1, &q.1)),
        (Value::Fcn(x, dx), Value::Fcn(y, dy)) => {
            dx == dy && x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| same(&p.0, &q.0) && same(&p.1, &q.1))
        }
        _ => false,
    }
}

/// Whether `v` is fully built (no lazy value) and small enough to be part
/// of a key; counts its nodes against `budget`.
pub(crate) fn plain(v: &Value, budget: &mut usize) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    match v {
        Value::Bool(_) | Value::Int(_) | Value::Str(_) | Value::ModelValue(_) | Value::Interval(..) => true,
        Value::Set(es) | Value::Tuple(es) => es.iter().all(|e| plain(e, budget)),
        Value::Record(fs) => fs.iter().all(|(_, e)| plain(e, budget)),
        Value::Fcn(ps, _) => ps.iter().all(|(k, e)| plain(k, budget) && plain(e, budget)),
        Value::LazySet(_) | Value::LazyFcn(_) => false,
    }
}

/// Whether `v` has at most `budget` nodes (a lazy value counts as one).
pub(crate) fn small(v: &Value, budget: &mut usize) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    match v {
        Value::Set(es) | Value::Tuple(es) => es.iter().all(|e| small(e, budget)),
        Value::Record(fs) => fs.iter().all(|(_, e)| small(e, budget)),
        Value::Fcn(ps, _) => ps.iter().all(|(k, e)| small(k, budget) && small(e, budget)),
        _ => true,
    }
}

/// Whether the arguments `args` can be a key.
pub(crate) fn keyable(args: &[Value]) -> bool {
    let mut budget = MAX_ARG_NODES;
    args.iter().all(|a| plain(a, &mut budget))
}

#[derive(Default)]
struct Table {
    entries: FxMap<Key, Value>,
    /// Per (interpreter, definition): lookups and hits.
    stats: FxMap<(u64, DefId), (u32, u32)>,
}

thread_local! {
    static TABLE: RefCell<Table> = RefCell::new(Table::default());
}

/// The memoized value of `def` applied to `args`, or `eval()` (recorded
/// when it succeeds, unless `def` is no longer memoized on this thread).
pub(crate) fn get_or_eval(
    uid: u64,
    def: DefId,
    keep_lazy: bool,
    args: Vec<Value>,
    eval: impl FnOnce() -> crate::error::EResult<Value>,
) -> crate::error::EResult<Value> {
    let key = Key { uid, def, keep_lazy, args };
    let hit = TABLE.with(|t| {
        let mut t = t.borrow_mut();
        let (lookups, hits) = *t.stats.get(&(uid, def)).unwrap_or(&(0, 0));
        if lookups >= PROBATION && (hits as u64) * (MIN_HIT_RATIO as u64) < lookups as u64 {
            return Err(());
        }
        let v = t.entries.get(&key).cloned();
        let s = t.stats.entry((uid, def)).or_default();
        s.0 = s.0.saturating_add(1);
        if v.is_some() {
            s.1 = s.1.saturating_add(1);
        }
        Ok(v)
    });
    match hit {
        Ok(Some(v)) => Ok(v),
        // Not under the borrow: the evaluation may apply memoized operators.
        Ok(None) => {
            let v = eval()?;
            let mut budget = MAX_VALUE_NODES;
            if small(&v, &mut budget) {
                TABLE.with(|t| {
                    let mut t = t.borrow_mut();
                    if t.entries.len() >= MAX_ENTRIES {
                        t.entries.clear();
                    }
                    t.entries.insert(key, v.clone());
                });
            }
            Ok(v)
        }
        Err(()) => eval(),
    }
}
