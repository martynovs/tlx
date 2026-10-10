//! Loops of the Cranelift translation: ranges and set bits, plus temp bindings.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, Value};
use cranelift_frontend::Variable;

use super::super::{no, Unsupported, I64, R};
use super::*;
use crate::ir::*;

impl<'c, 't, 'f> Gen<'c, 't, 'f> {
    /// A fresh i64 variable initialized to `v`.
    pub(in crate::jit) fn var(&mut self, v: Value) -> Variable {
        let x = self.b.declare_var(I64);
        self.b.def_var(x, v);
        x
    }

    /// `for (i = lo; i < hi; i++) body(i)` (`hi` evaluated once; with
    /// `inclusive`, `i <= hi`).
    pub(in crate::jit) fn for_range(
        &mut self,
        lo: Value,
        hi: Value,
        inclusive: bool,
        body: &mut dyn FnMut(&mut Self, Value) -> R<()>,
    ) -> R<()> {
        let i = self.var(lo);
        let (head, blk, exit) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
        self.b.ins().jump(head, &[]);
        self.switch(head);
        let iv = self.b.use_var(i);
        let c = self.cmp(if inclusive { IntCC::SignedLessThanOrEqual } else { IntCC::SignedLessThan }, iv, hi);
        self.brif(c, blk, &[], exit, &[]);
        self.switch(blk);
        body(self, iv)?;
        let next = self.b.ins().iadd_imm(iv, 1);
        self.b.def_var(i, next);
        self.b.ins().jump(head, &[]);
        self.switch(exit);
        Ok(())
    }

    /// For each set bit of the `n`-word bitset at `s`: `body(rank)`.
    pub(in crate::jit) fn for_bits(&mut self, s: Value, n: usize, body: &mut dyn FnMut(&mut Self, Value) -> R<()>) -> R<()> {
        self.for_bits_range(s, 0, n as u64 * 64, None, body)
    }

    /// For each set bit `lo <= rank < hi` of the bitset at `s` (`hi` within
    /// its words) also set in `mask` (if any; by word of the bitset):
    /// `body(rank)`, in increasing order.
    pub(in crate::jit) fn for_bits_range(
        &mut self,
        s: Value,
        lo: u64,
        hi: u64,
        mask: Option<&[u64]>,
        body: &mut dyn FnMut(&mut Self, Value) -> R<()>,
    ) -> R<()> {
        if lo >= hi {
            return Ok(());
        }
        let (w0, w1) = (lo / 64, (hi - 1) / 64);
        let lo_mask = !0u64 << (lo % 64);
        let hi_mask = if hi.is_multiple_of(64) { !0u64 } else { (1u64 << (hi % 64)) - 1 };
        // The mask of each word of the range.
        let masks: Vec<u64> = (w0..=w1)
            .map(|w| {
                let mut m = mask.map_or(!0, |m| m[w as usize]);
                if w == w0 {
                    m &= lo_mask;
                }
                if w == w1 {
                    m &= hi_mask;
                }
                m
            })
            .collect();
        if w0 == w1 {
            // One word: no loop over the words.
            let word = self.load(s, w0 as usize);
            let word = self.b.ins().band_imm(word, masks[0] as i64);
            let base = self.iconst(w0 as i64 * 64);
            return self.bits_of(word, base, body);
        }
        let table = mask.map(|_| self.cx.statics.words(&masks));
        let (a, b) = (self.iconst(w0 as i64), self.iconst(w1 as i64));
        self.for_range(a, b, true, &mut |g, w| {
            let p = g.index(s, w, 1);
            let word = g.b.ins().load(I64, Self::flags(), p, 0);
            let mut word = word;
            if let Some(t) = table {
                let tv = g.iconst(t - w0 as i64 * 8);
                let mp = g.index(tv, w, 1);
                let m = g.b.ins().load(I64, Self::flags(), mp, 0);
                word = g.b.ins().band(word, m);
            } else {
                if lo_mask != !0 {
                    let first = g.cmp_imm(IntCC::Equal, w, w0 as i64);
                    let (m, all) = (g.iconst(lo_mask as i64), g.iconst(-1));
                    let m = g.b.ins().select(first, m, all);
                    word = g.b.ins().band(word, m);
                }
                if hi_mask != !0 {
                    let last = g.cmp_imm(IntCC::Equal, w, w1 as i64);
                    let (m, all) = (g.iconst(hi_mask as i64), g.iconst(-1));
                    let m = g.b.ins().select(last, m, all);
                    word = g.b.ins().band(word, m);
                }
            }
            let base = g.b.ins().ishl_imm(w, 6);
            g.bits_of(word, base, body)
        })
    }

    /// For each set bit `i` of `word`: `body(base + i)`.
    fn bits_of(&mut self, word: Value, base: Value, body: &mut dyn FnMut(&mut Self, Value) -> R<()>) -> R<()> {
        let bits = self.var(word);
        let (head, blk, exit) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
        self.b.ins().jump(head, &[]);
        self.switch(head);
        let bv = self.b.use_var(bits);
        self.brif(bv, blk, &[], exit, &[]);
        self.switch(blk);
        let tz = self.b.ins().ctz(bv);
        let q = self.b.ins().iadd(base, tz);
        let m1 = self.b.ins().iadd_imm(bv, -1);
        let rest = self.b.ins().band(bv, m1);
        self.b.def_var(bits, rest);
        body(self, q)?;
        self.b.ins().jump(head, &[]);
        self.switch(exit);
        Ok(())
    }

    /// The value of temp `t`.
    pub(in crate::jit) fn temp(&mut self, t: Temp) -> R<Value> {
        match self.temps.get(t as usize) {
            Some(Ty::Val) if !self.mixing => no("boxed value"),
            _ => self.vals.get(t as usize).copied().flatten().ok_or(Unsupported(format!("unbound temp {t}"))),
        }
    }

    /// Bind temp `t` to `v` for the duration of `f`.
    pub(in crate::jit) fn with_temp<T>(&mut self, t: Temp, v: Value, f: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let i = t as usize;
        if self.vals.len() <= i {
            self.vals.resize(i + 1, None);
        }
        let old = self.vals[i].replace(v);
        let r = f(self);
        self.vals[i] = old;
        r
    }
}
