//! Set membership `v \in S` for every set representation.

use std::cmp::Ordering;

use super::{domain, for_each_element, is_enumerable};
use crate::error::err;
use crate::lazy::LazySet;
use crate::order::{cmp_tlc, eq_tlc, mv_member};
use crate::plan::Direction;
use crate::repr::{force_fcn, FcnRef};
use crate::value::Value;
use crate::Result;

/// `v \in s`
pub fn member(v: &Value, s: &Value) -> Result<bool> {
    match s {
        Value::Set(elems) => {
            // (An untyped model value compares with a model value by symbol
            // and sorts before the other values, without an error: `mv_cmp`.)
            if let Value::ModelValue(m) = v {
                if m.model_value_type().is_none() {
                    return Ok(elems
                        .binary_search_by(|e| match e {
                            Value::ModelValue(n) => n.cmp(m),
                            _ => Ordering::Greater,
                        })
                        .is_ok());
                }
            }
            let (mut lo, mut hi) = (0usize, elems.len());
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let (a, b) = Direction::Probe.operands(&elems[mid], v);
                match cmp_tlc(a, b)? {
                    Ordering::Equal => return Ok(true),
                    Ordering::Less => hi = mid,
                    Ordering::Greater => lo = mid + 1,
                }
            }
            Ok(false)
        }
        Value::Interval(lo, hi) => match v {
            Value::Int(x) => Ok(lo <= x && x <= hi),
            Value::ModelValue(m) if m.model_value_type().is_none() => Ok(false),
            _ if lo <= hi => err(format!("Attempted to check if the value:\n{v}\nis in the integer interval {s}")),
            _ => Ok(false),
        },
        Value::LazySet(l) => member_lazy(v, s, l),
        Value::Bool(_) => err(format!("Attempted to check if the value:\n{v}\nis an element of the boolean {s}")),
        Value::Int(_) => err(format!("Attempted to check if the value:\n{v}\nis an element of the integer {s}")),
        Value::Str(_) => err(format!("Attempted to check if the value:\n{v}\nis an element of the string {s}")),
        Value::ModelValue(_) => err(format!("Attempted to check if the value:\n{v}\nis an element of the model value {s}")),
        Value::Tuple(_) => err("Attempted to check set membership in a tuple value."),
        Value::Record(_) => err(format!("Attempted to check if element:\n{v}\nis in the record:\n{s}")),
        Value::Fcn(..) | Value::LazyFcn(_) => err(format!("Attempted to check if the value:\n{v}\nis an element of the function {s}")),
    }
}

fn member_lazy(v: &Value, s: &Value, l: &LazySet) -> Result<bool> {
    let builtin_member = |ok: Option<bool>, name: &str| -> Result<bool> {
        match (ok, v) {
            (Some(b), _) => Ok(b),
            (None, Value::ModelValue(m)) => mv_member(*m, v, s),
            (None, _) => err(format!("Attempted to check if the value:\n{v}\nis an element of {name}.")),
        }
    };
    match l {
        LazySet::Enum(es) => {
            for e in es {
                if eq_tlc(e, v)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        LazySet::Subset(base) => {
            if !is_enumerable(v) {
                return err(format!("Attempted to check if the non-enumerable value\n{v}\nis element of\n{s}"));
            }
            let mut res = true;
            for_each_element(v, &mut |e| {
                if member(&e, base)? {
                    Ok(true)
                } else {
                    res = false;
                    Ok(false)
                }
            })?;
            Ok(res)
        }
        LazySet::FcnSet(dom, range) => {
            let mut slot = None;
            let fv = match v {
                Value::Tuple(_) | Value::Record(_) | Value::Fcn(..) | Value::LazyFcn(_) => force_fcn(v, &mut slot)?,
                Value::ModelValue(m) => return mv_member(*m, v, s),
                _ => {
                    return err(format!("Attempted to check if \n{v}\nwhich is not a TLC function value, is in the set of functions:\n{s}"))
                }
            };
            let f = FcnRef::of(fv).unwrap();
            let dom_ok = match f {
                FcnRef::Tuple(t) => eq_tlc(&Value::Interval(1, t.len() as i64), dom)?,
                _ => eq_tlc(dom, &domain(fv)?)?,
            };
            if !dom_ok {
                return Ok(false);
            }
            for i in 0..f.len() {
                if !member(f.val(i), range)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        LazySet::Product(sets) => {
            let mut slot = None;
            let fv = force_fcn(v, &mut slot)?;
            match fv {
                Value::Tuple(t) => {
                    if t.len() != sets.len() {
                        return Ok(false);
                    }
                    for (e, s) in t.iter().zip(sets.iter()) {
                        if !member(e, s)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                Value::Fcn(p, _) if p.iter().all(|(k, _)| matches!(k, Value::Int(_))) => Ok(false),
                Value::ModelValue(m) => mv_member(*m, v, s),
                _ => err(format!("Attempted to check if non-tuple\n{v}\nis in the set of tuples:\n{s}")),
            }
        }
        LazySet::RecordSet(fields) => {
            let mut slot = None;
            let fv = force_fcn(v, &mut slot)?;
            match fv {
                Value::Record(r) => {
                    if r.len() != fields.len() {
                        return Ok(false);
                    }
                    for ((n1, e), (n2, s)) in r.iter().zip(fields.iter()) {
                        if n1 != n2 || !member(e, s)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                Value::Tuple(t) if t.is_empty() => Ok(fields.is_empty()),
                Value::ModelValue(m) => mv_member(*m, v, s),
                _ => err(format!("Attempted to check if non-record\n{v}\nis in the set of records:\n{s}")),
            }
        }
        LazySet::Seq(range) => {
            let mut slot = None;
            match force_fcn(v, &mut slot)? {
                Value::Tuple(t) => {
                    for e in t.iter() {
                        if !member(e, range)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                Value::ModelValue(m) => mv_member(*m, v, s),
                _ => err(format!("Attempted to check if the value:\n{v}\nis an element of {s}.")),
            }
        }
        LazySet::Union(sets) => {
            if !is_enumerable(sets) {
                return err(format!("Attempted to check if:\n {v}\nis an element of the non-enumerable set:\n {s}"));
            }
            let mut found = false;
            for_each_element(sets, &mut |e| {
                if member(v, &e)? {
                    found = true;
                    Ok(false)
                } else {
                    Ok(true)
                }
            })?;
            Ok(found)
        }
        LazySet::Cup(a, b) => Ok(member(v, a)? || member(v, b)?),
        LazySet::Cap(a, b) => Ok(member(v, a)? && member(v, b)?),
        LazySet::Diff(a, b) => Ok(member(v, a)? && !member(v, b)?),
        LazySet::Nat => builtin_member(if let Value::Int(n) = v { Some(*n >= 0) } else { None }, "Nat"),
        LazySet::Int => builtin_member(matches!(v, Value::Int(_)).then_some(true), "Int"),
        LazySet::StringSet => builtin_member(matches!(v, Value::Str(_)).then_some(true), "STRING"),
        // TLC's AnySet.member always returns true.
        LazySet::Any => Ok(true),
        LazySet::KSubset(k, base) => super::ksubset::ksubset_member(v, *k, base),
        LazySet::Custom(c) => c.contains(v),
    }
}
