//! TLC-compatible randomness for `RandomElement` (TLC) and the
//! Randomization module (tlc2/value/RandomEnumerableValues.java).
//!
//! TLC draws from a `java.util.Random` seeded with `-seed` (a random seed by
//! default). Constant-level evaluation and Init use one stream, so with the
//! same seed tlx picks exactly what TLC picks there (e.g. a random graph in a
//! constant definition). During model checking TLC reseeds the generator
//! for each explored state with `seed ^ fingerprint(state)`; tlx does the
//! same with its own fingerprints (which differ from TLC's), at the start of
//! each evaluation of a state's successors, so the choices are a function
//! of the state: independent of worker count, exploration order, and
//! re-execution (error traces).
//!
//! Deviation: without `-seed` tlx uses seed 0 (deterministic) instead of a
//! random seed.

use std::cell::RefCell;

use tlx_value::{ops, Value};

use crate::error::{EResult, EvalError};
use crate::eval::Ctx;
use crate::interp::Interp;

/// `java.util.Random` (48-bit linear congruential generator).
#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: u64,
}

const MULTIPLIER: u64 = 0x5_DEEC_E66D;
const MASK: u64 = (1 << 48) - 1;

impl JavaRandom {
    pub fn new(seed: i64) -> JavaRandom {
        JavaRandom { seed: (seed as u64 ^ MULTIPLIER) & MASK }
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(MULTIPLIER).wrapping_add(0xB)) & MASK;
        (self.seed >> (48 - bits)) as i64 as i32
    }

    /// `nextInt(bound)` (bound > 0).
    pub fn next_int(&mut self, bound: i32) -> i32 {
        if bound & -bound == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    pub fn next_double(&mut self) -> f64 {
        let hi = (self.next(26) as i64) << 27;
        let lo = self.next(27) as i64;
        (hi + lo) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// `floor(nextDouble() * n)`: TLC's random index into a set of size n.
    pub fn index(&mut self, n: usize) -> usize {
        ((self.next_double() * n as f64).floor() as usize).min(n.saturating_sub(1))
    }
}

/// The generators of one interpreter (`Interp::random`).
pub(crate) struct Randomness {
    seed: i64,
    /// The constant-level stream (TLC's main thread).
    global: JavaRandom,
    /// The Init stream: the constant-level stream when Init is first
    /// evaluated (TLC continues the main thread's stream).
    init: Option<JavaRandom>,
}

impl Randomness {
    pub(crate) fn new(seed: i64) -> Randomness {
        Randomness { seed, global: JavaRandom::new(seed), init: None }
    }
}

/// Which stream a thread's generator currently draws from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Init,
    State(u64),
}

thread_local! {
    /// (interpreter uid, stream, generator) of this thread.
    static SCOPE: RefCell<Option<(u64, Key, JavaRandom)>> = const { RefCell::new(None) };
}

impl Interp {
    /// Set the seed (`-seed`; also reported by `TLCGet("config")`).
    pub fn set_seed(&self, seed: u64) {
        *self.random.lock().unwrap() = Randomness::new(seed as i64);
    }

    pub(crate) fn seed(&self) -> i64 {
        self.random.lock().unwrap().seed
    }

    /// Start a new evaluation of Init or of the successors of a state: the
    /// next random choice of this thread restarts the stream of that state.
    pub fn begin_random_scope() {
        SCOPE.with(|s| *s.borrow_mut() = None);
    }

    /// Run `f` with the generator for context `cx`.
    pub(crate) fn with_random<T>(&self, cx: &Ctx, f: impl FnOnce(&mut JavaRandom) -> T) -> EResult<T> {
        let key = if let Some(s0) = cx.s0 {
            Key::State(tlx_value::fingerprint::fingerprint(&Value::tuple(s0.to_vec()))?)
        } else if cx.init {
            Key::Init
        } else {
            // Constant level: the shared stream.
            return Ok(f(&mut self.random.lock().unwrap().global));
        };
        SCOPE.with(|s| {
            let mut s = s.borrow_mut();
            let fresh = !matches!(&*s, Some((uid, k, _)) if *uid == self.uid && *k == key);
            if fresh {
                let mut r = self.random.lock().unwrap();
                let rng = match key {
                    Key::State(fp) => JavaRandom::new(r.seed ^ fp as i64),
                    Key::Init => {
                        let g = r.global.clone();
                        r.init.get_or_insert(g).clone()
                    }
                };
                *s = Some((self.uid, key, rng));
            }
            Ok(f(&mut s.as_mut().unwrap().2))
        })
    }

    /// `RandomElement(S)` (TLC.java): a uniformly chosen element of S in
    /// normalized order; for sets of functions, records and tuples, a
    /// random element of each component.
    pub(crate) fn random_element(&self, s: &Value, cx: &Ctx) -> EResult<Value> {
        use tlx_value::LazySet;
        if let Value::LazySet(l) = s {
            match &**l {
                LazySet::FcnSet(dom, range) => {
                    let mut pairs = Vec::new();
                    for d in ops::enumerate_normalized(dom)? {
                        pairs.push((d, self.random_element(range, cx)?));
                    }
                    return Ok(Value::fcn_over(dom, pairs)?);
                }
                LazySet::RecordSet(fields) => {
                    let mut out = Vec::new();
                    for (n, set) in fields {
                        out.push((*n, self.random_element(set, cx)?));
                    }
                    return Ok(Value::record(out));
                }
                LazySet::Product(sets) => {
                    let elems = sets.iter().map(|t| self.random_element(t, cx)).collect::<EResult<Vec<_>>>()?;
                    return Ok(Value::tuple(elems));
                }
                _ => {}
            }
        }
        let elems = ops::enumerate_normalized(s)?;
        if elems.is_empty() {
            return Err(EvalError::new("Attempted to compute RandomElement of the empty set."));
        }
        let i = self.with_random(cx, |r| r.index(elems.len()))?;
        Ok(elems[i].clone())
    }

    /// `RandomSubset(k, S)` (Randomization.java): k distinct elements chosen
    /// by TLC's linear congruential index generator (all of S if smaller).
    pub(crate) fn random_subset(&self, k: i64, s: &Value, cx: &Ctx) -> EResult<Value> {
        // (SetOfFcnsOrRcdsValue.elements(k): an empty set decides before
        // its constituents are enumerated.)
        if matches!(s, Value::LazySet(_)) && ops::is_empty_checked(s)? {
            return Ok(Value::empty_set());
        }
        if let Some(ix) = Indexed::of(s)? {
            // (TLC: IntervalValue / SetOfFcnsOrRcdsValue pick by index,
            // without enumerating the set.)
            if ix.len == 0 || k <= 0 {
                return Ok(Value::empty_set());
            }
            let picks = if ix.len <= i32::MAX as u128 {
                self.with_random(cx, |r| {
                    SubsetEnumerator::new(ix.len as i32, r).indices(k as usize).into_iter().map(|i| i as u128).collect()
                })?
            } else {
                // (TLC's BigIntegerSubsetEnumerator: k distinct indices of
                // a set with more than 2^31 - 1 elements; tlx draws them
                // differently.)
                self.with_random(cx, |r| distinct_indices(ix.len, k as usize, r))?
            };
            return Ok(Value::set_from(picks.into_iter().map(|i| ix.element(i)).collect::<EResult<Vec<_>>>()?)?);
        }
        let elems = ops::enumerate_normalized(s)?;
        let n = elems.len();
        if n == 0 || k <= 0 {
            return Ok(Value::empty_set());
        }
        let picks = self.with_random(cx, |r| SubsetEnumerator::new(n as i32, r).indices(k as usize))?;
        Ok(Value::set_from(picks.into_iter().map(|i| elems[i].clone()).collect())?)
    }

    /// `RandomSetOfSubsets(k, n, S)` (SubsetValue.CoinTossingSubsetEnumerator):
    /// k coin-tossed subsets of S, each element in with probability n/|S|.
    pub(crate) fn random_set_of_subsets(&self, k: i64, n: i64, s: &Value, cx: &Ctx) -> EResult<Value> {
        let elems = ops::enumerate_normalized(s)?;
        let p = if elems.is_empty() { 0.0 } else { n as f64 / elems.len() as f64 };
        let subsets = self.with_random(cx, |r| {
            (0..k).map(|_| elems.iter().filter(|_| r.next_double() < p).cloned().collect::<Vec<_>>()).collect::<Vec<_>>()
        })?;
        Ok(Value::set_from(subsets.into_iter().map(Value::set_from).collect::<Result<Vec<_>, _>>()?)?)
    }
}

/// A finite set whose element of a given index in TLC's normalized order is
/// computed without enumerating the set: an interval, or a product, set of
/// functions or set of records over enumerable sets (in normalized order,
/// their elements are those of a mixed radix: the first component, the
/// function's value at the least domain element, the first field are the
/// most significant digits).
struct Indexed {
    len: u128,
    kind: IndexedKind,
}

enum IndexedKind {
    Interval(i64),
    /// The normalized elements of each digit's set, most significant first,
    /// and how a choice of one element per digit makes an element.
    Mixed(Vec<Vec<Value>>, Shape),
}

enum Shape {
    Tuple,
    /// A function over this domain (normalized elements, one per digit).
    Fcn(Value, Vec<Value>),
    Record(Vec<tlx_value::Sym>),
}

impl Indexed {
    fn of(s: &Value) -> EResult<Option<Indexed>> {
        use tlx_value::LazySet;
        let digits = |sets: &[&Value]| -> EResult<Option<(Vec<Vec<Value>>, u128)>> {
            let mut out = Vec::with_capacity(sets.len());
            let mut len: u128 = 1;
            for t in sets {
                if !ops::is_enumerable(t) || !ops::is_finite_set(t)? {
                    return Ok(None);
                }
                let e = ops::enumerate_normalized(t)?;
                len = len.saturating_mul(e.len() as u128);
                out.push(e);
            }
            Ok(Some((out, len)))
        };
        Ok(match s {
            Value::Interval(lo, hi) => {
                Some(Indexed { len: if hi < lo { 0 } else { (*hi as i128 - *lo as i128 + 1) as u128 }, kind: IndexedKind::Interval(*lo) })
            }
            Value::LazySet(l) => match &**l {
                LazySet::Product(sets) => {
                    digits(&sets.iter().collect::<Vec<_>>())?.map(|(d, len)| Indexed { len, kind: IndexedKind::Mixed(d, Shape::Tuple) })
                }
                LazySet::RecordSet(fields) => digits(&fields.iter().map(|f| &f.1).collect::<Vec<_>>())?
                    .map(|(d, len)| Indexed { len, kind: IndexedKind::Mixed(d, Shape::Record(fields.iter().map(|f| f.0).collect())) }),
                LazySet::FcnSet(dom, range) => {
                    if !ops::is_enumerable(dom) || !ops::is_finite_set(dom)? {
                        return Ok(None);
                    }
                    let keys = ops::enumerate_normalized(dom)?;
                    let sets: Vec<&Value> = keys.iter().map(|_| range).collect();
                    digits(&sets)?.map(|(d, len)| Indexed { len, kind: IndexedKind::Mixed(d, Shape::Fcn(dom.clone(), keys.clone())) })
                }
                _ => None,
            },
            _ => None,
        })
    }

    /// The element of index `i` (`i < len`).
    fn element(&self, mut i: u128) -> EResult<Value> {
        match &self.kind {
            IndexedKind::Interval(lo) => Ok(Value::Int((*lo as i128 + i as i128) as i64)),
            IndexedKind::Mixed(digits, shape) => {
                let mut picks = vec![Value::FALSE; digits.len()];
                for (p, d) in picks.iter_mut().zip(digits).rev() {
                    let n = d.len() as u128;
                    *p = d[(i % n) as usize].clone();
                    i /= n;
                }
                Ok(match shape {
                    Shape::Tuple => Value::tuple(picks),
                    Shape::Record(names) => Value::record(names.iter().copied().zip(picks).collect()),
                    Shape::Fcn(dom, keys) => Value::fcn_over(dom, keys.iter().cloned().zip(picks).collect())?,
                })
            }
        }
    }
}

/// `k` distinct indices below `n` (`n` greater than `k`).
fn distinct_indices(n: u128, k: usize, r: &mut JavaRandom) -> Vec<u128> {
    let mut seen = std::collections::HashSet::with_capacity(k);
    let mut out = Vec::with_capacity(k);
    while out.len() < k {
        let i = ((r.next_double() * n as f64) as u128).min(n - 1);
        if seen.insert(i) {
            out.push(i);
        }
    }
    out
}

/// TLC's `EnumerableValue.SubsetEnumerator`: a full-period linear
/// congruential generator over 0..m (m >= n) with a random start and a
/// random prime increment.
struct SubsetEnumerator {
    n: i64,
    index: i64,
    a: i64,
    m: i64,
    c: i64,
}

impl SubsetEnumerator {
    fn new(n: i32, r: &mut JavaRandom) -> SubsetEnumerator {
        let (m, a) = optimal_m_and_a(n as i64);
        let index = r.next_int(n) as i64;
        let c = next_prime(r);
        SubsetEnumerator { n: n as i64, index, a, m, c }
    }

    fn indices(mut self, k: usize) -> Vec<usize> {
        (0..k)
            .map(|_| {
                loop {
                    self.index = (self.a * self.index + self.c) % self.m;
                    if self.index < self.n {
                        break;
                    }
                }
                self.index as usize
            })
            .collect()
    }
}

/// The prime factors of n with multiplicity.
fn prime_factors(mut n: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let mut p = 2;
    while p * p <= n {
        while n % p == 0 {
            out.push(p);
            n /= p;
        }
        p += 1;
    }
    if n > 1 {
        out.push(n);
    }
    out
}

/// TLC's `computeOptimalMandA`: the smallest m >= max(n, 9) that is not
/// divisible by 4 and not square-free, and a = (product of its distinct
/// prime factors) + 1.
fn optimal_m_and_a(n: i64) -> (i64, i64) {
    let mut m = n.max(9);
    loop {
        let f = prime_factors(m);
        let mut distinct = f.clone();
        distinct.dedup();
        if m % 4 != 0 && distinct.len() != f.len() {
            return (m, distinct.iter().product::<i64>() + 1);
        }
        m += 1;
    }
}

/// TLC's `RandomGenerator.primes`: the 1341 primes in [1277011, 1295953].
fn primes() -> &'static [i64] {
    static PRIMES: std::sync::OnceLock<Vec<i64>> = std::sync::OnceLock::new();
    PRIMES.get_or_init(|| (1_277_011..=1_295_953).filter(|&n| prime_factors(n).len() == 1).collect())
}

/// TLC's `RandomGenerator.nextPrime(Random)`.
fn next_prime(r: &mut JavaRandom) -> i64 {
    let ps = primes();
    let mut index = ps.len();
    while index == ps.len() {
        index = (r.next_double() * index as f64).floor() as usize;
    }
    ps[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches_java() {
        // new java.util.Random(42): nextInt(), nextInt(10), nextDouble().
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next(32), -1170105035);
        assert_eq!(r.next_int(10), 3);
        let mut r = JavaRandom::new(0);
        assert_eq!(r.next_double(), 0.730967787376657);
        let mut r = JavaRandom::new(-7);
        assert_eq!((r.next_int(1000), r.next_int(7), r.next_double()), (662, 0, 0.6731811902798038));
    }

    #[test]
    fn tlc_primes() {
        let ps = primes();
        assert_eq!((ps.len(), ps[0], ps[ps.len() - 1]), (1341, 1277011, 1295953));
    }

    #[test]
    fn optimal_parameters() {
        // m >= 9, not divisible by 4, with a repeated prime factor.
        assert_eq!(optimal_m_and_a(3), (9, 4));
        assert_eq!(optimal_m_and_a(10), (18, 7));
        assert_eq!(optimal_m_and_a(20), (25, 6));
    }
}
