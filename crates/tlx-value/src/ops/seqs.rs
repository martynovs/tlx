//! Sequence operators (`Sequences` module); `Len`, `\o`, `Tail` and `SubSeq`
//! also accept strings, like TLC.

use std::sync::Arc;

use super::lazy;
use crate::error::{err, err_of, ValueErrorKind};
use crate::lazy::LazySet;
use crate::repr::force_fcn;
use crate::value::Value;
use crate::Result;

/// The elements of `s` if it is a sequence (TLC `toTuple`).
fn as_seq<'a>(s: &'a Value, slot: &'a mut Option<Value>) -> Result<Option<&'a [Value]>> {
    match force_fcn(s, slot)? {
        Value::Tuple(t) => Ok(Some(t)),
        _ => Ok(None),
    }
}

fn seq_or<'a>(s: &'a Value, slot: &'a mut Option<Value>, msg: impl FnOnce() -> String) -> Result<&'a [Value]> {
    match as_seq(s, slot)? {
        Some(t) => Ok(t),
        None => err(msg()),
    }
}

/// `Len(s)` (also the length of a string)
pub fn len(s: &Value) -> Result<i64> {
    if let Value::Str(x) = s {
        return Ok(x.as_str().chars().count() as i64);
    }
    let mut slot = None;
    let t = seq_or(s, &mut slot, || format!("The argument of Len should be a sequence, but instead it is:\n{s}"))?;
    Ok(t.len() as i64)
}

/// `Append(s, e)`
pub fn append(s: &Value, e: Value) -> Result<Value> {
    let mut slot = None;
    let t = seq_or(s, &mut slot, || format!("Evaluating an expression of the form Append(s, v) when s is not a sequence:\n{s}"))?;
    let mut elems = Vec::with_capacity(t.len() + 1);
    elems.extend_from_slice(t);
    elems.push(e);
    Ok(Value::Tuple(Arc::from(elems)))
}

/// `s \o t` (sequences or strings)
pub fn concat(s: &Value, t: &Value) -> Result<Value> {
    if let Value::Str(a) = s {
        let Value::Str(b) = t else {
            return err(format!("Evaluating an expression of the form t \\o s when s is not a string:\n{t}"));
        };
        let mut joined = String::with_capacity(a.as_str().len() + b.as_str().len());
        joined.push_str(a.as_str());
        joined.push_str(b.as_str());
        return Ok(Value::str(&joined));
    }
    let (mut s1, mut s2) = (None, None);
    let a = seq_or(s, &mut s1, || format!("Evaluating an expression of the form s \\o t when s is not a sequence:\n{s}"))?;
    let b = seq_or(t, &mut s2, || format!("Evaluating an expression of the form t \\o s when s is not a sequence:\n{t}"))?;
    if a.is_empty() {
        return Ok(Value::Tuple(Arc::from(b)));
    }
    if b.is_empty() {
        return Ok(Value::Tuple(Arc::from(a)));
    }
    let mut elems = Vec::with_capacity(a.len() + b.len());
    elems.extend_from_slice(a);
    elems.extend_from_slice(b);
    Ok(Value::Tuple(Arc::from(elems)))
}

/// `Head(s)`
pub fn head(s: &Value) -> Result<Value> {
    let mut slot = None;
    let t = seq_or(s, &mut slot, || format!("The argument of Head should be a sequence, but instead it is:\n{s}"))?;
    match t.first() {
        Some(e) => Ok(e.clone()),
        None => err_of(ValueErrorKind::ApplyEmptySeq, "Attempted to apply Head to the empty sequence."),
    }
}

/// `Tail(s)` (also of a string)
pub fn tail(s: &Value) -> Result<Value> {
    if let Value::Str(x) = s {
        let mut chars = x.as_str().chars();
        if chars.next().is_none() {
            return err_of(ValueErrorKind::ApplyEmptySeq, "Attempted to apply Tail to the empty sequence.");
        }
        return Ok(Value::str(chars.as_str()));
    }
    let mut slot = None;
    let t = seq_or(s, &mut slot, || format!("The argument of Tail should be a sequence, but instead it is:\n{s}"))?;
    if t.is_empty() {
        return err_of(ValueErrorKind::ApplyEmptySeq, "Attempted to apply Tail to the empty sequence.");
    }
    Ok(Value::Tuple(Arc::from(&t[1..])))
}

/// `SubSeq(s, m, n)`
pub fn subseq(s: &Value, m: i64, n: i64) -> Result<Value> {
    let mut slot = None;
    let (str_chars, seq): (Option<Vec<char>>, Option<&[Value]>) = match s {
        Value::Str(x) => (Some(x.as_str().chars().collect()), None),
        _ => (None, Some(seq_or(s, &mut slot, || format!("The first argument of SubSeq should be a sequence, but instead it is:\n{s}"))?)),
    };
    if m > n {
        return Ok(if str_chars.is_some() { Value::str("") } else { Value::tuple(Vec::new()) });
    }
    let len = match (&str_chars, seq) {
        (Some(c), _) => c.len(),
        (_, Some(t)) => t.len(),
        _ => unreachable!(),
    } as i64;
    if m < 1 || m > len {
        return err(format!("The second argument of SubSeq must be in the domain of its first argument:\n{s}\n, but instead it is\n{m}"));
    }
    if n < 1 || n > len {
        return err(format!("The third argument of SubSeq must be in the domain of its first argument:\n{s}\n, but instead it is\n{n}"));
    }
    let (b, e) = (m as usize - 1, n as usize);
    Ok(match (str_chars, seq) {
        (Some(c), _) => Value::str(&c[b..e].iter().collect::<String>()),
        (_, Some(t)) => Value::Tuple(Arc::from(&t[b..e])),
        _ => unreachable!(),
    })
}

/// `Seq(s)` (lazy)
pub fn seq_set(s: &Value) -> Result<Value> {
    Ok(lazy(LazySet::Seq(s.clone())))
}
