mod common;

use std::cmp::Ordering;
use std::collections::HashSet;

use common::*;
use tlx_value::order::{cmp_tlc, eq_tlc};
use tlx_value::{ops, Sym, Value};

#[test]
fn strings_sort_in_intern_order() {
    // Intern "ord_b" before "ord_a": the set {"ord_a", "ord_b"} sorts b first.
    Sym::new("ord_b");
    Sym::new("ord_a");
    let v = set(vec![s("ord_a"), s("ord_b")]);
    assert_eq!(v.to_string(), r#"{"ord_b", "ord_a"}"#);
    assert_eq!(cmp_tlc(&s("ord_b"), &s("ord_a")).unwrap(), Ordering::Less);
    assert!(s("ord_b") < s("ord_a"));
    // CHOOSE-relevant normalized enumeration follows the same order.
    let u = ops::union(&set(vec![s("ord_a")]), &set(vec![s("ord_b")])).unwrap();
    assert_eq!(show(&ops::enumerate_normalized(&u).unwrap()), r#""ord_b" "ord_a""#);
    // Model values too.
    Sym::new("mv_z");
    Sym::new("mv_y");
    assert_eq!(set(vec![mv("mv_y"), mv("mv_z")]).to_string(), "{mv_z, mv_y}");
}

#[test]
fn values_are_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<Value>();
    check::<tlx_value::fingerprint::Fingerprinter>();
}

#[test]
fn interner_is_thread_safe() {
    // Miri interleaves the threads faithfully but runs ~1000x slower; the
    // thread structure is the point, so shrink the count under Miri only.
    let names: usize = if cfg!(miri) { 64 } else { 2000 };
    let handles: Vec<_> = (0..8)
        .map(|t| {
            std::thread::spawn(move || {
                (0..names)
                    .map(|i| {
                        let name = format!("thr_{}", (i * 7 + t * 13) % names);
                        let sym = Sym::new(&name);
                        assert_eq!(sym.as_str(), name);
                        (name, sym.id())
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut ids = std::collections::HashMap::new();
    for h in handles {
        for (name, id) in h.join().unwrap() {
            assert_eq!(*ids.entry(name).or_insert(id), id);
        }
    }
    assert_eq!(ids.len(), names);
    let distinct: HashSet<u32> = ids.values().copied().collect();
    assert_eq!(distinct.len(), names);
}

#[test]
fn scalar_order() {
    assert_eq!(cmp_tlc(&Value::FALSE, &Value::TRUE).unwrap(), Ordering::Less);
    assert_eq!(cmp_tlc(&int(-3), &int(2)).unwrap(), Ordering::Less);
    assert!(cmp_tlc(&int(1), &s("a")).is_err());
    assert!(cmp_tlc(&s("a"), &Value::TRUE).is_err());
    assert!(cmp_tlc(&Value::TRUE, &int(1)).is_err());
    let m = err_msg(cmp_tlc(&int(1), &s("a")));
    assert_eq!(m, "Attempted to compare integer 1 with non-integer:\n\"a\"");
}

#[test]
fn model_values() {
    let m = mv("m1");
    // untyped MV sorts before everything else, consistently in both directions
    for other in [int(1), s("x"), Value::TRUE, ints(&[1]), tup(vec![int(1)]), rec(&[("a", int(1))])] {
        assert_eq!(cmp_tlc(&m, &other).unwrap(), Ordering::Less, "{other}");
        assert_eq!(cmp_tlc(&other, &m).unwrap(), Ordering::Greater, "{other}");
        assert!(!eq_tlc(&m, &other).unwrap());
        assert!(!eq_tlc(&other, &m).unwrap());
        assert!(m < other);
    }
    assert_eq!(set(vec![int(2), m.clone(), int(1)]).to_string(), "{m1, 1, 2}");
    assert!(eq_tlc(&m, &mv("m1")).unwrap());
    assert!(!eq_tlc(&m, &mv("m2")).unwrap());

    // typed model values: x_1 vs non-MV and vs differently typed MV error
    let x1 = mv("x_1");
    assert!(eq_tlc(&x1, &int(1)).is_err());
    assert!(eq_tlc(&int(1), &x1).is_err());
    assert!(cmp_tlc(&x1, &int(1)).is_err());
    assert!(cmp_tlc(&int(1), &x1).is_err());
    assert!(eq_tlc(&x1, &mv("y_1")).is_err());
    assert!(cmp_tlc(&x1, &mv("y_1")).is_err());
    assert!(!eq_tlc(&x1, &mv("x_2")).unwrap());
    assert!(!eq_tlc(&x1, &m).unwrap());
    assert!(!eq_tlc(&m, &x1).unwrap());
    assert!(eq_tlc(&x1, &mv("x_1")).unwrap());
    // "x_" (length 2) is untyped
    assert!(!eq_tlc(&mv("x_"), &int(1)).unwrap());
    assert_eq!(Sym::new("x_1").model_value_type(), Some('x'));
    assert_eq!(Sym::new("x_").model_value_type(), None);
    // membership
    assert!(!ops::member(&m, &ints(&[1, 2])).unwrap());
    assert!(ops::member(&x1, &ints(&[1, 2])).is_err());
    assert!(!ops::member(&m, &ops::interval(1, 3)).unwrap());
    assert!(ops::member(&x1, &ops::interval(1, 3)).is_err());
    assert!(!ops::member(&x1, &ops::interval(3, 1)).unwrap());
}

#[test]
fn equality_matrix() {
    // errors
    assert!(eq_tlc(&int(1), &s("a")).is_err());
    assert!(eq_tlc(&int(1), &ints(&[1])).is_err());
    assert!(eq_tlc(&Value::TRUE, &int(1)).is_err());
    assert!(eq_tlc(&Value::empty_set(), &int(0)).is_err());
    assert!(eq_tlc(&tup(vec![]), &Value::empty_set()).is_err());
    assert!(eq_tlc(&Value::empty_set(), &tup(vec![])).is_err());
    assert!(eq_tlc(&s("abc"), &tup(vec![s("a"), s("b"), s("c")])).is_err());
    assert!(eq_tlc(&tup(vec![s("a")]), &s("a")).is_err());
    // the empty function is <<>>
    assert!(eq_tlc(&fcn(vec![]), &tup(vec![])).unwrap());
    // <<a, b>> = [i \in 1..2 |-> ...]
    let f = fcn(vec![(int(2), s("b")), (int(1), s("a"))]);
    assert!(eq_tlc(&tup(vec![s("a"), s("b")]), &f).unwrap());
    // [a |-> 1] = [x \in {"a"} |-> 1] (both directions)
    let r = rec(&[("a", int(1))]);
    let g = fcn(vec![(s("a"), int(1))]);
    assert!(eq_tlc(&r, &g).unwrap() && eq_tlc(&g, &r).unwrap());
    // D-31: function equality is independent of its representation.
    let t1 = tup(vec![int(1)]);
    let r2 = rec(&[("a", int(1)), ("b", int(2))]);
    assert!(!eq_tlc(&t1, &r).unwrap());
    assert!(!eq_tlc(&t1, &r2).unwrap());
    assert!(!eq_tlc(&r2, &t1).unwrap());
    assert!(!eq_tlc(&r, &t1).unwrap());
    assert!(!eq_tlc(&r, &tup(vec![])).unwrap());
    assert_eq!(cmp_tlc(&t1, &r).unwrap(), Ordering::Less);
    assert_eq!(cmp_tlc(&t1, &r2).unwrap(), Ordering::Less);
    assert_eq!(cmp_tlc(&r2, &t1).unwrap(), Ordering::Greater);
    // intervals
    assert!(eq_tlc(&ops::interval(1, 0), &Value::empty_set()).unwrap());
    assert!(eq_tlc(&Value::empty_set(), &ops::interval(1, 0)).unwrap());
    assert!(eq_tlc(&ops::interval(2, 1), &ops::interval(5, 3)).unwrap());
    assert!(eq_tlc(&ops::interval(1, 3), &ints(&[1, 2, 3])).unwrap());
    assert!(!eq_tlc(&ops::interval(1, 3), &ints(&[1, 2, 4])).unwrap());
    // lazy errors: sizes compared first
    assert!(!eq_tlc(&ints(&[1]), &set(vec![s("a"), s("b")])).unwrap());
    assert!(eq_tlc(&ints(&[1]), &set(vec![s("a")])).is_err());
    // membership
    assert!(ops::member(&s("a"), &ops::interval(1, 3)).is_err());
    assert!(!ops::member(&s("a"), &ops::interval(3, 1)).unwrap());
    assert!(ops::member(&s("a"), &ints(&[1, 2])).is_err());
    assert!(!ops::member(&s("a"), &Value::empty_set()).unwrap());
    assert!(ops::member(&int(1), &ops::seq_set(&ints(&[1])).unwrap()).is_err());
    let string = Value::LazySet(std::sync::Arc::new(tlx_value::LazySet::StringSet));
    let nat = Value::LazySet(std::sync::Arc::new(tlx_value::LazySet::Nat));
    assert!(ops::member(&int(1), &string).is_err());
    assert!(ops::member(&s("a"), &nat).is_err());
}

#[test]
fn set_normalization_errors_like_tlc() {
    // {<<1>>, [a |-> 1, b |-> 2]}: fine when the tuple is compared against the record
    let t1 = tup(vec![int(1)]);
    let r2 = rec(&[("a", int(1)), ("b", int(2))]);
    assert!(Value::set_from(vec![r2.clone(), t1.clone()]).is_ok());
    // D-31: insertion order does not change the meaning of a function set.
    assert_eq!(Value::set_from(vec![t1.clone(), r2.clone()]).unwrap(), Value::set_from(vec![r2, t1.clone()]).unwrap());
    assert!(Value::set_from(vec![t1, rec(&[("a", int(1))])]).is_ok());
    assert!(Value::set_from(vec![int(1), s("a")]).is_err());
    // large sets go through the total-order path, still erroring
    let mut v: Vec<Value> = (0..100).map(int).collect();
    v.push(s("a"));
    assert!(Value::set_from(v).is_err());
    let mut v: Vec<Value> = (0..100).map(|i| tup(vec![int(i)])).collect();
    v.push(ints(&[1]));
    assert!(Value::set_from(v).is_err());
}

#[test]
fn compound_order() {
    // sets: size first, then elements
    let v = set(vec![ints(&[1, 2]), ints(&[3]), ints(&[]), ints(&[0, 5])]);
    assert_eq!(v.to_string(), "{{}, {3}, {0, 5}, {1, 2}}");
    // tuples: length first
    let v = set(vec![tup(vec![int(5)]), tup(vec![int(1), int(1)]), tup(vec![int(2)])]);
    assert_eq!(v.to_string(), "{<<2>>, <<5>>, <<1, 1>>}");
    // functions: domain size, then whole domain, then values
    let f1 = fcn(vec![(int(0), int(9))]);
    let f2 = fcn(vec![(int(1), int(0))]); // <<0>>
    let f3 = fcn(vec![(int(0), int(1)), (int(5), int(0))]);
    assert_eq!(cmp_tlc(&f1, &f2).unwrap(), Ordering::Less);
    assert_eq!(cmp_tlc(&f2, &f1).unwrap(), Ordering::Greater);
    assert_eq!(cmp_tlc(&f1, &f3).unwrap(), Ordering::Less);
    assert!(f1 < f2 && f1 < f3);
    // records: field names (intern order) before values
    Sym::new("co_x");
    Sym::new("co_y");
    let ra = rec(&[("co_y", int(0))]);
    let rb = rec(&[("co_x", int(9))]);
    assert_eq!(cmp_tlc(&rb, &ra).unwrap(), Ordering::Less);
    assert!(rb < ra);
    // string-domain function vs record compare equal
    let g = fcn(vec![(s("co_x"), int(9))]);
    assert_eq!(cmp_tlc(&g, &rb).unwrap(), Ordering::Equal);
}

#[test]
fn extreme_intervals_order_without_overflow() {
    // i64::MIN..=i64::MAX has cardinality 2^64, which does not fit in i64:
    // `hi - lo + 1` in i64 overflows (panic in debug builds).
    let widest = ops::interval(i64::MIN, i64::MAX);
    let shorter = ops::interval(i64::MIN, i64::MAX - 1);
    assert_eq!(widest.cmp(&shorter), Ordering::Greater);
    assert_eq!(shorter.cmp(&widest), Ordering::Less);
    assert_eq!(widest.cmp(&widest), Ordering::Equal);
    // Same cardinality 2^64 - 1 from the other end: tie broken by lower bound.
    let shifted = ops::interval(i64::MIN + 1, i64::MAX);
    assert_eq!(shorter.cmp(&shifted), Ordering::Less);
    assert_eq!(shifted.cmp(&shorter), Ordering::Greater);
    // Extremes still order against ordinary and empty intervals by size.
    assert_eq!(widest.cmp(&ops::interval(1, 3)), Ordering::Greater);
    assert_eq!(ops::interval(1, 3).cmp(&widest), Ordering::Less);
    let empty = ops::interval(i64::MAX, i64::MIN);
    assert_eq!(empty.cmp(&widest), Ordering::Less);
    assert_eq!(empty.cmp(&Value::empty_set()), Ordering::Equal);
}

#[test]
fn ord_agrees_with_cmp_tlc() {
    let vals = vec![
        mv("m1"),
        mv("m2"),
        Value::FALSE,
        Value::TRUE,
        int(-1),
        int(0),
        int(7),
        s("p"),
        s("q"),
        Value::empty_set(),
        ints(&[1]),
        ints(&[1, 2]),
        ops::interval(1, 2),
        ops::interval(2, 3),
        ops::powerset(&ints(&[1])).unwrap(),
        tup(vec![]),
        tup(vec![int(1)]),
        tup(vec![int(1), int(2)]),
        rec(&[("a", int(1))]),
        rec(&[("a", int(1)), ("b", int(2))]),
        fcn(vec![(int(0), int(1))]),
        fcn(vec![(int(0), int(1)), (int(1), int(1))]),
        scale(ops::interval(1, 2), 1),
    ];
    for a in &vals {
        for b in &vals {
            if let Ok(c) = cmp_tlc(a, b) {
                assert_eq!(a.cmp(b), c, "{a} vs {b}");
            }
            if let Ok(e) = eq_tlc(a, b) {
                assert_eq!(a == b, e, "{a} = {b}");
            }
            assert_eq!(a.cmp(b), b.cmp(a).reverse(), "{a} vs {b}");
        }
    }
}

#[test]
fn representation_independent_eq_hash() {
    let mut h = HashSet::new();
    h.insert(ops::interval(1, 3));
    assert!(h.contains(&ints(&[1, 2, 3])));
    assert!(h.contains(&ops::union(&ints(&[1]), &ints(&[2, 3])).unwrap()));
    let p = ops::powerset(&ints(&[1, 2])).unwrap();
    let pn = p.normalize().unwrap();
    assert!(matches!(pn, Value::Set(_)));
    assert_eq!(p, pn);
    h.insert(p);
    assert!(h.contains(&pn));
    let lf = scale(ops::interval(1, 3), 2);
    let t = tup(vec![int(2), int(4), int(6)]);
    assert_eq!(lf, t);
    h.insert(lf);
    assert!(h.contains(&t));
    assert!(eq_tlc(&t, &scale(ops::interval(1, 3), 2)).unwrap());
    // empty interval == empty set
    assert_eq!(ops::interval(5, 1), Value::empty_set());
    assert_ne!(Value::empty_set(), tup(vec![]));
}

#[test]
fn certainly_equal_never_panics_on_non_enumerable_sets() {
    use std::sync::Arc;
    use tlx_value::order::certainly_equal;
    use tlx_value::LazySet;
    // `[{1, 2} -> Seq(Nat \ 0..1)]`, built twice: `==` (the total order)
    // cannot enumerate it and panics; `certainly_equal` answers false.
    let fs = || {
        let terms = Value::LazySet(Arc::new(LazySet::Diff(Value::LazySet(Arc::new(LazySet::Nat)), Value::Interval(0, 1))));
        Value::LazySet(Arc::new(LazySet::FcnSet(set(vec![int(1), int(2)]), Value::LazySet(Arc::new(LazySet::Seq(terms))))))
    };
    let (a, b) = (fs(), fs());
    assert!(certainly_equal(&a, &a));
    assert!(!certainly_equal(&a, &b));
    assert!(certainly_equal(&int(3), &int(3)));
    assert!(!certainly_equal(&int(3), &s("x")));
}

#[test]
fn function_witnesses_compare_in_both_directions_and_with_self() {
    let functions = [
        tup(vec![int(7)]),
        Value::fcn_rcd_from_pairs(vec![(int(1), int(7))]).unwrap(),
        Value::fcn_over(&ops::interval(1, 1), vec![(int(1), int(7))]).unwrap(),
        rec(&[("matrix_field", int(7))]),
        Value::fcn_rcd_from_pairs(vec![(s("matrix_field"), int(7))]).unwrap(),
        Value::empty_fcn(),
        tup(vec![]),
    ];
    for a in &functions {
        for b in &functions {
            let expected = a.cmp(b);
            assert_eq!(cmp_tlc(a, b).unwrap(), expected, "{a} vs {b}");
            assert_eq!(eq_tlc(a, b).unwrap(), expected == Ordering::Equal, "{a} = {b}");
        }
    }
}

#[test]
fn function_domains_finish_before_fallible_mapped_values() {
    let make = |key, value| Value::fcn_rcd_from_pairs(vec![(int(0), value), (key, int(9))]).unwrap();
    let a = make(int(2), int(0));
    let b = make(int(3), s("mapped_error"));
    // First mapped values are incomparable, but the second domain key wins.
    assert_eq!(cmp_tlc(&a, &b).unwrap(), Ordering::Less);
    assert_eq!(cmp_tlc(&b, &a).unwrap(), Ordering::Greater);
    assert!(!eq_tlc(&a, &b).unwrap());
    assert!(!eq_tlc(&b, &a).unwrap());
    let c = make(int(2), s("mapped_error"));
    assert!(err_msg(cmp_tlc(&a, &c)).contains("integer 0"));
    assert!(err_msg(cmp_tlc(&c, &a)).contains("string"));
    assert!(eq_tlc(&a, &c).is_err());
    assert!(eq_tlc(&c, &a).is_err());
}

#[test]
fn function_domain_kind_matrix_uses_structural_order() {
    let keys = [
        mv("matrix_untyped"),
        mv("x_matrix"),
        Value::TRUE,
        int(2),
        s("matrix_key"),
        ints(&[2]),
        tup(vec![int(2)]),
        rec(&[("matrix_key", int(2))]),
    ];
    for a in &keys {
        for b in &keys {
            let fa = ops::single_fcn(a.clone(), int(0)).unwrap();
            let fb = ops::single_fcn(b.clone(), int(0)).unwrap();
            assert_eq!(cmp_tlc(&fa, &fb).unwrap(), a.cmp(b), "{fa} vs {fb}");
            assert_eq!(eq_tlc(&fa, &fb).unwrap(), a == b, "{fa} = {fb}");
        }
    }
}
