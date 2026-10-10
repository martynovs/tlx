//! The immediate scalar schema of a handle (`u64` word): the low `TAG_BITS`
//! bits are a kind tag, and the payload above them is the value itself for
//! the scalar kinds — a `Sym` id for strings and model values, 0/1 for
//! Booleans, a signed integer of at most `IMM_INT_BITS` bits for small
//! integers (there is no room to tell them apart inside the handle, so the
//! tag carries that). Integers outside the immediate range and every
//! composite value go to the owner table instead.
//!
//! One owner of this schema, so the `Dyn` interner (`dynval`), the `Ref`
//! value store (`store`) and the C and JIT backends that inline the scalar
//! operations cannot drift apart: they all derive from these constants
//! (ARCH-07). The *composite* handle formats are separate and stay with
//! their tables — `dynval` uses tag 0 for a table entry (`SHARD_BITS`
//! index + shard), `store` tag 5 for a node (`STORE_BITS` store id +
//! index) — because they are ABI formats with their own bits, not part of
//! this schema.
//!
//! Decoding an arbitrary word is the checked boundary of CODE-06: `tag`,
//! `payload`, `as_int` and `as_bool` validate the tag and the payload
//! (Booleans must be 0/1), and `scalar_value` dispatches to the caller's
//! symbol resolver so both tables accept exactly the same scalar handles.

use tlx_value::{Sym, Value};

/// Bits of a handle that tell its kind.
pub const TAG_BITS: u32 = 3;
/// The tag mask: the low `TAG_BITS` bits of a handle.
pub const TAG_MASK: u64 = (1 << TAG_BITS) - 1;
/// Tag of a string handle (payload: the `Sym` id).
pub const TAG_STR: u64 = 1;
/// Tag of a model value handle (payload: the `Sym` id).
pub const TAG_MODEL_VALUE: u64 = 2;
/// Tag of a Boolean handle (payload: 0 or 1).
pub const TAG_BOOL: u64 = 3;
/// Tag of an immediate integer handle (payload: its two's-complement bits).
pub const TAG_INT: u64 = 4;
/// Payload bits of an immediate integer (the sign bit is the tag).
pub const IMM_INT_BITS: u32 = 63 - TAG_BITS;
/// Integers in `-IMM_INT..IMM_INT` are encoded in their handle.
pub const IMM_INT: i64 = 1 << IMM_INT_BITS;

/// The tag of handle `h`.
#[inline]
pub fn tag(h: u64) -> u64 {
    h & TAG_MASK
}

/// The payload of an immediate handle (`h >> TAG_BITS`; the inverse of
/// `encode`).
#[inline]
pub fn payload(h: u64) -> u64 {
    h >> TAG_BITS
}

/// The handle of `payload` with kind `tag` (the inverse of `tag` and
/// `payload`).
#[inline]
pub fn encode(payload: u64, tag: u64) -> u64 {
    payload << TAG_BITS | tag
}

/// Whether `t` (a tag) is an immediate scalar tag.
#[inline]
pub fn is_scalar_tag(t: u64) -> bool {
    matches!(t, TAG_STR | TAG_MODEL_VALUE | TAG_BOOL | TAG_INT)
}

/// Whether `h` is an immediate scalar handle (not a table or node handle).
#[inline]
pub fn is_immediate(h: u64) -> bool {
    is_scalar_tag(tag(h))
}

/// The immediate handle of a scalar (None: a composite value, or an
/// integer out of the immediate range).
#[inline]
pub fn immediate(v: &Value) -> Option<u64> {
    let (payload, tag) = match v {
        Value::Str(s) => (s.id() as u64, TAG_STR),
        Value::ModelValue(s) => (s.id() as u64, TAG_MODEL_VALUE),
        Value::Bool(b) => (*b as u64, TAG_BOOL),
        Value::Int(i) if (-IMM_INT..IMM_INT).contains(i) => (*i as u64, TAG_INT),
        _ => return None,
    };
    Some(encode(payload, tag))
}

/// The immediate handle of an integer (None: out of the immediate range).
#[inline]
pub fn int_handle(i: i64) -> Option<u64> {
    (-IMM_INT..IMM_INT).contains(&i).then_some(encode(i as u64, TAG_INT))
}

/// The immediate handle of a Boolean.
#[inline]
pub fn bool_handle(b: bool) -> u64 {
    encode(b as u64, TAG_BOOL)
}

/// The immediate handle of an interned string.
#[inline]
pub fn str_handle(s: Sym) -> u64 {
    encode(s.id() as u64, TAG_STR)
}

/// The integer of an immediate integer handle (None: another tag).
#[inline]
pub fn as_int(h: u64) -> Option<i64> {
    (tag(h) == TAG_INT).then_some((h as i64) >> TAG_BITS)
}

/// The Boolean of an immediate Boolean handle (None: another tag, or a
/// noncanonical payload).
#[inline]
pub fn as_bool(h: u64) -> Option<bool> {
    (tag(h) == TAG_BOOL && payload(h) <= 1).then_some(payload(h) != 0)
}

/// The checked scalar value of an arbitrary immediate word: the tag and
/// payload must be canonical, and `symbol` must resolve a string or model
/// value's word to a published `Sym`. `None` also covers a composite handle
/// (another tag): its format belongs to the owner table, which decodes it.
pub fn scalar_value(h: u64, symbol: impl FnOnce(u64) -> Option<Sym>) -> Option<Value> {
    Some(match tag(h) {
        TAG_STR => Value::Str(symbol(h)?),
        TAG_MODEL_VALUE => Value::ModelValue(symbol(h)?),
        TAG_BOOL => Value::Bool(as_bool(h)?),
        TAG_INT => Value::Int(as_int(h)?),
        _ => return None,
    })
}
