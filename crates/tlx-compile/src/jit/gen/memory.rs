//! Memory access of the Cranelift translation: loads, stores, copies, bit tests and word comparisons.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, MemFlags, StackSlotData, StackSlotKind, Value};

use cranelift_codegen::ir::types;

use super::super::I64;
use super::*;

impl<'c, 't, 'f> Gen<'c, 't, 'f> {
    pub(in crate::jit) fn flags() -> MemFlags {
        MemFlags::trusted()
    }

    pub(in crate::jit) fn load(&mut self, p: Value, word: usize) -> Value {
        match self.known.get(&p).and_then(|ws| ws.get(word)).copied() {
            Some(Known::Const(c)) => self.b.ins().iconst(I64, c as i64),
            Some(Known::Val(v)) => v,
            None => self.b.ins().load(I64, Self::flags(), p, (word * 8) as i32),
        }
    }

    /// Record that the words at `p` are `ws` wherever `p` is used (`p`
    /// points to immutable words: a constant, or a buffer written once
    /// before `p` is handed out).
    pub(in crate::jit) fn know(&mut self, p: Value, ws: Vec<Known>) {
        if ws.len() <= MAX_KNOWN {
            self.known.insert(p, ws);
        }
    }

    pub(in crate::jit) fn store(&mut self, v: Value, p: Value, word: usize) {
        self.b.ins().store(Self::flags(), v, p, (word * 8) as i32);
    }

    /// A fresh buffer holding the words `ws`, which are known to its
    /// loads; its stores are removed if nothing else reads it
    /// (`elide_stores`). Small encodings thus live in SSA values.
    pub(in crate::jit) fn materialize(&mut self, ws: &[Known]) -> Value {
        let vals: Vec<Value> = ws
            .iter()
            .map(|w| match *w {
                Known::Const(c) => self.iconst(c as i64),
                Known::Val(v) => v,
            })
            .collect();
        let buf = self.buf(ws.len());
        let p = self.addr(buf);
        let mut stores = Vec::with_capacity(ws.len());
        for (i, v) in vals.into_iter().enumerate() {
            stores.push(self.b.ins().store(Self::flags(), v, p, (i * 8) as i32));
        }
        self.elide.push((p, stores));
        self.know(p, ws.to_vec());
        p
    }

    /// The `n` words at `p` (loaded, or known).
    pub(in crate::jit) fn words(&mut self, p: Value, n: usize) -> Vec<Known> {
        (0..n)
            .map(|i| match self.known.get(&p).and_then(|ws| ws.get(i)).copied() {
                Some(k) => k,
                None => Known::Val(self.b.ins().load(I64, Self::flags(), p, (i * 8) as i32)),
            })
            .collect()
    }

    /// `p + idx * 8 * scale` (word index `idx` scaled by `scale` words).
    pub(in crate::jit) fn index(&mut self, p: Value, idx: Value, scale: usize) -> Value {
        if let Some(k) = self.konst(idx).filter(|k| (0..1 << 24).contains(k)) {
            return self.offset(p, k as usize * scale);
        }
        let off = self.b.ins().imul_imm(idx, (scale * 8) as i64);
        self.b.ins().iadd(p, off)
    }

    pub(in crate::jit) fn offset(&mut self, p: Value, words: usize) -> Value {
        if words == 0 {
            p
        } else {
            let q = self.b.ins().iadd_imm(p, (words * 8) as i64);
            if let Some(ws) = self.known.get(&p) {
                let rest = ws.get(words..).map(|r| r.to_vec()).unwrap_or_default();
                self.known.insert(q, rest);
            }
            q
        }
    }

    /// A function-scope buffer of `n` words.
    pub(in crate::jit) fn buf(&mut self, n: usize) -> Buf {
        Buf(self.b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, (n.max(1) * 8) as u32, 3)))
    }

    pub(in crate::jit) fn addr(&mut self, buf: Buf) -> Value {
        self.b.ins().stack_addr(I64, buf.0, 0)
    }

    /// memmove of `n` words (fixed).
    pub(in crate::jit) fn copy(&mut self, dst: Value, src: Value, n: usize) {
        if n <= INLINE_WORDS || (n <= MAX_KNOWN && self.known.contains_key(&src)) {
            let ws: Vec<Value> = (0..n).map(|i| self.load(src, i)).collect();
            for (i, w) in ws.into_iter().enumerate() {
                self.store(w, dst, i);
            }
        } else if n <= VECTOR_COPY_WORDS {
            // Two words at a time; all loads first (memmove).
            let f = MemFlags::new().with_notrap();
            let vs: Vec<Value> = (0..n / 2).map(|i| self.b.ins().load(types::I64X2, f, src, (i * 16) as i32)).collect();
            let last = (n % 2 == 1).then(|| self.load(src, n - 1));
            for (i, v) in vs.into_iter().enumerate() {
                self.b.ins().store(f, v, dst, (i * 16) as i32);
            }
            if let Some(w) = last {
                self.store(w, dst, n - 1);
            }
        } else {
            let bytes = self.iconst((n * 8) as i64);
            self.call_rt(super::super::rt::memmove as *const u8, &[dst, src, bytes]);
        }
    }

    /// memmove of `bytes` bytes (computed).
    pub(in crate::jit) fn copy_dyn(&mut self, dst: Value, src: Value, bytes: Value) {
        self.call_rt(super::super::rt::memmove as *const u8, &[dst, src, bytes]);
    }

    pub(in crate::jit) fn zero(&mut self, dst: Value, n: usize) {
        if n <= INLINE_WORDS {
            let z = self.iconst(0);
            for i in 0..n {
                self.store(z, dst, i);
            }
        } else if n <= VECTOR_COPY_WORDS {
            let f = MemFlags::new().with_notrap();
            let z = self.iconst(0);
            let zz = self.b.ins().splat(types::I64X2, z);
            for i in 0..n / 2 {
                self.b.ins().store(f, zz, dst, (i * 16) as i32);
            }
            if n % 2 == 1 {
                self.store(z, dst, n - 1);
            }
        } else {
            let bytes = self.iconst((n * 8) as i64);
            self.call_rt(super::super::rt::memzero as *const u8, &[dst, bytes]);
        }
    }

    /// Equality of `n` words (i64 0/1).
    pub(in crate::jit) fn eq_words(&mut self, a: Value, b: Value, n: usize) -> Value {
        if n <= 8 {
            // Words known equal drop out; one known to differ decides.
            let mut acc: Option<Value> = None;
            for i in 0..n {
                let (x, y) = (self.load(a, i), self.load(b, i));
                if x == y {
                    continue;
                }
                let d = match (self.konst(x), self.konst(y)) {
                    (Some(u), Some(v)) if u == v => continue,
                    (Some(_), Some(_)) => return self.iconst(0),
                    (_, Some(v)) => self.b.ins().bxor_imm(x, v),
                    (Some(u), _) => self.b.ins().bxor_imm(y, u),
                    _ => self.b.ins().bxor(x, y),
                };
                acc = Some(match acc {
                    Some(a) => self.b.ins().bor(a, d),
                    None => d,
                });
            }
            let Some(acc) = acc else { return self.iconst(1) };
            let c = self.cmp_imm(IntCC::Equal, acc, 0);
            self.b2i(c)
        } else {
            let k = self.iconst(n as i64);
            self.call_rt(super::super::rt::eq_words as *const u8, &[a, b, k])
        }
    }

    /// `tlx_bit(s, n, r)`: bit `r` of the `n`-word bitset `s` (0 outside).
    pub(in crate::jit) fn bit(&mut self, s: Value, n: usize, r: Value) -> Value {
        // The branchless form below reads word 0 also for an out-of-range
        // rank: an empty universe (0 words, e.g. a set variable that was only
        // {} in the pre-run) has no word 0 (`s` may dangle). C's `tlx_bit`
        // short-circuits instead.
        if n == 0 {
            return self.iconst(0);
        }
        if let Some(k) = self.konst(r) {
            if !(0..(n * 64) as i64).contains(&k) {
                return self.iconst(0);
            }
            let w = self.load(s, k as usize / 64);
            return match self.konst(w) {
                Some(x) => self.iconst((x as u64 >> (k % 64)) as i64 & 1),
                None => {
                    let x = self.b.ins().ushr_imm(w, k % 64);
                    self.b.ins().band_imm(x, 1)
                }
            };
        }
        let known = self.known.get(&s).and_then(|ws| ws.first()).copied();
        if let (1, Some(w)) = (n, known) {
            // One known word: no memory access.
            let w = match w {
                Known::Const(0) => return self.iconst(0),
                Known::Const(c) => self.iconst(c as i64),
                Known::Val(v) => v,
            };
            let x = self.b.ins().ushr(w, r);
            let x = self.b.ins().band_imm(x, 1);
            if self.bound(r).is_some_and(|b| b <= 64) {
                return x;
            }
            let inside = self.cmp_imm(IntCC::UnsignedLessThan, r, 64);
            let zero = self.iconst(0);
            return self.b.ins().select(inside, x, zero);
        }
        if self.bound(r).is_some_and(|b| b <= (n * 64) as u64) {
            let w = self.b.ins().ushr_imm(r, 6);
            let p = self.index(s, w, 1);
            let word = self.b.ins().load(I64, Self::flags(), p, 0);
            let x = self.b.ins().ushr(word, r);
            return self.b.ins().band_imm(x, 1);
        }
        let inside = self.cmp_imm(IntCC::UnsignedLessThan, r, (n * 64) as i64);
        let zero = self.iconst(0);
        let r_ = self.b.ins().select(inside, r, zero);
        let w = self.b.ins().ushr_imm(r_, 6);
        let p = self.index(s, w, 1);
        let word = self.b.ins().load(I64, Self::flags(), p, 0);
        let sh = self.b.ins().band_imm(r_, 63);
        let x = self.b.ins().ushr(word, sh);
        let x = self.b.ins().band_imm(x, 1);
        let inside = self.b2i(inside);
        self.b.ins().band(x, inside)
    }

    /// Set bit `r` (in range) of the bitset at `s`.
    pub(in crate::jit) fn set_bit(&mut self, s: Value, r: Value) {
        let w = self.b.ins().ushr_imm(r, 6);
        let p = self.index(s, w, 1);
        let word = self.b.ins().load(I64, Self::flags(), p, 0);
        let one = self.iconst(1);
        let sh = self.b.ins().band_imm(r, 63);
        let m = self.b.ins().ishl(one, sh);
        let word = self.b.ins().bor(word, m);
        self.b.ins().store(Self::flags(), word, p, 0);
    }
}
