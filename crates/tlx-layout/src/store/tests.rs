//! Differential tests of the store against the interpreter's values and
//! operations (`tlx_value`), on random finite values with aliases.

use std::cell::Cell;
use std::sync::Arc;

use tlx_base::rng::SplitMix64;
use tlx_value::{ops, order, FcnDom, Sym, Value};

use super::*;

thread_local! {
    static TINY: Cell<bool> = const { Cell::new(false) };
}

/// Under `with_tiny_hashes`, exact hashes keep 3 bits: nearly every
/// interning compares structurally against colliding nodes.
pub(super) fn tiny_hash(h: u64) -> u64 {
    if TINY.with(|t| t.get()) {
        h & (7 << 61)
    } else {
        h
    }
}

fn with_tiny_hashes<T>(f: impl FnOnce() -> T) -> T {
    TINY.with(|t| t.set(true));
    let r = f();
    TINY.with(|t| t.set(false));
    r
}

struct Gen(SplitMix64);

impl Gen {
    fn new(seed: u64) -> Gen {
        Gen(SplitMix64::new(seed))
    }
    fn below(&mut self, n: usize) -> usize {
        self.0.below(n)
    }
    fn scalar(&mut self) -> Value {
        match self.below(9) {
            0 => Value::Bool(self.below(2) == 1),
            1..=3 => Value::Int(self.below(5) as i64),
            4 => Value::Int((1i64 << 61) + self.below(2) as i64),
            5 | 6 => Value::str(["a", "b", "c"][self.below(3)]),
            _ => Value::model_value(["m1", "m2"][self.below(2)]),
        }
    }
    /// A random finite value of depth at most `d` (sets of comparable
    /// elements: one kind per set).
    fn value(&mut self, d: u32) -> Value {
        if d == 0 {
            return self.scalar();
        }
        match self.below(10) {
            0 | 1 => self.scalar(),
            2 => {
                let lo = self.below(3) as i64;
                Value::Interval(lo, lo + self.below(4) as i64 - 1)
            }
            3 => {
                let kind = self.below(3);
                let es: Vec<Value> = (0..self.below(4))
                    .map(|_| match kind {
                        0 => Value::Int(self.below(6) as i64),
                        1 => Value::str(["a", "b", "c", "d"][self.below(4)]),
                        _ => Value::tuple((0..self.below(2)).map(|_| Value::Int(self.below(3) as i64)).collect()),
                    })
                    .collect();
                Value::set_from(es).unwrap()
            }
            4 => Value::tuple((0..self.below(4)).map(|_| self.value(d - 1)).collect()),
            5 => {
                let names = ["a", "b", "c"];
                let fs: Vec<(Sym, Value)> = (0..1 + self.below(3)).map(|i| (Sym::new(names[i]), self.value(d - 1))).collect();
                Value::record(fs)
            }
            6 => {
                // A function over an enumerated domain of comparable keys.
                let n = self.below(4);
                let keys: Vec<Value> = match self.below(3) {
                    0 => (0..n).map(|i| Value::Int(i as i64 + self.below(2) as i64)).collect(),
                    1 => (0..n).map(|i| Value::str(["a", "b", "c", "d"][i])).collect(),
                    _ => (0..n).map(|i| Value::record(vec![(Sym::new("k"), Value::Int(i as i64))])).collect(),
                };
                let mut keys = keys;
                keys.dedup();
                let pairs = keys.into_iter().map(|k| (k, self.value(d - 1))).collect();
                Value::fcn_rcd_from_pairs(pairs).unwrap()
            }
            7 => {
                let lo = self.below(3) as i64;
                let n = self.below(3) as i64;
                let pairs = (lo..lo + n).map(|k| (Value::Int(k), self.value(d - 1))).collect();
                Value::fcn_over(&Value::Interval(lo, lo + n - 1), pairs).unwrap()
            }
            _ => {
                // An alias: a tuple or record as a function over an
                // enumerated domain.
                let v = self.value(d - 1);
                match v {
                    Value::Tuple(_) | Value::Record(_) => v.into_fcn_rcd(),
                    v => v,
                }
            }
        }
    }
}

/// Equality of representations: the same variants, domains and children.
fn exact(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Set(x), Value::Set(y)) | (Value::Tuple(x), Value::Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| exact(p, q))
        }
        (Value::Record(x), Value::Record(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| p.0 == q.0 && exact(&p.1, &q.1)),
        (Value::Fcn(x, d), Value::Fcn(y, e)) => {
            d == e && x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| exact(&p.0, &q.0) && exact(&p.1, &q.1))
        }
        (Value::Interval(a, b), Value::Interval(c, d)) => a == c && b == d,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Str(x), Value::Str(y)) | (Value::ModelValue(x), Value::ModelValue(y)) => x == y,
        _ => false,
    }
}

fn values(seed: u64, n: usize) -> Vec<Value> {
    let mut g = Gen::new(seed);
    let mut vs: Vec<Value> = (0..n).map(|_| g.value(3)).collect();
    // The alias pairs the contract names, explicitly.
    let one_two = Value::tuple(vec![Value::Int(1), Value::Int(2)]);
    vs.push(one_two.clone());
    vs.push(
        Value::fcn_over(
            &Value::set_from(vec![Value::Int(1), Value::Int(2)]).unwrap(),
            vec![(Value::Int(1), Value::Int(1)), (Value::Int(2), Value::Int(2))],
        )
        .unwrap(),
    );
    vs.push(one_two.into_fcn_rcd());
    let rec = Value::record(vec![(Sym::new("a"), Value::Int(1))]);
    vs.push(rec.clone());
    vs.push(rec.into_fcn_rcd());
    vs.push(Value::tuple(vec![]));
    vs.push(Value::empty_fcn());
    vs.push(Value::empty_set());
    vs.push(Value::Interval(1, 0));
    vs.push(Value::Interval(1, 3));
    vs.push(Value::set_from(vec![Value::Int(1), Value::Int(2), Value::Int(3)]).unwrap());
    vs
}

#[test]
fn freeze_thaw_is_exact_and_interning_is_by_representation() {
    for tiny in [false, true] {
        let run = || {
            let s = Store::new(usize::MAX);
            let vs = values(1, 400);
            let hs: Vec<u64> = vs.iter().map(|v| s.freeze(v).unwrap()).collect();
            for (v, h) in vs.iter().zip(&hs) {
                assert!(exact(&s.thaw(*h), v), "{v} thawed as {}", s.thaw(*h));
                assert_eq!(s.freeze(v).unwrap(), *h);
                assert!(s.is_valid(*h));
            }
            for (i, a) in vs.iter().enumerate() {
                for (j, b) in vs.iter().enumerate() {
                    assert_eq!(hs[i] == hs[j], exact(a, b), "{a} / {b}");
                }
            }
        };
        if tiny {
            with_tiny_hashes(run)
        } else {
            run()
        }
    }
}

/// Digests follow the interpreter's equality, aliases included, and so
/// does `same_value`.
#[test]
fn digests_follow_value_equality() {
    let s = Store::new(usize::MAX);
    let vs = values(2, 400);
    let hs: Vec<u64> = vs.iter().map(|v| s.freeze(v).unwrap()).collect();
    for (i, a) in vs.iter().enumerate() {
        for (j, b) in vs.iter().enumerate() {
            let eq = a == b;
            assert_eq!(s.digest(hs[i]) == s.digest(hs[j]), eq, "{a} / {b}");
            assert_eq!(s.same_value(hs[i], hs[j]), eq, "{a} / {b}");
        }
        assert_eq!(digest_of_value(a).unwrap(), s.digest(hs[i]));
    }
    // A digest does not depend on the store or on the order of interning.
    let t = Store::new(usize::MAX);
    for (v, h) in vs.iter().zip(&hs).rev() {
        assert_eq!(t.digest(t.freeze(v).unwrap()), s.digest(*h));
    }
}

/// The comparisons agree with `tlx_value::order` exactly (results and
/// errors), on every pair.
#[test]
fn comparisons_match_the_interpreter() {
    let s = Store::new(usize::MAX);
    let mut vs = values(3, 250);
    vs.push(Value::model_value("m_typed"));
    vs.push(Value::model_value("n_other"));
    let hs: Vec<u64> = vs.iter().map(|v| s.freeze(v).unwrap()).collect();
    for (i, a) in vs.iter().enumerate() {
        for (j, b) in vs.iter().enumerate() {
            let want = order::eq_tlc(a, b).ok();
            assert_eq!(eq_tlc(&s, hs[i], hs[j]).ok(), want, "{a} = {b}");
            let want = order::cmp_tlc(a, b).ok();
            assert_eq!(cmp_tlc(&s, hs[i], hs[j]).ok(), want, "compare {a} with {b}");
        }
    }
}

/// `got` (a store operation) against `want` (the interpreter's): equal
/// representations on success; an interpreter error is a store error. A
/// store error where the interpreter succeeds is counted (a fallback).
fn check(s: &Store, what: &str, got: Result<u64, RefOpError>, want: tlx_value::Result<Value>, fallbacks: &mut usize) {
    match (got, want) {
        (Ok(h), Ok(v)) => assert!(exact(&s.thaw(h), &v), "{what}: store {} interpreter {v}", s.thaw(h)),
        (Err(_), Err(_)) => {}
        (Ok(h), Err(e)) => panic!("{what}: store {} where the interpreter fails: {e}", s.thaw(h)),
        (Err(_), Ok(v)) => {
            eprintln!("FALLBACK {what} -> {v}");
            *fallbacks += 1
        }
    }
}

#[test]
fn operations_match_the_interpreter() {
    let s = Store::new(usize::MAX);
    let vs = values(4, 160);
    let hs: Vec<u64> = vs.iter().map(|v| s.freeze(v).unwrap()).collect();
    let mut fallbacks = 0;
    let int = |b: bool| s.freeze(&Value::Bool(b)).unwrap();
    for (f, hf) in vs.iter().zip(&hs) {
        check(&s, &format!("DOMAIN {f}"), s.domain(*hf), ops::domain(f), &mut fallbacks);
        check(&s, &format!("Len({f})"), s.len(*hf).map(|n| s.freeze(&Value::Int(n)).unwrap()), ops::len(f).map(Value::Int), &mut fallbacks);
        check(&s, &format!("Head({f})"), s.head(*hf), ops::head(f), &mut fallbacks);
        // (Tail of a string makes a string: left to the interpreter, which
        // keeps the strings' creation order.)
        if !matches!(f, Value::Str(_)) {
            check(&s, &format!("Tail({f})"), s.tail(*hf), ops::tail(f), &mut fallbacks);
        }
        if f.is_set() {
            check(
                &s,
                &format!("Cardinality({f})"),
                s.cardinality(*hf).map(|n| s.freeze(&Value::Int(n)).unwrap()),
                ops::cardinality(f).map(Value::Int),
                &mut fallbacks,
            );
        }
        for (k, hk) in vs.iter().zip(&hs).step_by(3) {
            check(&s, &format!("{f}[{k}]"), s.apply(*hf, *hk), ops::apply(f, k), &mut fallbacks);
            let sel = s.select(*hf, *hk);
            match (sel, ops::select(f, k)) {
                (Ok(Some(i)), Ok(Some(v))) => assert!(exact(&s.thaw(s.value_at(*hf, i)), &v)),
                (Ok(None), Ok(None)) | (Err(_), Err(_)) | (Err(_), Ok(_)) => {}
                (got, want) => panic!("select {f} [{k}]: {got:?} / {want:?}"),
            }
            // EXCEPT ![k] = TRUE (after the select).
            if let (Ok(Some(i)), Ok(Some(_))) = (s.select(*hf, *hk), ops::select(f, k)) {
                check(
                    &s,
                    &format!("[{f} EXCEPT ![{k}]]"),
                    s.set_at(*hf, i, int(true)),
                    ops::except(f, k, Value::Bool(true)),
                    &mut fallbacks,
                );
            }
            check(&s, &format!("{k} :> {f}"), s.single(*hk, *hf), ops::single_fcn(k.clone(), f.clone()), &mut fallbacks);
            check(&s, &format!("{f} @@ {k}"), s.merge(*hf, *hk), ops::fcn_merge(f, k), &mut fallbacks);
            check(&s, &format!("Append({f}, {k})"), s.append(*hf, *hk), ops::append(f, k.clone()), &mut fallbacks);
            let as_bool = |r: Result<bool, RefOpError>| r.map(int);
            check(&s, &format!("{k} \\in {f}"), as_bool(s.member(*hk, *hf)), ops::member(k, f).map(Value::Bool), &mut fallbacks);
            let dom = ops::domain(f).and_then(|d| ops::member(k, &d));
            check(&s, &format!("{k} \\in DOMAIN {f}"), as_bool(s.in_domain(*hk, *hf)), dom.map(Value::Bool), &mut fallbacks);
        }
    }
    assert_eq!(fallbacks, 0, "operations left to the interpreter");
}

/// Functions of 31/32/33 and 255/256/257 entries (TLC switches from a
/// linear to a binary key search at 32): lookups of present, absent and
/// ill-typed keys, updates and merges agree with the interpreter.
#[test]
fn growth_across_the_search_schedules() {
    let s = Store::new(usize::MAX);
    let mut fallbacks = 0;
    for n in [31usize, 32, 33, 255, 256, 257] {
        let keysets: [Vec<Value>; 3] = [
            (0..n)
                .map(|i| {
                    Value::record(vec![
                        (Sym::new("id"), Value::Int(i as i64)),
                        (Sym::new("seq"), Value::tuple(vec![Value::Int(i as i64 % 3)])),
                    ])
                })
                .collect(),
            (0..n).map(|i| Value::Int(2 * i as i64)).collect(),
            (0..n).map(|i| Value::str(&format!("k{i:03}"))).collect(),
        ];
        for keys in keysets {
            let f = Value::fcn_rcd_from_pairs(keys.iter().map(|k| (k.clone(), Value::Int(0))).collect()).unwrap();
            let hf = s.freeze(&f).unwrap();
            let mut probes: Vec<Value> = vec![keys[0].clone(), keys[n / 2].clone(), keys[n - 1].clone()];
            probes.extend([Value::Int(1), Value::Int(-1), Value::model_value("m1"), Value::str("zz"), Value::empty_set()]);
            probes.push(Value::record(vec![(Sym::new("id"), Value::Int(n as i64))]));
            for k in &probes {
                let hk = s.freeze(k).unwrap();
                check(&s, &format!("n={n} f[{k}]"), s.apply(hf, hk), ops::apply(&f, k), &mut fallbacks);
                let g = Value::fcn_rcd_from_pairs(vec![(k.clone(), Value::Int(1))]);
                if let Ok(g) = g {
                    let hg = s.freeze(&g).unwrap();
                    check(&s, &format!("n={n} f @@ ({k} :> 1)"), s.merge(hf, hg), ops::fcn_merge(&f, &g), &mut fallbacks);
                    check(&s, &format!("n={n} ({k} :> 1) @@ f"), s.merge(hg, hf), ops::fcn_merge(&g, &f), &mut fallbacks);
                }
                if let (Ok(Some(i)), Ok(Some(_))) = (s.select(hf, hk), ops::select(&f, k)) {
                    let one = s.freeze(&Value::Int(1)).unwrap();
                    check(&s, &format!("n={n} EXCEPT ![{k}]"), s.set_at(hf, i, one), ops::except(&f, k, Value::Int(1)), &mut fallbacks);
                }
                let dom = ops::domain(&f).and_then(|d| ops::member(k, &d));
                let got = s.in_domain(hk, hf).map(|b| s.freeze(&Value::Bool(b)).unwrap());
                check(&s, &format!("n={n} {k} \\in DOMAIN f"), got, dom.map(Value::Bool), &mut fallbacks);
            }
        }
    }
    assert_eq!(fallbacks, 0);
}

/// An operation that fails part-way (incomparable keys in `@@`) publishes
/// nothing; the store keeps working.
#[test]
fn failed_operations_publish_nothing() {
    let s = Store::new(usize::MAX);
    let f = s.freeze(&Value::fcn_rcd_from_pairs(vec![(Value::Int(1), Value::Int(0)), (Value::Int(5), Value::Int(0))]).unwrap()).unwrap();
    let g = s.freeze(&Value::fcn_rcd_from_pairs(vec![(Value::str("x"), Value::Int(0))]).unwrap()).unwrap();
    let before = s.stats().nodes;
    assert_eq!(s.merge(f, g), Err(RefOpError::Eval));
    assert_eq!(s.stats().nodes, before);
    assert!(s.merge(f, f).is_ok());
}

#[test]
fn the_budget_is_a_hard_limit() {
    let s = Store::new(4096);
    let big = Value::tuple((0..10_000).map(Value::Int).collect());
    assert_eq!(s.freeze(&big), Err(StoreError::Full));
    assert!(s.is_full());
    assert!(s.stats().node_bytes <= 4096);
}

#[test]
fn handles_of_another_store_are_rejected() {
    let (a, b) = (Store::new(usize::MAX), Store::new(usize::MAX));
    let h = a.freeze(&Value::tuple(vec![Value::Int(1)])).unwrap();
    assert!(a.node(h).is_some());
    assert!(b.node(h).is_none());
    assert!(!b.is_valid(h));
    assert!(b.try_thaw(h).is_none());
    assert!(a.try_thaw(h ^ (1 << 40)).is_none());
    assert!(!a.is_valid(0));
}

#[test]
fn store_ids_never_wrap_or_reuse() {
    let next = AtomicU64::new((1 << STORE_BITS) - 1);
    assert_eq!(allocate_uid(&next), (1 << STORE_BITS) - 1);
    for _ in 0..2 {
        assert!(std::panic::catch_unwind(|| allocate_uid(&next)).is_err());
        assert_eq!(next.load(Ordering::Relaxed), 1 << STORE_BITS);
    }
}

/// `Shape::Ref` is one shape per store: shapes of one store are equal (and
/// hash alike), of two stores differ, and print alike (generated code does
/// not depend on the store); a layout mixing two stores is refused.
#[test]
fn shapes_identify_their_store() {
    use std::hash::{BuildHasher, BuildHasherDefault};
    let (a, b) = (StoreRef::new(usize::MAX), StoreRef::new(usize::MAX));
    let hash = |s: &crate::Shape| BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default().hash_one(s);
    let (ra, ra2, rb) = (crate::Shape::Ref(a.clone()), crate::Shape::Ref(a.clone()), crate::Shape::Ref(b.clone()));
    assert_eq!(ra, ra2);
    assert_eq!(hash(&ra), hash(&ra2));
    assert_ne!(ra, rb);
    assert_eq!(format!("{ra:?}"), format!("{rb:?}"));
    assert_ne!(a.uid(), b.uid());
    let pair = |x: &crate::Shape, y: &crate::Shape| crate::Shape::Tuple(vec![x.clone(), y.clone()].into());
    assert_eq!(crate::layout::store_of([&pair(&crate::Shape::Bool, &ra)]), Some(a.clone()));
    assert_eq!(crate::layout::store_of([&ra, &ra2]), Some(a.clone()));
    assert_eq!(crate::layout::store_of([&crate::Shape::Bool]), None);
    let mixed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::layout::store_of([&pair(&ra, &rb)])));
    assert!(mixed.is_err(), "a layout of two stores");
}

#[test]
fn concurrent_interning_agrees() {
    let s = Store::new(usize::MAX);
    let vs = Arc::new(values(5, 200));
    let all: Vec<Vec<u64>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..8)
            .map(|t| {
                let (s, vs) = (&s, vs.clone());
                sc.spawn(move || {
                    let mut out: Vec<u64> = vec![0; vs.len()];
                    for i in 0..vs.len() {
                        let j = (i + t * 37) % vs.len();
                        out[j] = s.freeze(&vs[j]).unwrap();
                    }
                    out
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(all.windows(2).all(|w| w[0] == w[1]));
    for (v, h) in vs.iter().zip(&all[0]) {
        assert!(exact(&s.thaw(*h), v));
    }
}

#[test]
fn intervals_and_big_integers_keep_their_representation() {
    let s = Store::new(usize::MAX);
    let iv = s.freeze(&Value::Interval(2, 4)).unwrap();
    let set = s.freeze(&Value::set_from(vec![Value::Int(2), Value::Int(3), Value::Int(4)]).unwrap()).unwrap();
    assert_ne!(iv, set);
    assert_eq!(s.digest(iv), s.digest(set));
    assert_eq!(s.set_elems(iv).unwrap(), s.set_elems(set).unwrap());
    let big = s.freeze(&Value::Int(1 << 62)).unwrap();
    assert!(matches!(s.thaw(big), Value::Int(x) if x == 1 << 62));
    assert_eq!(s.member(big, iv), Ok(false));
    let f = s.freeze(&Value::Fcn(Arc::from(vec![(Value::Int(0), Value::Int(1))]), FcnDom::Interval)).unwrap();
    assert_eq!(s.select(f, s.freeze(&Value::str("a")).unwrap()), Err(RefOpError::Eval));
}
