//! Lazily represented sets and functions.

use std::fmt::Debug;

use crate::intern::Sym;
use crate::value::Value;

/// Sets kept symbolic until enumeration is required (tlc-semantics.md §1.5).
/// Construction functions in `ops` decide when to evaluate eagerly (e.g. a
/// `\cup` of two enumerated sets is computed immediately).
#[derive(Debug)]
pub enum LazySet {
    /// An explicit set whose elements may be incomparable. Preserve source
    /// order for membership and reducible set algebra until normalization.
    Enum(Vec<Value>),
    /// `SUBSET S`
    Subset(Value),
    /// `kSubset(k, S)` of FiniteSetsExt (TLC's KSubsetValue): the subsets of
    /// S with k elements (see `ops::ksubset`).
    KSubset(i64, Value),
    /// `[S -> T]`
    FcnSet(Value, Value),
    /// `S1 \X ... \X Sn`
    Product(Vec<Value>),
    /// `[f1 : S1, ...]`, fields sorted by `Sym` order.
    RecordSet(Vec<(Sym, Value)>),
    /// `Seq(S)` (infinite unless S is empty).
    Seq(Value),
    /// `UNION S` (enumeration may yield duplicates, like TLC's UnionValue).
    Union(Value),
    /// `S \cup T` (enumeration: S then T, may yield duplicates, like SetCupValue).
    Cup(Value, Value),
    /// `S \cap T`
    Cap(Value, Value),
    /// `S \ T`
    Diff(Value, Value),
    Nat,
    Int,
    StringSet,
    /// TLC's `Any` (module `TLC`): `v \in Any` is TRUE for every value, but it
    /// is not enumerable (TLC's `AnySet`, an opaque `UserValue`).
    Any,
    /// Evaluator-provided set (e.g. a set filter over an infinite set).
    Custom(Box<dyn CustomSet>),
}

/// A set whose membership / enumeration needs the evaluator.
pub trait CustomSet: Send + Sync + Debug {
    fn contains(&self, v: &Value) -> crate::Result<bool>;
    /// All elements in TLC enumeration order, or `Ok(None)` if infinite.
    fn enumerate(&self) -> crate::Result<Option<Vec<Value>>>;
    /// Visit the elements in the order of `enumerate` without materializing
    /// them all; `f` returns `Ok(false)` to stop. `Ok(None)` if infinite,
    /// else `Ok(Some(false))` iff `f` stopped the enumeration.
    fn for_each(&self, f: &mut dyn FnMut(Value) -> crate::Result<bool>) -> crate::Result<Option<bool>> {
        let Some(elems) = self.enumerate()? else { return Ok(None) };
        for e in elems {
            if !f(e)? {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    }
    /// `IsFiniteSet` of the set (by default: whether it can be enumerated).
    fn is_finite(&self) -> crate::Result<bool> {
        Ok(self.enumerate()?.is_some())
    }
    /// The error of an enumeration of the set when it cannot be enumerated
    /// (`enumerate` gave `None`); by default TLC's error for a quantifier
    /// over a non-enumerable set.
    fn not_enumerable(&self, shown: &Value) -> crate::ValueError {
        crate::ValueError::new(format!("TLC encountered the non-enumerable quantifier bound\n{shown}."))
    }
}

/// A function computed on demand (e.g. `[n \in Nat |-> n * 2]` or a recursive
/// function definition).
pub trait LazyFcn: Send + Sync + Debug {
    fn apply(&self, arg: &Value) -> crate::Result<Value>;
    fn domain(&self) -> crate::Result<Value>;
    /// True for a function over an enumerable domain that is lazy only
    /// because it was constructed where it is applied (TLC's KeepLazy): the
    /// evaluator materializes it when it escapes such a context.
    fn kept_lazy(&self) -> bool {
        false
    }
}
