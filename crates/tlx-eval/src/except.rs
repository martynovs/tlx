//! `[f EXCEPT ![k1]...[kn] = e, ...]`, the update step shared by both
//! dispatches: the tree-walking `Interp::except` and the resolved-tree
//! `Interp::exec_except`. One owner for the semantic decisions of an update:
//! the updates apply in order, each to the previous result; a key outside
//! the domain leaves the value unchanged (TLC warns) without evaluating the
//! replacement; the replacement is evaluated once with `@` bound to the old
//! point; the value is rebuilt with `ops::except`. The dispatches still
//! evaluate the base, the keys and the replacement themselves (the resolved
//! tree builds a multi-index key from its own code), and each states whether
//! an unchanged replacement keeps the function itself (`Unchanged`).

use tlx_value::{ops, Value};

use crate::apply::Cow;
use crate::env::Env;
use crate::error::EResult;
use crate::eval::{same_rep, AT};

/// What an update does when the replacement is the old point in the same
/// representation (`same_rep`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Unchanged {
    /// Rebuild with `ops::except` anyway (the tree walk, the reference).
    Rebuild,
    /// Keep the updated value itself, sharing its representation and its
    /// fingerprint (the resolved tree). A lazy function is still rebuilt:
    /// storing the point is what keeps it from being evaluated again.
    Keep,
}

/// `[base EXCEPT !keys(u1) = rhs(u1), !keys(u2) = rhs(u2), ...]`: for each
/// update in order, its keys (`keys`), then the update of the previous result
/// (`update`). The first error ends the evaluation.
pub(crate) fn except<U>(
    base: Cow<'_>,
    updates: &[U],
    env: &Env,
    unchanged: Unchanged,
    mut keys: impl FnMut(&U) -> EResult<Vec<Value>>,
    mut rhs: impl FnMut(&U, &Env) -> EResult<Value>,
) -> EResult<Value> {
    let mut v: Option<Value> = None;
    for u in updates {
        let keys = keys(u)?;
        let next = update(v.as_ref().unwrap_or(&base), &keys, env, unchanged, &mut |env: &Env| rhs(u, env))?;
        v = Some(next);
    }
    Ok(v.unwrap_or_else(|| base.into_owned()))
}

/// `[v EXCEPT ![k1][k2]... = rhs]` with `@` bound to the old value.
fn update<F: FnMut(&Env) -> EResult<Value>>(v: &Value, keys: &[Value], env: &Env, unchanged: Unchanged, rhs: &mut F) -> EResult<Value> {
    match keys.split_first() {
        None => rhs(&env.bind_val(AT, v.clone())),
        Some((k, rest)) => {
            // TLC warns and leaves the function unchanged when the key is
            // not in its domain.
            let Some(old) = ops::select(v, k)? else {
                return Ok(v.clone());
            };
            let new = update(&old, rest, env, unchanged, rhs)?;
            // Preserve the replacement's domain witness even when its
            // function pairs share storage with the old value (`same_rep`).
            if unchanged == Unchanged::Keep && same_rep(&old, &new) && !matches!(v, Value::LazyFcn(_)) {
                return Ok(v.clone());
            }
            Ok(ops::except(v, k, new)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tlx_value::lazy::LazyFcn;
    use tlx_value::{FcnDom, Sym};

    use super::*;
    use crate::env::Binding;

    fn int(n: i64) -> Value {
        Value::Int(n)
    }

    fn sym(s: &str) -> Sym {
        Sym::new(s)
    }

    /// The value bound to `@`.
    fn at(env: &Env) -> Value {
        match env.lookup(AT) {
            Some(Binding::Val(v)) => v.clone(),
            _ => panic!("@ is bound to a value"),
        }
    }

    /// `except` over updates given as (keys, replacement) pairs, where the
    /// replacement maps `@` to the new point; counts the replacement calls.
    fn run(base: &Value, updates: &[(Vec<Value>, fn(Value) -> EResult<Value>)], unchanged: Unchanged) -> (EResult<Value>, usize) {
        let calls = Cell::new(0);
        let r = except(
            Cow::Ref(base),
            updates,
            &Env::default(),
            unchanged,
            |(keys, _)| Ok(keys.clone()),
            |(_, rhs), env| {
                calls.set(calls.get() + 1);
                rhs(at(env))
            },
        );
        (r, calls.get())
    }

    fn incr(v: Value) -> EResult<Value> {
        let Value::Int(n) = v else { panic!("an integer point") };
        Ok(Value::Int(n + 10))
    }

    fn same(v: Value) -> EResult<Value> {
        Ok(v)
    }

    fn tuple(xs: &[i64]) -> Value {
        Value::tuple(xs.iter().copied().map(int).collect())
    }

    #[test]
    fn updates_apply_in_order_with_at_bound_to_the_previous_result() {
        let base = tuple(&[1, 2, 3]);
        let (r, calls) = run(&base, &[(vec![int(2)], incr), (vec![int(2)], incr), (vec![int(3)], incr)], Unchanged::Rebuild);
        assert_eq!(r.unwrap(), tuple(&[1, 22, 13]));
        assert_eq!(calls, 3);
    }

    #[test]
    fn no_update_returns_the_base() {
        let base = tuple(&[1]);
        let (r, calls) = run(&base, &[], Unchanged::Keep);
        assert!(matches!((r.unwrap(), &base), (Value::Tuple(x), Value::Tuple(y)) if Arc::ptr_eq(&x, y)));
        assert_eq!(calls, 0);
    }

    #[test]
    fn an_absent_key_leaves_the_value_and_skips_the_replacement() {
        let base = Value::Record(vec![(sym("a"), int(1))].into());
        for unchanged in [Unchanged::Rebuild, Unchanged::Keep] {
            let (r, calls) = run(&base, &[(vec![Value::Str(sym("b"))], incr)], unchanged);
            assert_eq!(r.unwrap(), base);
            assert_eq!(calls, 0, "{unchanged:?}");
            // Absent below the first key: the inner level is unchanged, the
            // outer level still takes its own step.
            let nested = tuple(&[]);
            let outer = Value::tuple(vec![nested.clone()]);
            let (r, calls) = run(&outer, &[(vec![int(1), int(5)], incr)], unchanged);
            assert_eq!(r.unwrap(), outer);
            assert_eq!(calls, 0);
        }
    }

    #[test]
    fn nested_keys_rebuild_each_level() {
        let inner = Value::Record(vec![(sym("a"), int(1)), (sym("b"), int(2))].into());
        let base = Value::tuple(vec![inner, int(9)]);
        let (r, calls) = run(&base, &[(vec![int(1), Value::Str(sym("a"))], incr)], Unchanged::Keep);
        let expected = Value::tuple(vec![Value::Record(vec![(sym("a"), int(11)), (sym("b"), int(2))].into()), int(9)]);
        assert_eq!(r.unwrap(), expected);
        assert_eq!(calls, 1);
    }

    #[test]
    fn the_first_error_ends_the_updates() {
        let base = tuple(&[1, 2]);
        fn fail(_: Value) -> EResult<Value> {
            Err(crate::EvalError::new("replacement failed"))
        }
        let (r, calls) = run(&base, &[(vec![int(1)], fail), (vec![int(2)], incr)], Unchanged::Keep);
        assert!(r.unwrap_err().to_string().contains("replacement failed"));
        assert_eq!(calls, 1, "the later update is not evaluated");
        // A key error (selecting from a non-function) comes before the
        // replacement.
        let (r, calls) = run(&int(3), &[(vec![int(1)], incr)], Unchanged::Keep);
        assert!(r.is_err());
        assert_eq!(calls, 0);
        // An error in the keys of a later update ends it before its
        // replacement, after the earlier update ran.
        let key_calls = Cell::new(0);
        let r = except(
            Cow::Ref(&base),
            &[1, 2],
            &Env::default(),
            Unchanged::Rebuild,
            |&u| if u == 2 { Err(crate::EvalError::new("key failed")) } else { Ok(vec![int(u)]) },
            |_, env| {
                key_calls.set(key_calls.get() + 1);
                incr(at(env))
            },
        );
        assert!(r.unwrap_err().to_string().contains("key failed"));
        assert_eq!(key_calls.get(), 1);
    }

    /// The `Unchanged` boundary: only `Keep` returns the updated value itself
    /// for a replacement in the same representation; both agree on the value.
    #[test]
    fn keep_shares_an_unchanged_replacement_and_rebuild_does_not() {
        let point = tuple(&[5]);
        let base = Value::Record(vec![(sym("a"), point)].into());
        let key = vec![Value::Str(sym("a"))];
        let (kept, _) = run(&base, &[(key.clone(), same)], Unchanged::Keep);
        let (rebuilt, _) = run(&base, &[(key, same)], Unchanged::Rebuild);
        let (Value::Record(b), Value::Record(k), Value::Record(r)) = (&base, kept.unwrap(), rebuilt.unwrap()) else { panic!("records") };
        assert!(Arc::ptr_eq(b, &k));
        assert!(!Arc::ptr_eq(b, &r));
        assert_eq!(k, r);
    }

    /// Shared pairs with another domain witness are a different
    /// representation: `Keep` stores the replacement (D10).
    #[test]
    fn keep_stores_a_replacement_with_another_domain_witness() {
        let pairs: Arc<[(Value, Value)]> = vec![(int(0), int(7))].into();
        let base = Value::tuple(vec![Value::Fcn(pairs.clone(), FcnDom::Enumerated)]);
        fn interval(v: Value) -> EResult<Value> {
            let Value::Fcn(ps, _) = v else { panic!("a function point") };
            Ok(Value::Fcn(ps, FcnDom::Interval))
        }
        let (r, _) = run(&base, &[(vec![int(1)], interval)], Unchanged::Keep);
        let inner = ops::select(&r.unwrap(), &int(1)).unwrap().unwrap();
        assert!(matches!(&inner, Value::Fcn(ps, FcnDom::Interval) if Arc::ptr_eq(ps, &pairs)));
    }

    /// An unchanged point of a lazy function is still stored: the function
    /// does not evaluate it again.
    #[test]
    fn keep_stores_an_unchanged_point_of_a_lazy_function() {
        #[derive(Debug)]
        struct Counting(Arc<AtomicUsize>);
        impl LazyFcn for Counting {
            fn domain(&self) -> tlx_value::Result<Value> {
                Ok(Value::Set(vec![int(1)].into()))
            }
            fn apply(&self, _: &Value) -> tlx_value::Result<Value> {
                self.0.fetch_add(1, Ordering::Relaxed);
                Ok(int(7))
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let base = Value::LazyFcn(Arc::new(Counting(calls.clone())));
        let (r, rhs_calls) = run(&base, &[(vec![int(1)], same)], Unchanged::Keep);
        let r = r.unwrap();
        assert_eq!((calls.load(Ordering::Relaxed), rhs_calls), (1, 1));
        assert_eq!(ops::apply(&r, &int(1)).unwrap(), int(7));
        assert_eq!(calls.load(Ordering::Relaxed), 1, "the stored point is not evaluated again");
    }
}
