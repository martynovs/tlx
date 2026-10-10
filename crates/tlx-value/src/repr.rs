//! Internal views over the different set / function representations.

use std::borrow::Cow;

use crate::error::err;
use crate::intern::Sym;
use crate::lazy::LazySet;
use crate::value::{lazy_fcn_to_value, Value};
use crate::Result;

/// The elements of a finite set in normalized (TLC) order.
pub(crate) enum Elems<'a> {
    Slice(&'a [Value]),
    Owned(Vec<Value>),
    /// `lo, len`
    Range(i64, usize),
}

impl Elems<'_> {
    pub(crate) fn len(&self) -> usize {
        match self {
            Elems::Slice(s) => s.len(),
            Elems::Owned(v) => v.len(),
            Elems::Range(_, n) => *n,
        }
    }

    pub(crate) fn get(&self, i: usize) -> Cow<'_, Value> {
        match self {
            Elems::Slice(s) => Cow::Borrowed(&s[i]),
            Elems::Owned(v) => Cow::Borrowed(&v[i]),
            Elems::Range(lo, _) => Cow::Owned(Value::Int(lo + i as i64)),
        }
    }

    pub(crate) fn into_vec(self) -> Vec<Value> {
        match self {
            Elems::Slice(s) => s.to_vec(),
            Elems::Owned(v) => v,
            Elems::Range(lo, n) => (0..n as i64).map(|i| Value::Int(lo + i)).collect(),
        }
    }
}

/// Number of elements of `lo..hi`, with TLC's 32-bit size check.
pub(crate) fn interval_len(lo: i64, hi: i64) -> Result<usize> {
    if hi < lo {
        return Ok(0);
    }
    let n = hi as i128 - lo as i128 + 1;
    if n > i32::MAX as i128 {
        return err(format!("Size of interval value exceeds the maximum representable size (32bits): {lo}..{hi}."));
    }
    Ok(n as usize)
}

/// Normalized elements of a set value. `Ok(None)` if `v` is not a set.
/// Errors if the set is infinite / cannot be enumerated.
pub(crate) fn set_elems(v: &Value) -> Result<Option<Elems<'_>>> {
    Ok(Some(match v {
        Value::Set(s) => Elems::Slice(s),
        Value::Interval(lo, hi) => Elems::Range(*lo, interval_len(*lo, *hi)?),
        Value::LazySet(_) => Elems::Owned(crate::ops::enumerate_normalized(v)?),
        _ => return Ok(None),
    }))
}

/// Infinite built-in sets (TLC `UserValue`s): never enumerable.
#[derive(Clone, Copy)]
pub(crate) enum Builtin<'a> {
    Nat,
    Int,
    Str,
    Seq(&'a Value),
}

impl Builtin<'_> {
    pub(crate) fn rank(self) -> u8 {
        match self {
            Builtin::Nat => 0,
            Builtin::Int => 1,
            Builtin::Str => 3,
            Builtin::Seq(_) => 4,
        }
    }
}

pub(crate) fn builtin(v: &Value) -> Option<Builtin<'_>> {
    match v {
        Value::LazySet(l) => match &**l {
            LazySet::Nat => Some(Builtin::Nat),
            LazySet::Int => Some(Builtin::Int),
            LazySet::StringSet => Some(Builtin::Str),
            LazySet::Seq(s) => Some(Builtin::Seq(s)),
            _ => None,
        },
        _ => None,
    }
}

/// A function value in canonical representation.
#[derive(Clone, Copy)]
pub(crate) enum FcnRef<'a> {
    Tuple(&'a [Value]),
    Record(&'a [(Sym, Value)]),
    Fcn(&'a [(Value, Value)]),
}

impl<'a> FcnRef<'a> {
    pub(crate) fn of(v: &'a Value) -> Option<FcnRef<'a>> {
        match v {
            Value::Tuple(t) => Some(FcnRef::Tuple(t)),
            Value::Record(r) => Some(FcnRef::Record(r)),
            Value::Fcn(f, _) => Some(FcnRef::Fcn(f)),
            _ => None,
        }
    }

    pub(crate) fn len(self) -> usize {
        match self {
            FcnRef::Tuple(t) => t.len(),
            FcnRef::Record(r) => r.len(),
            FcnRef::Fcn(f) => f.len(),
        }
    }

    pub(crate) fn key(self, i: usize) -> Cow<'a, Value> {
        match self {
            FcnRef::Tuple(_) => Cow::Owned(Value::Int(i as i64 + 1)),
            FcnRef::Record(r) => Cow::Owned(Value::Str(r[i].0)),
            FcnRef::Fcn(f) => Cow::Borrowed(&f[i].0),
        }
    }

    pub(crate) fn val(self, i: usize) -> &'a Value {
        match self {
            FcnRef::Tuple(t) => &t[i],
            FcnRef::Record(r) => &r[i].1,
            FcnRef::Fcn(f) => &f[i].1,
        }
    }
}

/// If `v` is a lazy function, materialize it into `slot` and return the
/// materialized value; otherwise return `v`. A function built over an
/// enumerated domain that is `1..n` or a set of strings is returned in its
/// `Tuple` / `Record` form (TLC converts it with `toTuple` / `toRcd`
/// wherever a sequence, a record or a function is compared or read).
pub(crate) fn force_fcn<'a>(v: &'a Value, slot: &'a mut Option<Value>) -> Result<&'a Value> {
    match v {
        Value::LazyFcn(f) => {
            let x = lazy_fcn_to_value(&**f)?;
            *slot = Some(x.fcn_canonical().unwrap_or(x));
            Ok(slot.as_ref().unwrap())
        }
        Value::Fcn(..) => match v.fcn_canonical() {
            Some(x) => {
                *slot = Some(x);
                Ok(slot.as_ref().unwrap())
            }
            None => Ok(v),
        },
        _ => Ok(v),
    }
}
