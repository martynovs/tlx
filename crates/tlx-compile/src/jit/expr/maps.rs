//! Cranelift generation of the map family of encoded expressions:
//! `MapGet`, `MapPut`, `MapBuild` and `MapMerge` (moved verbatim from
//! `encoded.rs`).

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, Value};
use tlx_layout::layout::MapDims;

use super::super::gen::Gen;
use super::super::{no, R};
use crate::ir::*;

impl Gen<'_, '_, '_> {
    /// The map family.
    pub(super) fn map_expr(&mut self, e: &Expr) -> R<Value> {
        Ok(match &*e.kind {
            ExprKind::MapGet { base, rank } => {
                let MapDims { universe: u, bitset_words: bw, slot_words: w } = self.map_dims(base.ty)?;
                let (m, r) = (self.expr(base)?, self.int_expr(rank)?);
                let present = self.bit(m, bw, r);
                let inside = self.cmp_imm(IntCC::SignedLessThan, r, u as i64);
                let inside = self.b2i(inside);
                let ok = self.b.ins().band(present, inside);
                self.check(ok);
                let p = self.offset(m, bw);
                self.index(p, r, w)
            }
            ExprKind::MapPut { base, rank, value, plan } => {
                let insert = &plan.inserts();
                let Ty::Enc(rsid) = value.ty else { return no("boxed map entry") };
                let MapDims { universe: u, bitset_words: bw, slot_words: w } = self.map_dims(e.ty)?;
                let (n, buf) = self.result_buf(e.ty, "boxed map")?;
                let r = self.int_expr(rank)?;
                let b = self.expr(base)?;
                let bp = self.addr(buf);
                self.copy(bp, b, n);
                // Empty-tuple key types are checked only on a lookup miss.
                let cond = if *insert {
                    let c = self.cmp_imm(IntCC::UnsignedLessThan, r, u as i64);
                    self.b2i(c)
                } else {
                    let present = self.bit(bp, bw, r);
                    let inside = self.cmp_imm(IntCC::SignedLessThan, r, u as i64);
                    let inside = self.b2i(inside);
                    self.b.ins().band(present, inside)
                };
                let (yes, miss, after) = (self.b.create_block(), self.b.create_block(), self.b.create_block());
                self.brif(cond, yes, &[], miss, &[]);
                self.switch(yes);
                let v = self.encoded(value, rsid)?;
                let bp = self.addr(buf);
                self.set_bit(bp, r);
                let p = self.offset(bp, bw);
                let dst = self.index(p, r, w);
                self.copy(dst, v, w);
                self.b.ins().jump(after, &[]);
                self.switch(miss);
                if *insert {
                    self.fail();
                } else {
                    let ranks = &plan.reject_when_empty;
                    if !ranks.is_empty() {
                        let mut non_int = self.iconst(0);
                        for range in ranks.iter() {
                            let lo = self.cmp_imm(IntCC::SignedGreaterThanOrEqual, r, range.start as i64);
                            let hi = self.cmp_imm(IntCC::SignedLessThan, r, range.end as i64);
                            let in_range = self.b.ins().band(lo, hi);
                            let in_range = self.b2i(in_range);
                            non_int = self.b.ins().bor(non_int, in_range);
                        }
                        let bp = self.addr(buf);
                        let mut bits = self.iconst(0);
                        for i in 0..bw {
                            let word = self.load(bp, i);
                            bits = self.b.ins().bor(bits, word);
                        }
                        let present = self.cmp_imm(IntCC::NotEqual, bits, 0);
                        let present = self.b2i(present);
                        let integral = self.b.ins().bxor_imm(non_int, 1);
                        let ok = self.b.ins().bor(present, integral);
                        self.check(ok);
                    }
                    self.b.ins().jump(after, &[]);
                }
                self.switch(after);
                self.addr(buf)
            }
            ExprKind::MapBuild { bind, set, rank, value } => {
                let Ty::Enc(rsid) = value.ty else { return no("boxed map entry") };
                let MapDims { universe: u, bitset_words: bw, slot_words: w } = self.map_dims(e.ty)?;
                let (n, buf) = self.result_buf(e.ty, "boxed map")?;
                let bp = self.addr(buf);
                self.zero(bp, n);
                let spec = self.spec_exprs(&[rank, value], *bind);
                self.foreach(*bind, set, None, None, spec, &mut |g| {
                    let r = g.int_expr(rank)?;
                    let ok = g.cmp_imm(IntCC::UnsignedLessThan, r, u as i64);
                    g.check(ok);
                    let v = g.encoded(value, rsid)?;
                    let bp = g.addr(buf);
                    g.set_bit(bp, r);
                    let p = g.offset(bp, bw);
                    let dst = g.index(p, r, w);
                    g.copy(dst, v, w);
                    Ok(())
                })?;
                self.addr(buf)
            }
            ExprKind::MapMerge(a, b) => {
                let MapDims { bitset_words: bw, slot_words: w, .. } = self.map_dims(e.ty)?;
                let (n, buf) = self.result_buf(e.ty, "boxed map")?;
                let (f, g_) = (self.expr(a)?, self.expr(b)?);
                let bp = self.addr(buf);
                self.copy(bp, g_, n);
                for i in 0..bw {
                    let (x, y) = (self.load(bp, i), self.load(f, i));
                    let z = self.b.ins().bor(x, y);
                    self.store(z, bp, i);
                }
                self.for_bits(f, bw, &mut |g, r| {
                    let bp = g.addr(buf);
                    let dp = g.offset(bp, bw);
                    let dst = g.index(dp, r, w);
                    let sp = g.offset(f, bw);
                    let src = g.index(sp, r, w);
                    g.copy(dst, src, w);
                    Ok(())
                })?;
                self.addr(buf)
            }
            _ => unreachable!("not a map expression"),
        })
    }
}
