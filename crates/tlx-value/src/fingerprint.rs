//! 64-bit fingerprints. Semantically equal values (any representation) have
//! equal fingerprints. Not required to match TLC's FP64 numerically.
//!
//! A value is serialized into a self-delimiting stream of 64-bit words
//! (tag, length, elements in normalized order, ...) which is fed into
//! `tlx_base::hash::FoldHasher`, a two-lane folded-multiply hash (128 bits of
//! internal state, 64-bit output) finalized by the shared
//! `tlx_base::hash::fmix64`.
//! Strings and model values contribute a precomputed 64-bit hash of their
//! text, so the fingerprint of a scalar does not depend on intern order.
//! Compound values do: record fields, set elements and function domains are
//! streamed in their stored (normalized) order, which for strings is `Sym`
//! order, the first-intern order (TLC's string order). Within one process
//! that order never changes, so equal values have equal fingerprints; a
//! fingerprint is only comparable with one of another process that has the
//! same intern order (recover restores it, `intern::restore`; see
//! tests/sym_order.rs). No allocation happens for scalars, tuples, records
//! or enumerated sets; lazy values are materialized first.

use std::hash::Hasher;

use tlx_base::hash::FoldHasher;

use crate::error::err;
use crate::repr::{builtin, force_fcn, set_elems, Builtin};
use crate::value::{FcnKind, Value};

const TAG_BOOL: u64 = 0x8c3a_5f1e_29d4_b701;
const TAG_INT: u64 = 0x1f6b_92c4_e85a_3d17;
const TAG_STR: u64 = 0x5d29_e7a1_43bc_f80b;
const TAG_MV: u64 = 0xa476_1d3e_9f02_c5b9;
const TAG_SET: u64 = 0x3e98_c06b_7d51_a24f;
const TAG_TUPLE: u64 = 0xd1c5_4a87_0e3f_96b3;
const TAG_RECORD: u64 = 0x6a0f_b39d_c218_e475;
const TAG_FCN: u64 = 0xf25e_7c80_b46a_1d99;
const TAG_BUILTIN: u64 = 0x4b81_d6f7_250c_e93d;

/// Receiver of the canonical word stream of a value.
pub(crate) trait Sink {
    fn word(&mut self, w: u64);

    /// `walk` met a lazy value (whose elements it then streams).
    #[inline(always)]
    fn lazy(&mut self) {}
}

/// Adapter feeding the word stream into a `std::hash::Hasher`.
pub(crate) struct HashSink<'a, H: Hasher>(pub &'a mut H);

impl<H: Hasher> Sink for HashSink<'_, H> {
    #[inline]
    fn word(&mut self, w: u64) {
        self.0.write_u64(w);
    }
}

/// `walk` of an element of a compound value: scalars (most elements) are
/// streamed here, without a call.
#[inline(always)]
fn walk_elem<S: Sink>(v: &Value, s: &mut S, strict: bool) -> crate::Result<()> {
    match v {
        Value::Bool(b) => {
            s.word(TAG_BOOL);
            s.word(*b as u64);
        }
        Value::Int(n) => {
            s.word(TAG_INT);
            s.word(*n as u64);
        }
        Value::Str(sym) => {
            s.word(TAG_STR);
            s.word(sym.text_hash());
        }
        Value::ModelValue(sym) => {
            s.word(TAG_MV);
            s.word(sym.text_hash());
        }
        _ => walk(v, s, strict)?,
    }
    Ok(())
}

/// Emit the canonical word stream of `v`. With `strict`, values TLC cannot
/// fingerprint (infinite sets such as `Nat` or `Seq(S)`) are errors;
/// otherwise (for `Hash`) they get a structural encoding.
pub(crate) fn walk<S: Sink>(v: &Value, s: &mut S, strict: bool) -> crate::Result<()> {
    match v {
        Value::Bool(b) => {
            s.word(TAG_BOOL);
            s.word(*b as u64);
        }
        Value::Int(n) => {
            s.word(TAG_INT);
            s.word(*n as u64);
        }
        Value::Str(sym) => {
            s.word(TAG_STR);
            s.word(sym.text_hash());
        }
        Value::ModelValue(sym) => {
            s.word(TAG_MV);
            s.word(sym.text_hash());
        }
        Value::Set(elems) => {
            s.word(TAG_SET);
            s.word(elems.len() as u64);
            for e in elems.iter() {
                walk_elem(e, s, strict)?;
            }
        }
        Value::Interval(lo, hi) => {
            let n = crate::repr::interval_len(*lo, *hi)?;
            s.word(TAG_SET);
            s.word(n as u64);
            for i in 0..n as i64 {
                s.word(TAG_INT);
                s.word((lo + i) as u64);
            }
        }
        Value::LazySet(_) => {
            s.lazy();
            if let Some(b) = builtin(v) {
                if strict {
                    return err(format!("TLC has found a state in which the value of a variable contains {v}"));
                }
                s.word(TAG_BUILTIN);
                s.word(b.rank() as u64);
                if let Builtin::Seq(r) = b {
                    walk(r, s, strict)?;
                }
                return Ok(());
            }
            let elems = set_elems(v)?.unwrap();
            s.word(TAG_SET);
            s.word(elems.len() as u64);
            for i in 0..elems.len() {
                walk(&elems.get(i), s, strict)?;
            }
        }
        Value::Tuple(elems) => {
            s.word(TAG_TUPLE);
            s.word(elems.len() as u64);
            for e in elems.iter() {
                walk_elem(e, s, strict)?;
            }
        }
        Value::Record(fields) => {
            s.word(TAG_RECORD);
            s.word(fields.len() as u64);
            for (k, e) in fields.iter() {
                s.word(k.text_hash());
                walk_elem(e, s, strict)?;
            }
        }
        // A function over an enumerated domain fingerprints (and hashes) as
        // the equal `Tuple` / `Record` (see `FcnKind`).
        Value::Fcn(pairs, _) => match FcnKind::of(pairs) {
            FcnKind::Tuple => {
                s.word(TAG_TUPLE);
                s.word(pairs.len() as u64);
                for (_, e) in pairs.iter() {
                    walk_elem(e, s, strict)?;
                }
            }
            FcnKind::Record => {
                s.word(TAG_RECORD);
                s.word(pairs.len() as u64);
                for (k, e) in pairs.iter() {
                    let Value::Str(k) = k else { unreachable!("record-like keys are strings") };
                    s.word(k.text_hash());
                    walk_elem(e, s, strict)?;
                }
            }
            FcnKind::Fcn => {
                s.word(TAG_FCN);
                s.word(pairs.len() as u64);
                for (k, e) in pairs.iter() {
                    walk_elem(k, s, strict)?;
                    walk_elem(e, s, strict)?;
                }
            }
        },
        Value::LazyFcn(_) => {
            s.lazy();
            let mut slot = None;
            walk(force_fcn(v, &mut slot)?, s, strict)?;
        }
    }
    Ok(())
}

/// Fingerprint of a value (normalizes lazy finite values as needed).
pub fn fingerprint(v: &Value) -> crate::Result<u64> {
    let mut fp = Fingerprinter::new();
    fp.add(v)?;
    Ok(fp.finish())
}

/// `fingerprint(v)`, and whether `v` has no lazy part (`Value::is_normal`:
/// otherwise its normal form has this fingerprint), in one walk.
pub fn fingerprint_and_normal(v: &Value) -> crate::Result<(u64, bool)> {
    struct Checked {
        fp: Fingerprinter,
        normal: bool,
    }
    impl Sink for Checked {
        #[inline(always)]
        fn word(&mut self, w: u64) {
            self.fp.word(w);
        }
        fn lazy(&mut self) {
            self.normal = false;
        }
    }
    let mut c = Checked { fp: Fingerprinter::new(), normal: true };
    walk(v, &mut c, true)?;
    Ok((c.fp.finish(), c.normal))
}

/// Incremental fingerprinting of a sequence of values (e.g. a state).
/// Sensitive to the order of `add` calls.
#[derive(Clone)]
pub struct Fingerprinter(FoldHasher);

impl Fingerprinter {
    pub fn new() -> Fingerprinter {
        Fingerprinter(FoldHasher::new())
    }
    pub fn add(&mut self, v: &Value) -> crate::Result<()> {
        walk(v, self, true)
    }
    pub fn finish(&self) -> u64 {
        self.0.finish()
    }
}

impl Sink for Fingerprinter {
    #[inline(always)]
    fn word(&mut self, w: u64) {
        self.0.word(w);
    }
}

impl Default for Fingerprinter {
    fn default() -> Self {
        Self::new()
    }
}
