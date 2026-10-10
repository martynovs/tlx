//! The borrowed-or-owned application/field tail, shared by both dispatches:
//! the tree-walking `Interp::eval_cow` (`fn_app_cow`/`field_cow`) and the
//! resolved-tree `Interp::exec_cow` (`exec_fn_app`/`exec_field`). Applying a
//! function or selecting a field returns the stored value by reference when
//! the function (or record) is borrowed, and otherwise computes the value and
//! hands it to `kept_point`, which builds a function kept lazy only by
//! KeepLazy (TLC would build it when it is used). One owner for that tail and
//! for the `Cow` result it produces; the two dispatches still decide their own
//! function and argument evaluation (the resolved tree looks a tuple key up
//! without building it).

use tlx_value::{ops, Sym, Value};

use crate::error::EResult;
use crate::eval::Ctx;

/// A value read where it is stored (a state, an environment, a constant)
/// instead of copied, or computed (`Interp::eval_cow`): reading a compound
/// value without copying it saves two reference count updates (atomic
/// operations).
pub(crate) enum Cow<'a> {
    Ref(&'a Value),
    Own(Value),
}

impl std::ops::Deref for Cow<'_> {
    type Target = Value;

    #[inline]
    fn deref(&self) -> &Value {
        match self {
            Cow::Ref(v) => v,
            Cow::Own(v) => v,
        }
    }
}

impl Cow<'_> {
    #[inline]
    pub(crate) fn into_owned(self) -> Value {
        match self {
            Cow::Ref(v) => v.clone(),
            Cow::Own(v) => v,
        }
    }
}

/// Build a function that is lazy only because of KeepLazy when it leaves the
/// application that kept it lazy (TLC would build it when it is used, e.g.
/// fingerprinted). Only the value itself is inspected, not its parts.
/// `f[x]`'s value `v` as the caller sees it: tuple, record and explicit
/// function selection returns the stored value unchanged (Tool.OPCODE_fa);
/// only a lambda application evaluates a body under the caller's KeepLazy
/// control (`force_kept` unless the caller keeps it lazy).
pub(crate) fn kept_point(f: &Value, v: Value, cx: &Ctx) -> EResult<Value> {
    if cx.keep_lazy || !matches!(f, Value::LazyFcn(_)) {
        Ok(v)
    } else {
        force_kept(v)
    }
}

pub(crate) fn force_kept(v: Value) -> EResult<Value> {
    match &v {
        Value::LazyFcn(f) if f.kept_lazy() => {
            let mut pairs = Vec::new();
            let domain = f.domain()?;
            for k in ops::enumerate_normalized(&domain)? {
                let x = f.apply(&k)?;
                pairs.push((k, force_kept(x)?));
            }
            Ok(Value::fcn_over(&domain, pairs)?)
        }
        _ => Ok(v),
    }
}

/// `f[arg]` (the tail of `fn_app_cow` and `exec_fn_app`): the point stored in
/// a borrowed `f`, or the computed value, finalized by `kept_point`. An owned
/// `f` (a function just built, e.g. a temporary tuple) has no stored point to
/// borrow from, so it is applied directly.
pub(crate) fn apply_tail<'a>(f: Cow<'a>, arg: &Value, cx: &Ctx) -> EResult<Cow<'a>> {
    match f {
        Cow::Ref(fr) => match ops::apply_ref(fr, arg)? {
            Some(v) => Ok(Cow::Ref(v)),
            None => {
                let v = ops::apply(fr, arg)?;
                Ok(Cow::Own(kept_point(fr, v, cx)?))
            }
        },
        Cow::Own(fo) => {
            let v = ops::apply(&fo, arg)?;
            Ok(Cow::Own(kept_point(&fo, v, cx)?))
        }
    }
}

/// `r.field` (the tail of `field_cow` and `exec_field`): the field stored in
/// a borrowed `r`, or the computed value.
pub(crate) fn field_tail<'a>(r: Cow<'a>, field: Sym) -> EResult<Cow<'a>> {
    match r {
        Cow::Ref(rr) => match ops::field_ref(rr, field)? {
            Some(v) => Ok(Cow::Ref(v)),
            None => Ok(Cow::Own(ops::field(rr, field)?)),
        },
        Cow::Own(ro) => Ok(Cow::Own(ops::field(&ro, field)?)),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tlx_value::lazy::LazyFcn;
    use tlx_value::Value;

    use super::*;

    /// A lazy function that is lazy only because of KeepLazy, and counts its
    /// applications.
    #[derive(Debug)]
    struct KeptFcn(Arc<AtomicUsize>);

    impl LazyFcn for KeptFcn {
        fn domain(&self) -> tlx_value::Result<Value> {
            Ok(Value::Set(vec![Value::Int(0)].into()))
        }

        fn apply(&self, _: &Value) -> tlx_value::Result<Value> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(Value::Int(9))
        }

        fn kept_lazy(&self) -> bool {
            true
        }
    }

    fn sym(s: &str) -> Sym {
        Sym::new(s)
    }

    fn kept_fcn() -> Value {
        Value::LazyFcn(Arc::new(KeptFcn(Arc::new(AtomicUsize::new(0)))))
    }

    /// The message of a failed selection (`Cow` is not `Debug`).
    fn err_of(r: EResult<Cow<'_>>) -> String {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    /// A borrowed function's stored point is returned by reference, not
    /// cloned; a lazy function has no stored point and is computed.
    #[test]
    fn apply_tail_borrows_a_stored_point_and_computes_a_lazy_one() {
        let tuple = Value::tuple(vec![Value::Int(1), Value::Int(2)]);
        let Cow::Ref(v) = apply_tail(Cow::Ref(&tuple), &Value::Int(2), &Ctx::NONE).unwrap() else {
            panic!("a stored tuple element is borrowed");
        };
        let Value::Tuple(items) = &tuple else { panic!("tuple") };
        assert!(std::ptr::eq(v, &items[1]), "the stored element is returned by reference");

        let calls = Arc::new(AtomicUsize::new(0));
        let lazy = Value::LazyFcn(Arc::new(KeptFcn(calls.clone())));
        let Cow::Own(v) = apply_tail(Cow::Ref(&lazy), &Value::Int(0), &Ctx::NONE).unwrap() else {
            panic!("an applied lazy function is computed");
        };
        assert_eq!(v, Value::Int(9));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    /// The borrowed and owned branches of the tail agree on the value (an
    /// owned value, e.g. a just-built temporary, has nothing to borrow from).
    #[test]
    fn apply_tail_borrowed_and_owned_agree() {
        let tuple = Value::tuple(vec![Value::Int(1), Value::Int(2)]);
        let borrowed = apply_tail(Cow::Ref(&tuple), &Value::Int(2), &Ctx::NONE).unwrap().into_owned();
        let owned = apply_tail(Cow::Own(tuple.clone()), &Value::Int(2), &Ctx::NONE).unwrap().into_owned();
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed, Value::Int(2));
    }

    /// `kept_point` builds a function kept lazy only by KeepLazy, unless the
    /// caller keeps it lazy (KeepLazy) or the value is not such a function.
    #[test]
    fn kept_point_builds_a_keep_lazy_function_only_when_it_escapes() {
        let lazy = kept_fcn();
        let kept = kept_point(&lazy, lazy.clone(), &Ctx::NONE).unwrap();
        let Value::Fcn(pairs, _) = &kept else { panic!("the escaping function is built: {kept}") };
        assert_eq!(pairs[0].1, Value::Int(9));
        assert!(matches!(kept_point(&lazy, lazy.clone(), &Ctx::NONE.lazy(true)).unwrap(), Value::LazyFcn(_)));
        assert_eq!(kept_point(&Value::Int(0), Value::Int(5), &Ctx::NONE).unwrap(), Value::Int(5));
    }

    /// Selection keeps `apply_ref`'s errors (type and bounds) unchanged.
    #[test]
    fn apply_tail_keeps_apply_ref_errors() {
        let int = Value::Int(0);
        assert!(err_of(apply_tail(Cow::Ref(&int), &Value::Int(1), &Ctx::NONE)).contains("non-function"));

        let tuple = Value::tuple(vec![Value::Int(1), Value::Int(2)]);
        assert!(err_of(apply_tail(Cow::Ref(&tuple), &Value::Int(3), &Ctx::NONE)).contains("out of bounds"));
        assert!(err_of(apply_tail(Cow::Ref(&tuple), &Value::Str(sym("x")), &Ctx::NONE)).contains("non integral index"));
    }

    /// A record field is borrowed by reference; a lazy function's field is
    /// computed (deferred), like `r.field` on a function value.
    #[test]
    fn field_tail_borrows_a_record_and_computes_a_lazy_field() {
        let record = Value::Record(vec![(sym("a"), Value::Int(1)), (sym("b"), Value::Int(2))].into());
        let Cow::Ref(v) = field_tail(Cow::Ref(&record), sym("b")).unwrap() else { panic!("a record field is borrowed") };
        let Value::Record(fields) = &record else { panic!("record") };
        assert!(std::ptr::eq(v, &fields[1].1));

        assert_eq!(field_tail(Cow::Ref(&kept_fcn()), sym("a")).unwrap().into_owned(), Value::Int(9));
    }

    /// Both dispatches run the shared tail: the program-owned expression goes
    /// through the resolved tree (`exec_fn_app`/`exec_field`, a tuple key
    /// looked up without building it), a fresh HIR clone through the tree
    /// walk; they agree on the value and on the error.
    #[test]
    fn code_and_tree_dispatches_agree_through_the_shared_tail() {
        use crate::env::Env;

        let dir = tlx_testkit::TempDir::new("apply-dispatch");
        let path = dir.write(
            "Apply.tla",
            &tlx_testkit::module(
                "Apply",
                "EXTENDS Naturals\nKeyed == [e \\in {<<1, 2>>, <<3, 4>>} |-> e]\nByArgs == Keyed[3, 4]\nByTuple == Keyed[<<3, 4>>]\nField == [a |-> 7].a\nAbsent == Keyed[5, 6]\n",
            ),
        );
        let prog = tlx_sema::Program::load(&path, &[]).unwrap();
        let ip = crate::interp::Interp::new(prog, Default::default()).unwrap();
        let eval = |name: &str| -> (Result<Value, String>, Result<Value, String>) {
            let def = ip.prog.defs.iter().find(|d| &*d.name == name).expect("definition");
            let tlx_sema::hir::DefKind::Op(body) = &def.kind else { panic!("{name} is an operator") };
            // A fresh HIR root bypasses code_of and exercises the tree walk;
            // the program-owned root uses code.rs.
            let mut tree = body.clone();
            tree.kind = Arc::new((*body.kind).clone());
            let code = ip.eval(body, &Env::default(), &Ctx::NONE).map_err(|e| e.to_string());
            let walk = ip.eval(&tree, &Env::default(), &Ctx::NONE).map_err(|e| e.to_string());
            (code, walk)
        };
        let tuple = Value::tuple(vec![Value::Int(3), Value::Int(4)]);
        for name in ["ByArgs", "ByTuple"] {
            let (code, walk) = eval(name);
            assert_eq!(code, walk, "{name}");
            assert_eq!(code.unwrap(), tuple, "{name}");
        }
        let (code, walk) = eval("Field");
        assert_eq!(code, walk);
        assert_eq!(code.unwrap(), Value::Int(7));
        let (code, walk) = eval("Absent");
        assert_eq!(code, walk);
        assert!(code.unwrap_err().contains("not in the domain of the function"));
    }

    /// A missing field is an error, not a deferred miss; a non-function keeps
    /// `field`'s error.
    #[test]
    fn field_tail_reports_a_missing_field_and_a_non_function() {
        let record = Value::Record(vec![(sym("a"), Value::Int(1))].into());
        assert!(err_of(field_tail(Cow::Ref(&record), sym("b"))).contains("nonexistent field"));
        let int = Value::Int(0);
        assert!(err_of(field_tail(Cow::Ref(&int), sym("a"))).contains("non-function"));
    }
}
