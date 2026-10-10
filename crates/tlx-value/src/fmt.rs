//! TLC-style printing: `{1, 2}`, `<<1, "a">>`, `[a |-> 1, b |-> 2]`,
//! `(1 :> "x" @@ 2 :> "y")`, `1..3`, model values bare, strings quoted.
//!
//! Lazy sets are printed expanded (normalized) when TLC would expand them
//! (`TLCGlobals.expand`, enumBound 2000; `SUBSET S` only when |S| < 7) and
//! symbolically otherwise (`SUBSET (1..3)`, `[S -> T]`, `(S \X T)`,
//! `[a: S]`, `Seq(S)`, `S \cup T`, `UNION(S)`, `Nat`, ...).

use std::cell::Cell;
use std::fmt;

use crate::lazy::LazySet;
use crate::value::Value;

/// TLC's `TLCGlobals.enumBound`.
const ENUM_BOUND: i64 = 2000;

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(true) => f.write_str("TRUE"),
            Value::Bool(false) => f.write_str("FALSE"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Str(s) => write_string(f, s.as_str()),
            Value::ModelValue(s) => f.write_str(s.as_str()),
            Value::Set(elems) => write_set(f, elems),
            Value::Interval(lo, hi) => {
                if lo <= hi {
                    write!(f, "{lo}..{hi}")
                } else {
                    f.write_str("{}")
                }
            }
            Value::Tuple(elems) => write_seq(f, "<<", elems, ", ", ">>", |f, e| write!(f, "{e}")),
            // Like TLC (FcnRcdValue.isRcd): as a record only if every field
            // is a name, otherwise as a function (`("" :> 1)`).
            Value::Record(fields) if fields.iter().all(|(k, _)| is_name(k.as_str())) => {
                write_seq(f, "[", fields, ", ", "]", |f, (k, v)| write!(f, "{} |-> {v}", k.as_str()))
            }
            Value::Record(fields) => write_seq(f, "(", fields, " @@ ", ")", |f, (k, v)| {
                write_string(f, k.as_str())?;
                write!(f, " :> {v}")
            }),
            // A function over an enumerated domain prints as the equal
            // `Tuple` / `Record` (TLC's FcnRcdValue printing).
            Value::Fcn(pairs, _) => match self.fcn_canonical() {
                Some(c) => write!(f, "{c}"),
                None => write_seq(f, "(", pairs, " @@ ", ")", |f, (k, v)| write!(f, "{k} :> {v}")),
            },
            Value::LazySet(l) => write_lazy_set(f, self, l),
            Value::LazyFcn(lf) => match expanding(|| crate::value::lazy_fcn_to_value(&**lf).ok()) {
                Some(v) => write!(f, "{v}"),
                None => match expanding(|| lf.domain().ok()) {
                    Some(d) => write!(f, "[x \\in {d} |-> ...]"),
                    None => write!(f, "{lf:?}"),
                },
            },
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// TLC's `FcnRcdValue.isName`: letters, digits and `_` with at least one
/// letter, and not `WF_...`/`SF_...`.
fn is_name(s: &str) -> bool {
    s.chars().all(|c| c == '_' || c.is_alphanumeric())
        && s.chars().any(|c| c.is_alphabetic())
        && !(s.len() >= 4 && (s.starts_with("WF_") || s.starts_with("SF_")))
}

fn write_string(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_str("\"")?;
    for c in s.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\t' => f.write_str("\\t")?,
            '\n' => f.write_str("\\n")?,
            '\u{c}' => f.write_str("\\f")?,
            '\r' => f.write_str("\\r")?,
            c => write!(f, "{c}")?,
        }
    }
    f.write_str("\"")
}

/// `open item1 sep item2 ... close`.
fn write_seq<T>(
    f: &mut fmt::Formatter<'_>,
    open: &str,
    items: &[T],
    sep: &str,
    close: &str,
    item: impl Fn(&mut fmt::Formatter<'_>, &T) -> fmt::Result,
) -> fmt::Result {
    f.write_str(open)?;
    for (i, e) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(sep)?;
        }
        item(f, e)?;
    }
    f.write_str(close)
}

fn write_set(f: &mut fmt::Formatter<'_>, elems: &[Value]) -> fmt::Result {
    write_seq(f, "{", elems, ", ", "}", |f, e| write!(f, "{e}"))
}

thread_local! {
    /// Set while a lazy value is being expanded for printing: values printed
    /// meanwhile (e.g. inside error messages) stay symbolic, which prevents
    /// infinite recursion.
    static EXPANDING: Cell<bool> = const { Cell::new(false) };
}

/// Run `f` with expansion disabled for nested printing; `None` if already
/// inside an expansion.
fn expanding<T>(f: impl FnOnce() -> Option<T>) -> Option<T> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            EXPANDING.with(|e| e.set(false));
        }
    }
    if EXPANDING.with(|e| e.replace(true)) {
        return None;
    }
    let _reset = Reset;
    f()
}

/// Print the normalized set if it can be computed and `small` holds.
fn try_expand(f: &mut fmt::Formatter<'_>, v: &Value, small: bool) -> Option<fmt::Result> {
    if !small {
        return None;
    }
    let elems = expanding(|| crate::ops::enumerate_normalized(v).ok())?;
    Some(write_set(f, &elems))
}

fn write_lazy_set(f: &mut fmt::Formatter<'_>, v: &Value, l: &LazySet) -> fmt::Result {
    let card = |s: &Value| expanding(|| crate::ops::size(s).ok());
    let small = |s: &Value| card(s).is_some_and(|n| n < ENUM_BOUND);
    match l {
        LazySet::Enum(es) => write_set(f, es),
        LazySet::Subset(s) => {
            if let Some(r) = try_expand(f, v, card(s).is_some_and(|n| n < 7)) {
                return r;
            }
            if matches!(s, Value::Interval(..)) {
                write!(f, "SUBSET ({s})")
            } else {
                write!(f, "SUBSET {s}")
            }
        }
        LazySet::FcnSet(d, r) => {
            if let Some(res) = try_expand(f, v, small(v)) {
                return res;
            }
            write!(f, "[{d} -> {r}]")
        }
        LazySet::Product(sets) => {
            if let Some(res) = try_expand(f, v, small(v)) {
                return res;
            }
            if sets.is_empty() {
                return Ok(());
            }
            write_seq(f, "(", sets, " \\X ", ")", |f, s| write!(f, "{s}"))
        }
        LazySet::RecordSet(fields) => {
            if let Some(res) = try_expand(f, v, small(v)) {
                return res;
            }
            write_seq(f, "[", fields, ", ", "]", |f, (k, s)| write!(f, "{}: {s}", k.as_str()))
        }
        LazySet::Seq(s) => write!(f, "Seq({s})"),
        // KSubsetValue.toString: the elements if fewer than 64 (a failing
        // size is swallowed), else the definition.
        LazySet::KSubset(k, base) => {
            if let Some(es) = expanding(|| {
                crate::ops::ksubset::ksubset_expands(*k, base)
                    .ok()
                    .filter(|&b| b)
                    .and_then(|_| crate::ops::ksubset::ksubset_elements(*k, base).ok())
            }) {
                return write_set(f, &es);
            }
            write!(f, "{{s \\in SUBSET ({base}) : Cardinality(s) = {k}}}")
        }
        LazySet::Union(s) => {
            if let Some(res) = try_expand(f, v, true) {
                return res;
            }
            write!(f, "UNION({s})")
        }
        LazySet::Cup(a, b) => {
            if let Some(res) = try_expand(f, v, true) {
                return res;
            }
            write!(f, "{a} \\cup {b}")
        }
        LazySet::Cap(a, b) => {
            if let Some(res) = try_expand(f, v, true) {
                return res;
            }
            write!(f, "{a} \\cap {b}")
        }
        LazySet::Diff(a, b) => {
            if let Some(res) = try_expand(f, v, true) {
                return res;
            }
            write!(f, "{a} \\ {b}")
        }
        LazySet::Nat => f.write_str("Nat"),
        LazySet::Int => f.write_str("Int"),
        LazySet::StringSet => f.write_str("STRING"),
        LazySet::Any => f.write_str("ANY"),
        LazySet::Custom(c) => {
            if let Some(res) = try_expand(f, v, true) {
                return res;
            }
            write!(f, "{c:?}")
        }
    }
}

/// Check that `v` can be printed the way TLC's `ToString`, `Print` and
/// `PrintT` print it (`Value.toStringUnchecked`): there TLC expands lazy
/// values as `Display` does, but an expansion of a set that fails (a
/// non-enumerable `{x \in S : P}`, `SUBSET FALSE`, `[Nat -> S]`, ...) is an
/// error instead of the symbolic form. (A lazy function, `[x \in Nat |-> e]`,
/// is printed leniently even there.) Error messages and traces
/// print with `Display` (TLC's `toString`, which swallows these errors).
pub fn check_printable(v: &Value) -> crate::Result<()> {
    printable(v).map(drop)
}

/// `check_printable` that also returns what to print: `v` with each lazy set
/// that the check expands replaced by its normalized elements, so that
/// `Display` of the result prints the same text as `Display` of `v` without
/// enumerating those sets a second time (a `PrintT` of a large set filter
/// computed it twice).
pub fn printable(v: &Value) -> crate::Result<Value> {
    use crate::ops::{enumerate_normalized, size as cardinality};
    if v.is_normal() {
        return Ok(v.clone());
    }
    let all = |es: &[Value]| es.iter().map(printable).collect::<crate::Result<Vec<_>>>();
    let expand = |v: &Value| -> crate::Result<Value> { Ok(Value::Set(all(&enumerate_normalized(v)?)?.into())) };
    // Product of cardinalities, saturating like TLC's (capped at i32::MAX).
    let product = |sizes: &mut dyn Iterator<Item = &Value>| -> crate::Result<i64> {
        let mut n: i64 = 1;
        for s in sizes {
            let k = cardinality(s)?;
            if k == 0 {
                return Ok(0);
            }
            if n <= i32::MAX as i64 {
                n = n.saturating_mul(k);
            }
        }
        Ok(n)
    };
    match v {
        Value::Set(es) => Ok(Value::Set(all(es)?.into())),
        Value::Tuple(es) => Ok(Value::Tuple(all(es)?.into())),
        Value::Record(fs) => Ok(Value::Record(fs.iter().map(|(k, x)| Ok((*k, printable(x)?))).collect::<crate::Result<Vec<_>>>()?.into())),
        Value::Fcn(ps, d) => {
            Ok(Value::Fcn(ps.iter().map(|(k, x)| Ok((printable(k)?, printable(x)?))).collect::<crate::Result<Vec<_>>>()?.into(), *d))
        }
        // FcnLambdaValue.toString swallows: it prints the materialized
        // function leniently, or the symbolic form if that fails.
        Value::LazyFcn(_) => Ok(v.clone()),
        // A set that is not expanded keeps its symbolic form (`Display`
        // prints it so); its parts are only checked.
        Value::LazySet(l) => match &**l {
            LazySet::Enum(_) => expand(v),
            // SubsetValue: |S| < 7 is expanded.
            LazySet::Subset(s) => {
                if cardinality(s)? < 7 {
                    expand(v)
                } else {
                    check_printable(s).map(|()| v.clone())
                }
            }
            // SetOfFcnsValue: |R|^|D| (1 for an empty D) < enumBound.
            LazySet::FcnSet(d, r) => {
                let dn = cardinality(d)?;
                let n = if dn == 0 { 1 } else { cardinality(r)?.saturating_pow(dn.min(64) as u32) };
                if n < ENUM_BOUND {
                    expand(v)
                } else {
                    check_printable(d).and_then(|()| check_printable(r)).map(|()| v.clone())
                }
            }
            // SetOfTuplesValue / SetOfRcdsValue: the product of the sizes.
            LazySet::Product(sets) => {
                if product(&mut sets.iter())? < ENUM_BOUND {
                    expand(v)
                } else {
                    sets.iter().try_for_each(check_printable).map(|()| v.clone())
                }
            }
            LazySet::RecordSet(fields) => {
                if product(&mut fields.iter().map(|(_, s)| s))? < ENUM_BOUND {
                    expand(v)
                } else {
                    fields.iter().try_for_each(|(_, s)| check_printable(s)).map(|()| v.clone())
                }
            }
            LazySet::Seq(s) => check_printable(s).map(|()| v.clone()),
            LazySet::KSubset(k, base) => {
                if crate::ops::ksubset::ksubset_expands(*k, base)? {
                    Ok(Value::Set(all(&crate::ops::ksubset::ksubset_elements(*k, base)?)?.into()))
                } else {
                    check_printable(base).map(|()| v.clone())
                }
            }
            LazySet::Nat | LazySet::Int | LazySet::StringSet | LazySet::Any => Ok(v.clone()),
            // UnionValue, SetCupValue, SetCapValue, SetDiffValue,
            // SetPredValue: always expanded.
            LazySet::Union(_) | LazySet::Cup(..) | LazySet::Cap(..) | LazySet::Diff(..) | LazySet::Custom(_) => expand(v),
        },
        Value::Bool(_) | Value::Int(_) | Value::Str(_) | Value::ModelValue(_) | Value::Interval(..) => Ok(v.clone()),
    }
}
