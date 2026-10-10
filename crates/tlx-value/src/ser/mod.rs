//! Compact binary serialization of values for checkpoint/recover and disk
//! spilling (docs/scale.md §9).
//!
//! # Byte stream
//!
//! A stream is written by one `Writer` and read back by one `Reader`, front
//! to back. Integers are LEB128 varints (`uvar`), signed ones zigzag-encoded
//! first (`ivar`). A value is a tag byte followed by its payload:
//!
//! | tag | value | payload |
//! |---|---|---|
//! | 0 / 1 | `FALSE` / `TRUE` | — |
//! | 2 | `Int` | ivar |
//! | 3 / 4 | `Str` / `ModelValue` | symbol reference |
//! | 5 | `Set` | uvar n, n values (normalized order) |
//! | 6 | `Interval` | ivar lo, ivar hi |
//! | 7 | `Tuple` | uvar n, n values |
//! | 8 | `Record` | uvar n, n × (symbol reference, value) in `Sym` order |
//! | 9 | `Fcn` | uvar n, n × (key, value) in key order |
//! | 15 | `Fcn` over an interval (`FcnDom::Interval`) | as 9 |
//! | 10..=13 | `Nat`, `Int`, `Real`, `STRING` | — |
//! | 14 | `Seq(S)` | value S |
//!
//! Other lazy values are normalized before they are written (errors for
//! infinite ones); the representation of normal values (`Interval` vs. an
//! enumerated `Set`) is preserved, so a decoded value is identical to the
//! encoded one, not just equal.
//!
//! A symbol reference is `uvar 0, uvar len, UTF-8 text` the first time a
//! stream mentions a symbol (it gets the next stream-local index) and
//! `uvar index + 1` afterwards. Streams therefore carry symbol *text*, never
//! process-local `Sym` ids, and stay compact for repeated strings.
//!
//! # Intern order
//!
//! `Sym` order (= first-intern order) is TLC's string order; it decides set
//! and record field order and CHOOSE results. Decoding a stream interns its
//! texts, which preserves values but not the order of symbols the stream
//! sees for the first time. Recover therefore restores the order first: a
//! checkpoint contains the full intern table (`write_intern_table`), and the
//! recovering process replays it with `read_intern_table` +
//! `intern::restore` before it parses the spec or interns anything else
//! (same tlx build, spec and cfg; startup interning then finds every text
//! already present). Encoded sets and records are not re-sorted on decode.
//!
//! The format is versioned by the enclosing frame
//! (`tlx_base::frame::FORMAT_VERSION`).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::intern::Sym;
use crate::lazy::LazySet;
use crate::value::{FcnDom, Value};

const T_FALSE: u8 = 0;
const T_TRUE: u8 = 1;
const T_INT: u8 = 2;
const T_STR: u8 = 3;
const T_MV: u8 = 4;
const T_SET: u8 = 5;
const T_INTERVAL: u8 = 6;
const T_TUPLE: u8 = 7;
const T_RECORD: u8 = 8;
const T_FCN: u8 = 9;
const T_NAT: u8 = 10;
const T_INTSET: u8 = 11;
// 12: formerly the `Real` set (Reals now uses its TLA+ definitions).
const T_STRING: u8 = 13;
const T_SEQ: u8 = 14;
const T_FCN_INTERVAL: u8 = 15;

/// Maximal nesting depth accepted by the decoder (guards the stack against
/// corrupt input; real values are far shallower).
const MAX_DEPTH: u32 = 2000;

/// Malformed input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError {
    pub message: String,
    /// Byte offset in the stream where the problem was detected.
    pub offset: usize,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "corrupt data at byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for DecodeError {}

pub type DecodeResult<T> = Result<T, DecodeError>;

/// Encoder of one byte stream (see the module documentation).
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
    syms: HashMap<Sym, u32, tlx_base::hash::IntegerBuildHasher>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer::default()
    }

    pub fn with_capacity(n: usize) -> Writer {
        Writer { buf: Vec::with_capacity(n), syms: HashMap::default() }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Start a new stream (forgets the symbol dictionary), keeping the buffer.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.syms.clear();
    }

    #[inline]
    pub fn u8(&mut self, b: u8) {
        self.buf.push(b);
    }

    #[inline]
    pub fn uvar(&mut self, n: u64) {
        tlx_base::bytes::put_uvar(&mut self.buf, n);
    }

    #[inline]
    pub fn ivar(&mut self, n: i64) {
        self.uvar(tlx_base::bytes::zigzag(n));
    }

    #[inline]
    pub fn usize(&mut self, n: usize) {
        self.uvar(n as u64);
    }

    /// Length-prefixed bytes.
    pub fn bytes(&mut self, b: &[u8]) {
        self.usize(b.len());
        self.buf.extend_from_slice(b);
    }

    pub fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }

    /// A symbol reference (text on first use in this stream).
    pub fn sym(&mut self, s: Sym) {
        let next = self.syms.len() as u32;
        match self.syms.entry(s) {
            std::collections::hash_map::Entry::Occupied(e) => {
                let i = *e.get();
                self.uvar(i as u64 + 1);
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(next);
                self.u8(0);
                self.str(s.as_str());
            }
        }
    }

    /// Append `v`. Lazy values are normalized first; errors if one cannot be
    /// (an infinite set other than `Nat`, `Int`, `Real`, `STRING`, `Seq(S)`).
    /// On error the stream contents are unspecified.
    pub fn value(&mut self, v: &Value) -> crate::Result<()> {
        match v {
            Value::Bool(false) => self.u8(T_FALSE),
            Value::Bool(true) => self.u8(T_TRUE),
            Value::Int(n) => {
                self.u8(T_INT);
                self.ivar(*n);
            }
            Value::Str(s) => {
                self.u8(T_STR);
                self.sym(*s);
            }
            Value::ModelValue(s) => {
                self.u8(T_MV);
                self.sym(*s);
            }
            Value::Set(es) => {
                self.u8(T_SET);
                self.values(es)?;
            }
            Value::Interval(lo, hi) => {
                self.u8(T_INTERVAL);
                self.ivar(*lo);
                self.ivar(*hi);
            }
            Value::Tuple(es) => {
                self.u8(T_TUPLE);
                self.values(es)?;
            }
            Value::Record(fs) => {
                self.u8(T_RECORD);
                self.usize(fs.len());
                for (k, x) in fs.iter() {
                    self.sym(*k);
                    self.value(x)?;
                }
            }
            Value::Fcn(ps, d) => {
                self.u8(if *d == FcnDom::Interval { T_FCN_INTERVAL } else { T_FCN });
                self.usize(ps.len());
                for (k, x) in ps.iter() {
                    self.value(k)?;
                    self.value(x)?;
                }
            }
            Value::LazySet(l) => match &**l {
                LazySet::Nat => self.u8(T_NAT),
                LazySet::Int => self.u8(T_INTSET),
                LazySet::StringSet => self.u8(T_STRING),
                LazySet::Seq(s) => {
                    self.u8(T_SEQ);
                    self.value(s)?;
                }
                _ => self.value(&v.normalize()?)?,
            },
            Value::LazyFcn(_) => self.value(&v.normalize()?)?,
        }
        Ok(())
    }

    fn values(&mut self, es: &[Value]) -> crate::Result<()> {
        self.usize(es.len());
        for e in es {
            self.value(e)?;
        }
        Ok(())
    }
}

/// Decode exactly `n` items into a shared slice.
///
/// `(0..n)` has an exact length, so `Arc` allocates the slice once and fills
/// it in place; decoding into a `Vec` and converting it would allocate twice
/// and copy every item. `n` must come from `Reader::count` (which rejects
/// counts that cannot fit in the input); items after the first error get the
/// placeholder `fill`, are never returned, and the first error wins.
pub fn decode_arc<T: Clone>(n: usize, fill: T, mut item: impl FnMut() -> DecodeResult<T>) -> DecodeResult<Arc<[T]>> {
    let mut err = None;
    let items: Arc<[T]> = (0..n)
        .map(|_| match item() {
            Ok(v) => v,
            Err(e) => {
                err.get_or_insert(e);
                fill.clone()
            }
        })
        .collect();
    match err {
        Some(e) => Err(e),
        None => Ok(items),
    }
}

/// Decoder of one byte stream written by a `Writer`.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    syms: Vec<Sym>,
    depth: u32,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0, syms: Vec::new(), depth: 0 }
    }

    /// Current byte offset.
    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn error<T>(&self, message: impl Into<String>) -> DecodeResult<T> {
        Err(DecodeError { message: message.into(), offset: self.pos })
    }

    /// Error unless the whole input has been consumed.
    pub fn finish(&self) -> DecodeResult<()> {
        if self.is_empty() {
            Ok(())
        } else {
            self.error(format!("{} trailing bytes", self.remaining()))
        }
    }

    #[inline]
    pub fn u8(&mut self) -> DecodeResult<u8> {
        match self.buf.get(self.pos) {
            Some(&b) => {
                self.pos += 1;
                Ok(b)
            }
            None => self.error("unexpected end of data"),
        }
    }

    #[inline]
    pub fn uvar(&mut self) -> DecodeResult<u64> {
        match tlx_base::bytes::get_uvar(self.buf, &mut self.pos) {
            Ok(n) => Ok(n),
            Err(tlx_base::bytes::VarintError::End) => self.error("unexpected end of data"),
            Err(tlx_base::bytes::VarintError::Overflow) => self.error("varint overflow"),
        }
    }

    #[inline]
    pub fn ivar(&mut self) -> DecodeResult<i64> {
        let u = self.uvar()?;
        Ok(tlx_base::bytes::unzigzag(u))
    }

    pub fn usize(&mut self) -> DecodeResult<usize> {
        let n = self.uvar()?;
        usize::try_from(n).or_else(|_| self.error("length out of range"))
    }

    /// A count of items that occupy at least `min_bytes` bytes each (rejects
    /// counts that cannot fit in the rest of the input before allocating).
    pub fn count(&mut self, min_bytes: usize) -> DecodeResult<usize> {
        let n = self.usize()?;
        if n.saturating_mul(min_bytes) > self.remaining() {
            return self.error(format!("count {n} exceeds the remaining input"));
        }
        Ok(n)
    }

    pub fn bytes(&mut self) -> DecodeResult<&'a [u8]> {
        let n = self.count(1)?;
        let b = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(b)
    }

    pub fn str(&mut self) -> DecodeResult<&'a str> {
        let at = self.pos;
        let b = self.bytes()?;
        std::str::from_utf8(b).map_err(|_| DecodeError { message: "invalid UTF-8".into(), offset: at })
    }

    pub fn sym(&mut self) -> DecodeResult<Sym> {
        let i = self.uvar()?;
        if i == 0 {
            let s = Sym::new(self.str()?);
            self.syms.push(s);
            return Ok(s);
        }
        match self.syms.get((i - 1) as usize) {
            Some(&s) => Ok(s),
            None => self.error(format!("unknown symbol reference {i}")),
        }
    }

    pub fn value(&mut self) -> DecodeResult<Value> {
        if self.depth >= MAX_DEPTH {
            return self.error("value nested too deeply");
        }
        self.depth += 1;
        let r = self.value_inner();
        self.depth -= 1;
        r
    }

    fn value_inner(&mut self) -> DecodeResult<Value> {
        let tag = self.u8()?;
        Ok(match tag {
            T_FALSE => Value::FALSE,
            T_TRUE => Value::TRUE,
            T_INT => Value::Int(self.ivar()?),
            T_STR => Value::Str(self.sym()?),
            T_MV => Value::ModelValue(self.sym()?),
            T_SET => {
                let es = self.values()?;
                #[cfg(debug_assertions)]
                self.check_canonical("set", &es, |v| v)?;
                Value::Set(es)
            }
            T_INTERVAL => Value::Interval(self.ivar()?, self.ivar()?),
            // A tuple is not a set: its order is arbitrary.
            T_TUPLE => Value::Tuple(self.values()?),
            T_RECORD => {
                let n = self.count(2)?;
                if n == 0 {
                    return self.error("empty record");
                }
                let mut fs: Vec<(Sym, Value)> = Vec::with_capacity(n);
                for _ in 0..n {
                    let k = self.sym()?;
                    if fs.last().is_some_and(|l| l.0 >= k) {
                        return self
                            .error(format!("record fields out of order at {:?} (intern order differs from the writer's?)", k.as_str()));
                    }
                    let x = self.value()?;
                    fs.push((k, x));
                }
                Value::Record(Arc::from(fs))
            }
            T_FCN | T_FCN_INTERVAL => {
                let dom = if tag == T_FCN_INTERVAL { FcnDom::Interval } else { FcnDom::Enumerated };
                let n = self.count(2)?;
                let ps = decode_arc(n, (Value::FALSE, Value::FALSE), || Ok((self.value()?, self.value()?)))?;
                #[cfg(debug_assertions)]
                self.check_canonical("function domain", &ps, |p| &p.0)?;
                Value::Fcn(ps, dom)
            }
            T_NAT => Value::LazySet(Arc::new(LazySet::Nat)),
            T_INTSET => Value::LazySet(Arc::new(LazySet::Int)),
            T_STRING => Value::LazySet(Arc::new(LazySet::StringSet)),
            T_SEQ => Value::LazySet(Arc::new(LazySet::Seq(self.value()?))),
            _ => {
                self.pos -= 1;
                return self.error(format!("unknown value tag {tag}"));
            }
        })
    }

    fn values(&mut self) -> DecodeResult<Arc<[Value]>> {
        let n = self.count(1)?;
        decode_arc(n, Value::FALSE, || self.value())
    }

    /// Debug check: set elements and function domains are sorted in TLC order
    /// with no duplicates. Every `Value` is canonical by construction (its
    /// comparisons, hashing and printing assume it) and the encoder writes
    /// nothing else, so a stream that fails this is corrupt.
    #[cfg(debug_assertions)]
    fn check_canonical<T>(&self, what: &str, items: &[T], key: impl Fn(&T) -> &Value) -> DecodeResult<()> {
        for w in items.windows(2) {
            if !matches!(crate::order::try_total_cmp(key(&w[0]), key(&w[1])), Ok(std::cmp::Ordering::Less)) {
                return self.error(format!("{what} not in canonical order at {}", key(&w[1])));
            }
        }
        Ok(())
    }
}

/// Encode one value as a self-contained stream.
#[doc(hidden)]
pub fn encode_value(v: &Value) -> crate::Result<Vec<u8>> {
    let mut w = Writer::new();
    w.value(v)?;
    Ok(w.into_bytes())
}

/// Decode a stream holding exactly one value.
#[doc(hidden)]
pub fn decode_value(b: &[u8]) -> DecodeResult<Value> {
    let mut r = Reader::new(b);
    let v = r.value()?;
    r.finish()?;
    Ok(v)
}

/// Write the full string intern table (`intern::snapshot`): count, then
/// each text in id order.
pub fn write_intern_table(w: &mut Writer) {
    let texts = crate::intern::snapshot();
    w.usize(texts.len());
    for t in texts {
        w.str(t);
    }
}

/// Read a table written by `write_intern_table` (restore it with
/// `intern::restore`, before anything else interns).
pub fn read_intern_table<'a>(r: &mut Reader<'a>) -> DecodeResult<Vec<&'a str>> {
    let n = r.count(1)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(r.str()?);
    }
    Ok(out)
}
