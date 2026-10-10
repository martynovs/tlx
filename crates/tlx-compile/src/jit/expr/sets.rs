//! Cranelift generation of the set family of encoded expressions: bit sets, sorted sets, the
//! ranks, the recodes, union field access and the boxed builders (moved verbatim from
//! `encoded.rs`).

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, Value};

use super::super::gen::{Gen, Known, MAX_SSA_WORDS};
use super::super::{no, R};
use crate::ir::encoding::{RecodeTable, UnionArms};
use crate::ir::*;

impl Gen<'_, '_, '_> {
    /// The set family.
    pub(super) fn set_expr(&mut self, e: &Expr) -> R<Value> {
        Ok(match &*e.kind {
            ExprKind::BitMember { set, rank } => {
                let Ty::Enc(sid) = set.ty else { return no("boxed membership") };
                let n = self.words_of(sid);
                let (s, r) = (self.expr(set)?, self.int_expr(rank)?);
                self.bit(s, n, r)
            }
            ExprKind::BitSetOp(op, a, b) => {
                let (n, buf) = self.result_buf(e.ty, "boxed set")?;
                let (x, y) = (self.expr(a)?, self.expr(b)?);
                let mut ws = Vec::with_capacity(n);
                for i in 0..n {
                    let (u, v) = (self.load(x, i), self.load(y, i));
                    ws.push(match op {
                        SetOp::Union => self.b.ins().bor(u, v),
                        SetOp::Inter => self.b.ins().band(u, v),
                        SetOp::Diff => self.b.ins().band_not(u, v),
                    });
                }
                if n <= MAX_SSA_WORDS {
                    let ws: Vec<Known> = ws.into_iter().map(Known::Val).collect();
                    return Ok(self.materialize(&ws));
                }
                let bp = self.addr(buf);
                for (i, w) in ws.into_iter().enumerate() {
                    self.store(w, bp, i);
                }
                bp
            }
            ExprKind::BitCard(s) => {
                let Ty::Enc(sid) = s.ty else { return no("boxed cardinality") };
                let n = self.words_of(sid);
                let p = self.expr(s)?;
                let mut acc = self.iconst(0);
                for i in 0..n {
                    let w = self.load(p, i);
                    let c = self.b.ins().popcnt(w);
                    acc = self.b.ins().iadd(acc, c);
                }
                acc
            }
            ExprKind::Rank(x) => {
                let p = self.expr(x)?;
                if matches!(x.ty, Ty::Enc(sid) if self.words_of(sid) == 0) {
                    // A zero-word encoding (a set over an empty universe)
                    // has a single value: rank 0.
                    self.iconst(0)
                } else {
                    self.load(p, 0)
                }
            }
            ExprKind::Concat(xs) => {
                let (total, buf) = self.result_buf(e.ty, "boxed construction")?;
                let mut parts = Vec::new();
                for x in xs {
                    let Ty::Enc(xs_) = x.ty else { return no("boxed part") };
                    let n = self.words_of(xs_);
                    parts.push((self.expr(x)?, n));
                }
                if total <= MAX_SSA_WORDS {
                    let mut ws = Vec::with_capacity(total);
                    for (v, n) in parts {
                        ws.extend(self.words(v, n));
                    }
                    return Ok(self.materialize(&ws));
                }
                let bp = self.addr(buf);
                let mut off = 0;
                for (v, n) in parts {
                    let dst = self.offset(bp, off);
                    self.copy(dst, v, n);
                    off += n;
                }
                bp
            }
            ExprKind::BitSet(rs) => {
                let (n, buf) = self.result_buf(e.ty, "boxed set")?;
                let u = self.universe(e.ty)?;
                let bp = self.addr(buf);
                self.zero(bp, n);
                for r in rs {
                    let r = self.int_expr(r)?;
                    let ok = self.cmp_imm(IntCC::UnsignedLessThan, r, u as i64);
                    self.check(ok);
                    let bp = self.addr(buf);
                    self.set_bit(bp, r);
                }
                self.addr(buf)
            }
            ExprKind::UnionField { base, offsets } => {
                let items: Vec<i32> = offsets.iter().map(|o| o.map_or(-1, |x| x as i32)).collect();
                let tbl = self.cx.statics.i32s(&items);
                let u = self.expr(base)?;
                let o = self.table_i32(u, tbl, &items);
                let ok = self.cmp_imm(IntCC::SignedGreaterThanOrEqual, o, 0);
                self.check(ok);
                let p = self.offset(u, 1);
                self.index(p, o, 1)
            }
            ExprKind::UnionFieldInt { base, entries } => {
                let offs: Vec<i32> = entries.iter().map(|o| o.map_or(-1, |x| x.0 as i32)).collect();
                let los: Vec<i64> = entries.iter().map(|o| o.map_or(0, |x| x.1)).collect();
                let (to, tl) = (self.cx.statics.i32s(&offs), self.cx.statics.i64s(&los));
                let u = self.expr(base)?;
                let o = self.table_i32(u, to, &offs);
                let ok = self.cmp_imm(IntCC::SignedGreaterThanOrEqual, o, 0);
                self.check(ok);
                let p = self.offset(u, 1);
                let p = self.index(p, o, 1);
                let w = self.load(p, 0);
                let tag = self.load(u, 0);
                match self.konst(tag).and_then(|t| los.get(t as usize)) {
                    Some(lo) => self.add_imm(w, *lo),
                    None => {
                        let tlv = self.iconst(tl);
                        let lp = self.index(tlv, tag, 1);
                        let lo = self.load(lp, 0);
                        self.b.ins().iadd(w, lo)
                    }
                }
            }
            ExprKind::SortedMember { set, elem, elem_words } => {
                let (s, x) = (self.expr(set)?, self.expr(elem)?);
                let n = self.iconst(*elem_words as i64);
                self.call_rt(super::super::rt::sorted_member as *const u8, &[s, x, n])
            }
            ExprKind::Recode { x, arm_base, table, words } => {
                let v = self.expr(x)?;
                let tbl = self.cx.statics.words(table);
                let words = *words as usize;
                let codes = RecodeTable { arm_base, table, words }.codes() as i64;
                let code = if arm_base.is_empty() {
                    self.load(v, 0)
                } else {
                    let base = self.cx.statics.words(arm_base);
                    let tag = self.load(v, 0);
                    let inside = self.cmp_imm(IntCC::UnsignedLessThan, tag, arm_base.len() as i64);
                    let (yes, no_) = (self.b.create_block(), self.b.create_block());
                    let (j, r) = self.join();
                    self.brif(inside, yes, &[], no_, &[]);
                    self.switch(yes);
                    let bv = self.iconst(base);
                    let bp = self.index(bv, tag, 1);
                    let b0 = self.load(bp, 0);
                    let x1 = self.load(v, 1);
                    let c = self.b.ins().iadd(b0, x1);
                    self.b.ins().jump(j, &[c.into()]);
                    self.switch(no_);
                    let m = self.iconst(-1);
                    self.b.ins().jump(j, &[m.into()]);
                    self.switch(j);
                    r
                };
                let ok = self.cmp_imm(IntCC::UnsignedLessThan, code, codes);
                self.check(ok);
                let p = match self.konst(code).filter(|c| (0..codes).contains(c)) {
                    Some(c) => {
                        // A known entry of the table: its words are known.
                        let at = c as usize * words;
                        let p = self.iconst(tbl + (at * 8) as i64);
                        self.know(p, table[at..at + words].iter().map(|w| Known::Const(*w)).collect());
                        p
                    }
                    None => {
                        let tv = self.iconst(tbl);
                        self.index(tv, code, words)
                    }
                };
                let w0 = self.load(p, 0);
                let ok = self.cmp_imm(IntCC::NotEqual, w0, -1);
                self.check(ok);
                p
            }
            ExprKind::RankUnion { x, arms } => {
                let v = self.expr(x)?;
                let tag = self.load(v, 0);
                let (j, r) = self.join();
                for (i, (base, radices)) in UnionArms::new(arms).enumerate() {
                    let (yes, next) = (self.b.create_block(), self.b.create_block());
                    let c = self.cmp_imm(IntCC::Equal, tag, i as i64);
                    self.brif(c, yes, &[], next, &[]);
                    self.switch(yes);
                    // The arm offset plus the mixed-radix rank within the arm.
                    let mut acc = self.iconst(0);
                    for (k, c) in radices.iter().enumerate() {
                        let d = self.load(v, k + 1);
                        let m = self.b.ins().imul_imm(acc, *c as i64);
                        acc = self.b.ins().iadd(m, d);
                    }
                    let res = self.b.ins().iadd_imm(acc, base as i64);
                    self.b.ins().jump(j, &[res.into()]);
                    self.switch(next);
                }
                self.fail();
                self.switch(j);
                r
            }
            ExprKind::Lookup { x, table } => {
                let tbl = self.cx.statics.i32s(table);
                let v = self.expr(x)?;
                self.table_i32(v, tbl, table)
            }
            ExprKind::RankFlat { x, radices } => {
                let v = self.expr(x)?;
                let mut acc = self.iconst(0);
                for (i, c) in radices.iter().enumerate() {
                    let d = self.load(v, i);
                    let m = self.b.ins().imul_imm(acc, *c as i64);
                    acc = self.b.ins().iadd(m, d);
                }
                acc
            }
            ExprKind::SortedInsert { set, elem, elem_words, max_card } => {
                let (n, buf) = self.result_buf(e.ty, "boxed set")?;
                let (s, x) = (self.expr(set)?, self.expr(elem)?);
                let bp = self.addr(buf);
                let (nw, k, m) = (self.iconst(n as i64), self.iconst(*elem_words as i64), self.iconst(*max_card as i64));
                let ok = self.call_rt(super::super::rt::sorted_insert as *const u8, &[bp, nw, s, x, k, m]);
                self.check(ok);
                bp
            }
            ExprKind::SortedOp { op, a, b, elem_words, max_card } => {
                let opn = match op {
                    SetOp::Union => 0,
                    SetOp::Inter => 1,
                    SetOp::Diff => 2,
                };
                let (n, buf) = self.result_buf(e.ty, "boxed set")?;
                let (a, b) = (self.expr(a)?, self.expr(b)?);
                let bp = self.addr(buf);
                let (o, nw, k, m) =
                    (self.iconst(opn), self.iconst(n as i64), self.iconst(*elem_words as i64), self.iconst(*max_card as i64));
                let ok = self.call_rt(super::super::rt::sorted_op as *const u8, &[o, bp, nw, a, b, k, m]);
                self.check(ok);
                bp
            }
            _ => unreachable!("not an encoded-value expression"),
        })
    }
}
