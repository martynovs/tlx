//! Out-of-line helpers called by JIT code (the counterparts of the C
//! `runtime.h` functions that are not inlined). All take and return `i64`
//! (pointers and sizes included) so that one signature shape fits all;
//! status results are 1 = ok, 0 = runtime error.
//!
//! Generated-call contract: sizes are nonnegative, arithmetic fits usize/i64,
//! and every span is contained in one allocation and at most isize::MAX bytes.
//! u64 pointers are non-null and 8-byte aligned even for empty spans. Inputs
//! are initialized encodings matching the verified IR; the owning generated
//! frame/statics stay live throughout the call. No Rust borrow escapes it.
//! Mutable spans are exclusive; overlapping sorted-set inputs are copied
//! before borrowing output. Iterator storage is aligned for SubsetsIter and
//! disjoint from all buffers. Expected evaluation failures return 0; verified
//! shapes, capacities and plans must prevent bounds/arithmetic panics, since
//! these extern C helpers cannot unwind through generated frames.

use tlx_value::arith;

use crate::exec::encops;
use crate::ir::SetOp;

/// # Safety
/// p spans n initialized, aligned u64s in one allocation, live and immutable
/// for the returned lifetime; the module contract applies even when n is 0.
unsafe fn words<'a>(p: i64, n: usize) -> &'a [u64] {
    // SAFETY: the caller establishes the readable span and borrow lifetime.
    unsafe { std::slice::from_raw_parts(p as *const u64, n) }
}

/// # Safety
/// p is non-null and u64-aligned even for n == 0, and spans n initialized
/// words in one live allocation (at most isize::MAX bytes). The caller has
/// exclusive access for the returned lifetime, with no overlapping input
/// references. Uninitialized output slots must be written before calling.
unsafe fn words_mut<'a>(p: i64, n: usize) -> &'a mut [u64] {
    // SAFETY: the caller establishes the initialized, exclusive span above;
    // all helper callers keep the borrow within the generated function call.
    unsafe { std::slice::from_raw_parts_mut(p as *mut u64, n) }
}

/// # Safety
/// As words_mut, but the exclusive output allocation may be uninitialized.
/// Existing contents are discarded; no live input may overlap this span.
#[inline]
unsafe fn output_words<'a>(p: i64, n: usize) -> &'a mut [u64] {
    // SAFETY: raw writes accept uninitialized storage; the caller supplies
    // the full exclusive span. Initialize before creating any Rust reference.
    unsafe { (p as *mut u64).write_bytes(0, n) };
    // SAFETY: all n words are now initialized, with the same exclusive lifetime.
    unsafe { words_mut(p, n) }
}

pub extern "C" fn memmove(dst: i64, src: i64, bytes: i64) -> i64 {
    if bytes > 0 {
        // SAFETY: both byte spans are live for bytes bytes; copy permits
        // overlap. Generated code supplies nonnegative, representable sizes.
        unsafe { std::ptr::copy(src as *const u8, dst as *mut u8, bytes as usize) };
    }
    0
}

pub extern "C" fn memzero(dst: i64, bytes: i64) -> i64 {
    if bytes > 0 {
        // SAFETY: dst exclusively owns bytes writable bytes; raw writes do
        // not require initialized contents and retain no pointer or reference.
        unsafe { std::ptr::write_bytes(dst as *mut u8, 0, bytes as usize) };
    }
    0
}

/// 1 if the `n` words at `a` and `b` are equal.
pub extern "C" fn eq_words(a: i64, b: i64, n: i64) -> i64 {
    // SAFETY: both inputs span n initialized immutable words for this call.
    unsafe { (words(a, n as usize) == words(b, n as usize)) as i64 }
}

/// A result through `r`: 1 = ok, 0 = runtime error.
fn out(v: tlx_value::Result<i64>, r: i64) -> i64 {
    let Ok(v) = v else { return 0 };
    // SAFETY: r is an aligned exclusive one-word result slot in the current
    // generated frame. A raw write initializes it without forming a reference.
    unsafe { *(r as *mut i64) = v };
    1
}

/// `tlx_divn`: `\div` of Naturals (truncation quirk); result through `r`.
pub extern "C" fn divn(a: i64, b: i64, r: i64) -> i64 {
    if b == 0 {
        return 0;
    }
    let mut q = a.wrapping_div(b);
    if q < 0 && q.wrapping_mul(b) != a {
        q -= 1;
    }
    // SAFETY: r is the aligned exclusive one-word result slot; raw writing
    // accepts its initially uninitialized contents and retains no reference.
    unsafe { *(r as *mut i64) = q };
    1
}

/// `tlx_pow` (`arith::pow`); result through `r`.
pub extern "C" fn pow(a: i64, b: i64, r: i64) -> i64 {
    out(arith::pow(a, b), r)
}

/// `tlx_gcd` (`arith::gcd`); result through `r`.
pub extern "C" fn gcd(a: i64, b: i64, r: i64) -> i64 {
    out(arith::gcd(a, b), r)
}

/// Whether the `n`-word ranges at `a` and `b` overlap.
fn overlap(a: i64, an: usize, b: i64, bn: usize) -> bool {
    a < b + 8 * bn as i64 && b < a + 8 * an as i64
}

/// The sorted-array set at `s` (elements of `n` words), up to its last
/// element.
///
/// # Safety
/// s has a readable header and 1 + header * n initialized words, all in
/// one live allocation. That computed span satisfies words' size contract.
unsafe fn sorted_set<'a>(s: i64, n: usize) -> &'a [u64] {
    // SAFETY: the caller supplies the header and the complete initialized
    // element span it describes; the verified shape bounds the multiplication.
    unsafe { words(s, 1 + words(s, 1)[0] as usize * n) }
}

/// `tlx_sorted_member(s, x, n)`.
pub extern "C" fn sorted_member(s: i64, x: i64, n: i64) -> i64 {
    let n = n as usize;
    // SAFETY: s is an initialized sorted set and x spans its n-word element
    // encoding; both immutable borrows end before returning to generated code.
    let (s, x) = unsafe { (sorted_set(s, n), words(x, n)) };
    encops::sorted_member(s, x, n) as i64
}

/// `tlx_sorted_insert(out, words, s, x, n, max_card)`.
pub extern "C" fn sorted_insert(out: i64, nwords: i64, s: i64, x: i64, n: i64, max_card: i64) -> i64 {
    let (nw, n) = (nwords as usize, n as usize);
    // (Copied only if `out` overlaps an input: this runs once per
    // successor.)
    let copy;
    // SAFETY: s spans nw initialized words and x spans n. If out overlaps
    // either, copy both inputs so no input reference aliases the output borrow.
    let (src, x) = unsafe {
        if overlap(out, nw, s, nw) || overlap(out, nw, x, n) {
            copy = (words(s, nw).to_vec(), words(x, n).to_vec());
            (&copy.0[..], &copy.1[..])
        } else {
            (words(s, nw), words(x, n))
        }
    };
    // SAFETY: out spans nw writable words; any overlapping inputs were copied
    // above. Initialize the fresh slot before borrowing it for the safe helper.
    let o = unsafe { output_words(out, nw) };
    encops::sorted_insert(o, src, x, n, max_card as u64) as i64
}

/// `tlx_sorted_add(s, x, n, max_card)`: insert in place (`s` has room for
/// `max_card` elements).
pub extern "C" fn sorted_add(s: i64, x: i64, n: i64, max_card: i64) -> i64 {
    let n = n as usize;
    // SAFETY: s has an initialized one-word header, read before mutable access.
    let len = unsafe { words(s, 1)[0] } as usize;
    // The generated set buffer has room for one more element below max_card.
    let cap = 1 + (len + 1).min(max_card.max(0) as usize).max(len) * n;
    let mut local = [0u64; 16];
    let heap;
    // (Copied: `x` may point into `s`.)
    let x: &[u64] = if n <= local.len() {
        // SAFETY: x spans n initialized words, copied before s is mutably
        // borrowed, including when the input points inside the set storage.
        local[..n].copy_from_slice(unsafe { words(x, n) });
        &local[..n]
    } else {
        // SAFETY: the same initialized x span is copied into owned storage.
        heap = unsafe { words(x, n) }.to_vec();
        &heap
    };
    let live = 1 + len * n;
    // SAFETY: s owns cap writable words; its header and len elements are
    // already initialized. Initialize only the newly exposed tail, after
    // copying the possibly overlapping x input, before forming the full slice.
    unsafe { (s as *mut u64).add(live).write_bytes(0, cap - live) };
    // SAFETY: the complete cap-word span is now initialized and exclusive;
    // x references only the local copy, not this mutable slice.
    let sw = unsafe { words_mut(s, cap) };
    encops::sorted_add(sw, x, n, max_card as u64) as i64
}

/// `tlx_sorted_op(op, out, words, a, b, n, max_card)`: union (0),
/// intersection (1), difference (2) of sorted-array sets.
pub extern "C" fn sorted_op(op: i64, out: i64, nwords: i64, a: i64, b: i64, n: i64, max_card: i64) -> i64 {
    let (nw, n) = (nwords as usize, n as usize);
    let op = match op {
        0 => SetOp::Union,
        1 => SetOp::Inter,
        _ => SetOp::Diff,
    };
    let copy;
    // SAFETY: a/b contain initialized headers and their complete element
    // spans. Copy both if output overlaps either before making a mutable slice.
    let (av, bv) = unsafe {
        let (av, bv) = (sorted_set(a, n), sorted_set(b, n));
        if overlap(out, nw, a, av.len()) || overlap(out, nw, b, bv.len()) {
            copy = (av.to_vec(), bv.to_vec());
            (&copy.0[..], &copy.1[..])
        } else {
            (av, bv)
        }
    };
    // SAFETY: out spans nw exclusive writable words; retained inputs are
    // disjoint (or owned copies). Initialize before borrowing the output.
    let o = unsafe { output_words(out, nw) };
    encops::sorted_op(op, av, bv, n, max_card as u64, o) as i64
}

/// Iterator over the subsets of a bitset (by size, then lexicographically
/// by rank: TLC's `SUBSET` order), as the C `Subsets` loop.
#[repr(C)]
pub struct SubsetsIter {
    ranks: [u32; 64],
    idx: [u32; 64],
    n: u32,
    k: u32,
    /// 0: before the first subset of size k; 1: inside.
    started: u32,
}

pub const SUBSETS_ITER_BYTES: u32 = std::mem::size_of::<SubsetsIter>() as u32;

/// Start iterating over the subsets of the `nwords`-word bitset `s` (0 if
/// it has more than 24 elements: runtime error, as in C).
pub extern "C" fn subsets_init(it: i64, s: i64, nwords: i64) -> i64 {
    let it = it as *mut SubsetsIter;
    // SAFETY: generated code supplies an aligned, exclusively writable
    // SubsetsIter-sized stack slot, disjoint from s. It starts uninitialized:
    // write every field before creating a reference (not just n/k/started).
    unsafe { it.write(SubsetsIter { ranks: [0; 64], idx: [0; 64], n: 0, k: 0, started: 0 }) };
    // SAFETY: the full slot was initialized above and remains exclusively
    // borrowed for this call; no pointer escapes the generated stack frame.
    let it = unsafe { &mut *it };
    // SAFETY: s spans nwords initialized immutable bitset words, disjoint
    // from the iterator slot initialized above.
    for (w, &word) in unsafe { words(s, nwords as usize) }.iter().enumerate() {
        let mut b = word;
        while b != 0 {
            if it.n == 24 {
                return 0;
            }
            it.ranks[it.n as usize] = w as u32 * 64 + b.trailing_zeros();
            it.n += 1;
            b &= b - 1;
        }
    }
    it.k = 0;
    it.started = 0;
    1
}

/// Write the next subset into `out` (`nwords` words); 0 when done.
pub extern "C" fn subsets_next(it: i64, out: i64, nwords: i64) -> i64 {
    // SAFETY: it is the aligned exclusive slot fully initialized by a
    // successful subsets_init call with the same nwords; it does not alias out.
    let it = unsafe { &mut *(it as *mut SubsetsIter) };
    loop {
        if it.k > it.n {
            return 0;
        }
        let k = it.k as usize;
        if it.started == 0 {
            for j in 0..k {
                it.idx[j] = j as u32;
            }
            it.started = 1;
            break;
        }
        if encops::next_combination(&mut it.idx[..k], it.n) {
            break;
        }
        it.k += 1;
        it.started = 0;
    }
    // SAFETY: out spans nwords exclusive writable words, disjoint from it.
    // Initialize before creating a slice; the retained ranks fit this bitset.
    let o = unsafe { output_words(out, nwords as usize) };
    for j in 0..it.k as usize {
        let r = it.ranks[it.idx[j] as usize];
        o[(r / 64) as usize] |= 1u64 << (r % 64);
    }
    1
}

/// Sort indices of encoded elements using the same plan as generated C.
/// # Safety
/// p is a validated complete order plan, s is its initialized set encoding,
/// and out has exclusive room for the set cardinality, disjoint from p/s.
pub(super) unsafe extern "C" fn order_indices(p: i64, s: i64, out: i64) -> i64 {
    // SAFETY: the verified plan has its four initialized header words.
    let head = unsafe { words(p, 4) };
    // SAFETY: the plan header span was validated against its backing array.
    let plan = unsafe { words(p, head[2] as usize) };
    // SAFETY: s contains the initialized set-cardinality header.
    let n = unsafe { words(s, 1) }[0] as usize;
    // SAFETY: the verified encoding contains n elements of header[1] words.
    let set = unsafe { words(s, 1 + n * head[1] as usize) };
    // SAFETY: the generated frame owns n output slots disjoint from inputs.
    // MaybeUninit permits fresh storage; sorting initializes before reading.
    let out = unsafe { std::slice::from_raw_parts_mut(out as *mut std::mem::MaybeUninit<u64>, n) };
    crate::encoded_order::indices_uninit(plan, set, out);
    0
}

/// # Safety
/// keys spans universe initialized rank keys, s spans nw initialized bitset
/// words with zero padding past universe, and out has exclusive room for
/// universe indices, disjoint from both inputs. The module contract applies.
pub(super) unsafe extern "C" fn order_bit_indices(keys: i64, s: i64, nw: i64, out: i64, universe: i64) -> i64 {
    // SAFETY: verified IR provides the immutable universe-sized key array.
    let keys = unsafe { words(keys, universe as usize) };
    // SAFETY: the input bitset has nw initialized words and zero padding.
    let set = unsafe { words(s, nw as usize) };
    // SAFETY: the generated frame owns universe output slots, disjoint from
    // keys/set. MaybeUninit permits fresh storage; only the written prefix
    // will be read, so neither a preliminary bit count nor clearing is needed.
    let out = unsafe { std::slice::from_raw_parts_mut(out as *mut std::mem::MaybeUninit<u64>, universe as usize) };
    crate::encoded_order::bit_indices_uninit(keys, set, out) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsets_init_initializes_the_entire_slot() {
        // Poison an initialized slot so a partial initializer fails
        // deterministically, without reading uninitialized memory in the test.
        let mut slot = SubsetsIter { ranks: [u32::MAX; 64], idx: [u32::MAX; 64], n: u32::MAX, k: u32::MAX, started: u32::MAX };
        let empty = [0u64];
        assert_eq!(subsets_init(std::ptr::addr_of_mut!(slot) as i64, empty.as_ptr() as i64, 1), 1);
        assert_eq!(slot.ranks, [0; 64]);
        assert_eq!(slot.idx, [0; 64]);
        assert_eq!((slot.n, slot.k, slot.started), (0, 0, 0));
        let mut out = [u64::MAX];
        assert_eq!(subsets_next(std::ptr::addr_of_mut!(slot) as i64, out.as_mut_ptr() as i64, 1), 1);
        assert_eq!(out, [0]);
        assert_eq!(subsets_next(std::ptr::addr_of_mut!(slot) as i64, out.as_mut_ptr() as i64, 1), 0);
    }

    #[test]
    fn outputs_initialize_fresh_slots_and_preserve_overlapping_inputs() {
        use std::mem::MaybeUninit;
        let mut output = MaybeUninit::<[u64; 5]>::uninit();
        let s = [2, 1, 3, 0, 0];
        let x = [2u64];
        assert_eq!(sorted_insert(output.as_mut_ptr() as i64, 5, s.as_ptr() as i64, x.as_ptr() as i64, 1, 4), 1);
        // SAFETY: successful sorted_insert initialized the entire result span.
        assert_eq!(unsafe { output.assume_init() }, [3, 1, 2, 3, 0]);
        let mut aliased = s;
        let p = aliased.as_mut_ptr() as i64;
        assert_eq!(sorted_insert(p, 5, p, x.as_ptr() as i64, 1, 4), 1);
        assert_eq!(aliased, [3, 1, 2, 3, 0]);

        let b = [2u64, 2, 3, 0, 0];
        for (op, expected) in [(0, [3, 1, 2, 3, 0]), (1, [1, 3, 0, 0, 0]), (2, [1, 1, 0, 0, 0])] {
            let mut output = MaybeUninit::<[u64; 5]>::uninit();
            assert_eq!(sorted_op(op, output.as_mut_ptr() as i64, 5, s.as_ptr() as i64, b.as_ptr() as i64, 1, 4), 1);
            // SAFETY: successful sorted_op initialized all five result words.
            assert_eq!(unsafe { output.assume_init() }, expected);
            let mut aliased = s;
            let p = aliased.as_mut_ptr() as i64;
            assert_eq!(sorted_op(op, p, 5, p, b.as_ptr() as i64, 1, 4), 1);
            assert_eq!(aliased, expected);
        }

        let mut growing = [MaybeUninit::new(2u64), MaybeUninit::new(1), MaybeUninit::new(3), MaybeUninit::uninit()];
        let gp = growing.as_mut_ptr() as i64;
        assert_eq!(sorted_add(gp, x.as_ptr() as i64, 1, 3), 1);
        // SAFETY: the header/old elements started initialized; sorted_add
        // initialized the spare slot before growing into it.
        assert_eq!(growing.map(|v| unsafe { v.assume_init() }), [3, 1, 2, 3]);
        let mut aliased = [2u64, 1, 3, 0];
        let p = aliased.as_mut_ptr() as i64;
        assert_eq!(sorted_add(p, p + 8, 1, 3), 1);
        assert_eq!(aliased, [2, 1, 3, 0]);
    }

    #[test]
    fn ordering_and_subsets_write_only_their_initialized_results() {
        use std::mem::MaybeUninit;
        let plan = [crate::encoded_order::schema::WORDS, 1, 4, 0];
        let set = [3u64, 9, 2, 5];
        let mut out = MaybeUninit::<[u64; 3]>::uninit();
        // SAFETY: valid one-word order plan/set, and exclusive fresh output
        // with room for all three indices, disjoint from both inputs.
        unsafe { order_indices(plan.as_ptr() as i64, set.as_ptr() as i64, out.as_mut_ptr() as i64) };
        // SAFETY: order_indices initialized all three cardinality slots.
        assert_eq!(unsafe { out.assume_init() }, [1, 2, 0]);
        let mut initialized = [99u64; 4];
        crate::encoded_order::indices(&plan, &set, &mut initialized);
        assert_eq!(initialized, [1, 2, 0, 99]);

        let keys = [2u64, 1, 0, 3];
        let bits = [0b0101u64];
        let mut out = [MaybeUninit::<u64>::uninit(); 4];
        // SAFETY: four rank keys, one padded bitset word and four exclusive
        // output slots; only the two present-rank results will be read.
        let n = unsafe { order_bit_indices(keys.as_ptr() as i64, bits.as_ptr() as i64, 1, out.as_mut_ptr() as i64, 4) };
        assert_eq!(n, 2);
        // SAFETY: the returned prefix has exactly two initialized results.
        assert_eq!(unsafe { [out[0].assume_init(), out[1].assume_init()] }, [2, 0]);
        let mut initialized = [99u64; 4];
        assert_eq!(crate::encoded_order::bit_indices(&keys, &bits, &mut initialized), 2);
        assert_eq!(initialized, [2, 0, 99, 99]);
        assert_eq!(crate::encoded_order::bit_indices(&keys, &[0], &mut initialized), 0);
        assert_eq!(initialized, [2, 0, 99, 99]);

        let mut it = MaybeUninit::<SubsetsIter>::uninit();
        let bitset = [0b101001u64];
        let ip = it.as_mut_ptr() as i64;
        assert_eq!(subsets_init(ip, bitset.as_ptr() as i64, 1), 1);
        for expected in [0u64, 1, 8, 32, 9, 33, 40, 41] {
            let mut out = MaybeUninit::<u64>::uninit();
            assert_eq!(subsets_next(ip, out.as_mut_ptr() as i64, 1), 1);
            // SAFETY: successful subsets_next initializes its one-word result.
            assert_eq!(unsafe { out.assume_init() }, expected);
        }
        let mut untouched = u64::MAX;
        assert_eq!(subsets_next(ip, std::ptr::addr_of_mut!(untouched) as i64, 1), 0);
        assert_eq!(untouched, u64::MAX);
    }
}
