//! Native Bags operators, following TLC's Java overrides
//! (tlc2/module/Bags.java) rather than the TLA+ definitions where they
//! differ (e.g. `IsABag` of a non-function is FALSE, `BagCardinality` and
//! `(+)` reject non-bags, `CopiesIn` returns the count as is).

use tlx_value::{ops, Value};

use crate::error::{EResult, EvalError};

/// An error TLC raises as an EvalException (TLC_MODULE_ARGUMENT_ERROR,
/// TLC_MODULE_APPLYING_TO_WRONG_VALUE, ...): exit status 255.
fn arg_err(msg: String) -> EvalError {
    EvalError::of_kind(tlx_runtime::ErrorKind::ArgumentError, msg)
}

/// TLC_MODULE_APPLYING_TO_WRONG_VALUE.
fn wrong_value(op: &str, what: &str, v: &Value) -> EvalError {
    arg_err(format!("Applying {op} to the following value,\nwhich is not {what}:\n{v}"))
}

/// The (key, count) pairs of a function with a finite domain, in normalized
/// key order; `None` if `b` is not such a function (TLC: `toFcnRcd` null).
fn pairs(b: &Value) -> EResult<Option<Vec<(Value, Value)>>> {
    let is_fcn = matches!(b, Value::Tuple(_) | Value::Record(_) | Value::Fcn(..) | Value::LazyFcn(_));
    if !is_fcn {
        return Ok(None);
    }
    let dom = ops::domain(b)?;
    if !ops::is_finite_set(&dom)? {
        return Ok(None);
    }
    let mut out = Vec::new();
    for k in ops::enumerate_normalized(&dom)? {
        let v = ops::apply(b, &k)?;
        out.push((k, v));
    }
    Ok(Some(out))
}

fn fcn_pairs(b: &Value, op: &str, what: &str) -> EResult<Vec<(Value, Value)>> {
    pairs(b)?.ok_or_else(|| wrong_value(op, what, b))
}

fn not_a_bag(which: &str, op: &str, b: &Value) -> EvalError {
    arg_err(format!("The {which} argument of {op} should be a bag, but instead it is:\n{b}"))
}

/// The count of a bag element (an integer).
fn count(v: &Value, which: &str, op: &str, b: &Value) -> EResult<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        _ => Err(not_a_bag(which, op, b)),
    }
}

/// TLC's `addCounts`: 32-bit overflow is an error.
fn add_counts(x: i64, y: i64) -> EResult<i64> {
    let r = x + y;
    if r > i32::MAX as i64 || r < i32::MIN as i64 {
        return Err(EvalError::of_kind(tlx_runtime::ErrorKind::Overflow, format!("Overflow when computing {x}+{y}")));
    }
    Ok(r)
}

/// The index of the first of `keys` equal to `k` by TLC's `k.equals(x)`,
/// which fails for values of different kinds (`"c" = 3`), as Bags.java's
/// linear searches do.
fn find_key<'a>(k: &Value, keys: impl Iterator<Item = &'a Value>) -> EResult<Option<usize>> {
    for (i, x) in keys.enumerate() {
        if tlx_value::order::eq_tlc(k, x)? {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

/// Accumulate (key, count) pairs in first-occurrence order (TLC builds
/// unnormalized functions; `fcn_rcd_from_pairs` normalizes them).
fn add_into(acc: &mut Vec<(Value, i64)>, k: Value, n: i64) -> EResult<()> {
    match find_key(&k, acc.iter().map(|(x, _)| x))? {
        Some(i) => acc[i].1 = add_counts(acc[i].1, n)?,
        None => acc.push((k, n)),
    }
    Ok(())
}

fn to_value(acc: Vec<(Value, i64)>) -> EResult<Value> {
    Ok(Value::fcn_rcd_from_pairs(acc.into_iter().map(|(k, n)| (k, Value::Int(n))).collect())?)
}

pub fn is_a_bag(b: &Value) -> EResult<Value> {
    Ok(Value::Bool(match pairs(b)? {
        None => false,
        Some(ps) => ps.iter().all(|(_, v)| matches!(v, Value::Int(i) if *i > 0)),
    }))
}

pub fn bag_cardinality(b: &Value) -> EResult<Value> {
    let ps = fcn_pairs(b, "BagCardinality", "a function with a finite domain")?;
    let mut n = 0;
    for (_, v) in &ps {
        match v {
            Value::Int(c) if *c > 0 => n = add_counts(n, *c)?,
            _ => return Err(wrong_value("BagCardinality", "a bag", b)),
        }
    }
    Ok(Value::Int(n))
}

/// `BagIn(e, B)` (`copies > 0`) and `CopiesIn(e, B)` (the count, 0 if absent).
pub fn bag_in(e: &Value, b: &Value, copies: bool) -> EResult<Value> {
    let op = if copies { "CopiesIn" } else { "BagIn" };
    // (TLC fails with a NullPointerException on a non-function.)
    let ps = pairs(b)?.ok_or_else(|| EvalError::new(format!("The second argument of {op} should be a bag, but instead it is:\n{b}")))?;
    if let Some(i) = find_key(e, ps.iter().map(|(k, _)| k))? {
        let v = &ps[i].1;
        let c = count(v, "second", op, b)?;
        return Ok(if copies { v.clone() } else { Value::Bool(c > 0) });
    }
    Ok(if copies { Value::Int(0) } else { Value::FALSE })
}

/// `B1 (+) B2`.
pub fn bag_cup(b1: &Value, b2: &Value) -> EResult<Value> {
    if !matches!(is_a_bag(b1)?, Value::Bool(true)) {
        return Err(not_a_bag("first", "(+)", b1));
    }
    if !matches!(is_a_bag(b2)?, Value::Bool(true)) {
        return Err(not_a_bag("second", "(+)", b2));
    }
    // TLC: B1's entries, then each key of B2 looked up among B1's keys.
    let mut acc = Vec::new();
    for (k, v) in fcn_pairs(b1, "(+)", "a bag")? {
        acc.push((k, count(&v, "first", "(+)", b1)?));
    }
    let n1 = acc.len();
    for (k, v) in fcn_pairs(b2, "(+)", "a bag")? {
        let n = count(&v, "first", "(+)", b1)?;
        match find_key(&k, acc[..n1].iter().map(|(x, _)| x))? {
            Some(i) => acc[i].1 = add_counts(acc[i].1, n)?,
            None => acc.push((k, n)),
        }
    }
    to_value(acc)
}

/// `B1 (-) B2`: the elements of B1 whose count stays positive.
pub fn bag_diff(b1: &Value, b2: &Value) -> EResult<Value> {
    let p1 = pairs(b1)?.ok_or_else(|| not_a_bag("first", "(-)", b1))?;
    let p2 = pairs(b2)?.ok_or_else(|| not_a_bag("second", "(-)", b2))?;
    let mut acc = Vec::new();
    for (k, v) in p1 {
        let mut n = count(&v, "first", "(-)", b1)?;
        if let Some(i) = find_key(&k, p2.iter().map(|(x, _)| x))? {
            let w = &p2[i].1;
            n -= count(w, "second", "(-)", b2)?;
        }
        if n > 0 {
            acc.push((k, n));
        }
    }
    to_value(acc)
}

/// `BagUnion(S)`: the sum of the (distinct) bags in S.
pub fn bag_union(s: &Value) -> EResult<Value> {
    let bags = ops::enumerate_normalized(s).map_err(|_| wrong_value("BagUnion", "a finite enumerable set", s))?;
    match bags.len() {
        0 => return Ok(Value::empty_fcn()),
        1 => return Ok(bags[0].clone()),
        _ => {}
    }
    let mut acc = Vec::new();
    for (i, b) in bags.iter().enumerate() {
        let ps =
            pairs(b)?.ok_or_else(|| arg_err(format!("Applying BagUnion to the following set, whose elements are not all bags:\n{s}")))?;
        for (k, v) in ps {
            let n = count(&v, "first", "BagUnion", b)?;
            if i == 0 {
                // TLC copies the first bag without comparing its keys.
                acc.push((k, n));
            } else {
                add_into(&mut acc, k, n)?;
            }
        }
    }
    to_value(acc)
}

/// `B1 \sqsubseteq B2`.
pub fn sq_subseteq(b1: &Value, b2: &Value) -> EResult<Value> {
    let p1 = fcn_pairs(b1, "\\sqsubseteq", "a function with a finite domain")?;
    let p2 = fcn_pairs(b2, "\\sqsubseteq", "a function with a finite domain")?;
    for (k, v) in p1 {
        let mut n = count(&v, "first", "\\sqsubseteq", b1)?;
        if let Some(i) = find_key(&k, p2.iter().map(|(x, _)| x))? {
            let w = &p2[i].1;
            n -= count(w, "second", "\\sqsubseteq", b2)?;
        }
        if n > 0 {
            return Ok(Value::FALSE);
        }
    }
    Ok(Value::TRUE)
}

pub fn bag_to_set(b: &Value) -> EResult<Value> {
    fcn_pairs(b, "BagToSet", "a function with a finite domain")?;
    Ok(ops::domain(b)?)
}

pub fn set_to_bag(s: &Value) -> EResult<Value> {
    let elems = ops::enumerate_normalized(s).map_err(|_| wrong_value("BagToSet", "a function with a finite domain", s))?;
    Ok(Value::fcn_rcd_from_pairs(elems.into_iter().map(|e| (e, Value::Int(1))).collect())?)
}

/// The pairs of `BagOfAll`'s / `FoldBag`'s bag argument.
pub fn bag_pairs(b: &Value, op: &str) -> EResult<Vec<(Value, Value)>> {
    pairs(b)?
        .ok_or_else(|| arg_err(format!("The second argument of {op} should be a function with a finite domain, but instead it is:\n{b}")))
}

/// Sum the counts of `(image, count)` pairs by image (`BagOfAll`). As
/// Bags.java, the counts are not checked: an image seen once keeps its
/// count as is (`BagOfAll(LAMBDA x : x, <<"a", 3>>)` is `<<"a", 3>>`), and
/// only counts added up must be integers (a cast to IntValue).
pub fn bag_of_images(images: Vec<(Value, Value)>, b: &Value) -> EResult<Value> {
    let mut acc: Vec<(Value, Value)> = Vec::new();
    for (k, v) in images {
        match find_key(&k, acc.iter().map(|(x, _)| x))? {
            Some(i) => {
                let (Value::Int(x), Value::Int(y)) = (&acc[i].1, &v) else {
                    return Err(not_a_bag("second", "BagOfAll", b));
                };
                acc[i].1 = Value::Int(add_counts(*x, *y)?);
            }
            None => acc.push((k, v)),
        }
    }
    Ok(Value::fcn_rcd_from_pairs(acc)?)
}
