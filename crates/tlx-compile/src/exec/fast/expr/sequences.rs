//! Value family of the closure compiler: slices, indexing, EXCEPT updates, the loop builders and the sequence operations
//! (moved verbatim from `encoded.rs`).

use tlx_layout::Shape;

use super::super::{move_words, Code, Loc};
use super::*;
use crate::ir::*;

impl<'c> Compiler<'c> {
    /// The value family.
    pub(super) fn seq_expr(&mut self, e: &Expr) -> C<Code> {
        let ty = e.ty;

        Ok(match &*e.kind {
            ExprKind::Slice { base, offset } => {
                let n = self.words(ty)?;
                let (b, _) = self.enc(base)?;
                let off = *offset as usize;
                match self.last {
                    Place::Fixed(l) => self.fixed(l.plus(off), n),
                    Place::Temp(o) => self.at_temp(o + off, n),
                    _ => Code::E(Box::new(move |fx| Ok(b.at(fx)?.plus(off))), n),
                }
            }
            ExprKind::Index { base, arg, map, elem_words } => {
                let (b, _) = self.enc(base)?;
                let known = self.known_loc();
                let slot = self.slot_in(arg, map)?;
                let n = *elem_words as usize;
                if let Some(l) = known {
                    return Ok(Code::E(
                        Box::new(move |fx| match slot(fx)? {
                            Some(s) => Ok(l.plus(s * n)),
                            None => err(NOT_IN_DOMAIN),
                        }),
                        n,
                    ));
                }
                Code::E(
                    Box::new(move |fx| {
                        let l = b.at(fx)?;
                        match slot(fx)? {
                            Some(s) => Ok(l.plus(s * n)),
                            None => err(NOT_IN_DOMAIN),
                        }
                    }),
                    n,
                )
            }
            ExprKind::ExceptAt { base, offset, value } => {
                let nb = self.words(ty)?;
                let (b, _) = self.enc(base)?;
                let place = self.last;
                let (v, nv) = self.enc(value)?;
                let out = self.out_slot(place, nb);
                let off = *offset as usize;
                self.fresh(
                    Box::new(move |fx| {
                        let l = b.at(fx)?;
                        fx.copy_to(l, out, nb);
                        let lv = v.at(fx)?;
                        fx.copy_to(lv, out + off, nv);
                        Ok(Loc::frame(out))
                    }),
                    nb,
                    out,
                )
            }
            ExprKind::ExceptIndex { base, arg, map, elem_words, value } => {
                let nb = self.words(ty)?;
                let (b, _) = self.enc(base)?;
                let place = self.last;
                let slot = self.slot_in(arg, map)?;
                let (v, _) = self.enc(value)?;
                let out = self.out_slot(place, nb);
                let n = *elem_words as usize;
                self.fresh(
                    Box::new(move |fx| {
                        let l = b.at(fx)?;
                        fx.copy_to(l, out, nb);
                        let Some(s) = slot(fx)? else { return Ok(Loc::frame(out)) };
                        let lv = v.at(fx)?;
                        fx.copy_to(lv, out + s * n, n);
                        Ok(Loc::frame(out))
                    }),
                    nb,
                    out,
                )
            }
            ExprKind::Filter { bind, set, pred } => self.filter(ty, *bind, set, pred)?,
            ExprKind::MapSet { bind, set, elem, cond } => self.map_set(ty, *bind, set, elem, cond.as_ref())?,
            ExprKind::Fold { product, bind, set, body } => {
                let src = self.src(set, false)?;
                let bind = self.slots[*bind as usize];
                let body = self.int_(body)?;
                let product = *product;
                Code::I(Box::new(move |fx| {
                    let mut acc: i64 = if product { 1 } else { 0 };
                    src.run(fx, bind, &mut |fx| {
                        if product && acc == 0 {
                            return Ok(false);
                        }
                        let v = body(fx)?;
                        acc = if product { arith::mul(acc, v)? } else { arith::add(acc, v)? };
                        Ok(true)
                    })?;
                    Ok(acc)
                }))
            }
            ExprKind::UnionFold { bind, set, body } => {
                let n = self.words(ty)?;
                let src = self.src(set, false)?;
                let bind = self.slots[*bind as usize];
                let (x, _) = self.enc(body)?;
                let out = self.alloc(n);
                self.fresh(
                    Box::new(move |fx| {
                        fx.fr.w[out..out + n].fill(0);
                        src.run(fx, bind, &mut |fx| {
                            let l = x.at(fx)?;
                            for i in 0..n {
                                let v = fx.word(l, i);
                                fx.fr.w[out + i] |= v;
                            }
                            Ok(true)
                        })?;
                        Ok(Loc::frame(out))
                    }),
                    n,
                    out,
                )
            }
            ExprKind::Collect { bind, set, elem } => {
                let n = self.words(ty)?;
                let Shape::Seq { elem: es, max_len } = self.shape(ty)? else { return Err("Collect into a non-sequence".into()) };
                let (ew, max_len) = (tlx_layout::layout::words(es), *max_len);
                let src = self.src(set, false)?;
                let bind = self.slots[*bind as usize];
                let (x, _) = self.enc(elem)?;
                let out = self.alloc(n);
                self.fresh(
                    Box::new(move |fx| {
                        fx.fr.w[out..out + n].fill(0);
                        let mut len = 0usize;
                        src.run(fx, bind, &mut |fx| {
                            let l = x.at(fx)?;
                            if len == max_len {
                                return err(SEQ_TOO_LONG);
                            }
                            fx.copy_to(l, out + 1 + len * ew, ew);
                            len += 1;
                            Ok(true)
                        })?;
                        fx.fr.w[out] = len as u64;
                        Ok(Loc::frame(out))
                    }),
                    n,
                    out,
                )
            }

            ExprKind::SeqLen(s) => {
                let (s, _) = self.enc(s)?;
                Code::I(Box::new(move |fx| {
                    let l = s.at(fx)?;
                    Ok(fx.word(l, 0) as i64)
                }))
            }
            ExprKind::SeqIndex { base, arg, elem_words } => {
                let (b, _) = self.enc(base)?;
                let i = self.int_(arg)?;
                let k = *elem_words as usize;
                Code::E(
                    Box::new(move |fx| {
                        let l = b.at(fx)?;
                        let i = i(fx)?;
                        if i < 1 || i as u64 > fx.word(l, 0) {
                            return err("Attempted to apply a sequence to an index outside its domain.");
                        }
                        Ok(l.plus(1 + (i as usize - 1) * k))
                    }),
                    k,
                )
            }
            ExprKind::SeqTail { base, elem_words } => {
                let n = self.words(ty)?;
                let (b, _) = self.enc(base)?;
                let k = *elem_words as usize;
                let out = self.out_slot(self.last, n);
                self.fresh(
                    Box::new(move |fx| {
                        let l = b.at(fx)?;
                        fx.copy_to(l, out, n);
                        let w = &mut fx.fr.w[out..out + n];
                        let len = w[0] as usize;
                        if len == 0 {
                            return err("Attempted to apply Tail to the empty sequence.");
                        }
                        w.copy_within(1 + k..1 + len * k, 1);
                        w[1 + (len - 1) * k..1 + len * k].fill(0);
                        w[0] -= 1;
                        Ok(Loc::frame(out))
                    }),
                    n,
                    out,
                )
            }
            ExprKind::SeqAppend { base, value, elem_words, max_len } => {
                let n = self.words(ty)?;
                let (b, _) = self.enc(base)?;
                let place = self.last;
                let (v, _) = self.enc(value)?;
                let (k, max_len) = (*elem_words as usize, *max_len as usize);
                let out = self.out_slot(place, n);
                self.fresh(
                    Box::new(move |fx| {
                        let l = b.at(fx)?;
                        fx.copy_to(l, out, n);
                        let lv = v.at(fx)?;
                        let len = fx.fr.w[out] as usize;
                        if len >= max_len {
                            return err(SEQ_TOO_LONG);
                        }
                        fx.copy_to(lv, out + 1 + len * k, k);
                        fx.fr.w[out] += 1;
                        Ok(Loc::frame(out))
                    }),
                    n,
                    out,
                )
            }
            ExprKind::SeqSub { base, lo, hi, elem_words } => {
                let n = self.words(ty)?;
                let (b, _) = self.enc(base)?;
                let (lo, hi) = (self.int_(lo)?, self.int_(hi)?);
                let k = *elem_words as usize;
                let stage = self.alloc(n);
                let out = self.alloc(n);
                self.fresh(
                    Box::new(move |fx| {
                        let l = b.at(fx)?;
                        fx.copy_to(l, stage, n);
                        let (m, h) = (lo(fx)?, hi(fx)?);
                        let len = fx.fr.w[stage] as i64;
                        fx.fr.w[out..out + n].fill(0);
                        if h >= m {
                            if m < 1 || h > len {
                                return err("SubSeq out of range");
                            }
                            let c = (h - m + 1) as usize;
                            let start = stage + 1 + (m as usize - 1) * k;
                            fx.fr.w.copy_within(start..start + c * k, out + 1);
                            fx.fr.w[out] = c as u64;
                        }
                        Ok(Loc::frame(out))
                    }),
                    n,
                    out,
                )
            }
            ExprKind::SeqFilter { bind, seq, pred, elem_words } => {
                let n = self.words(ty)?;
                let (s, _) = self.enc(seq)?;
                let k = *elem_words as usize;
                let bind = self.slots[*bind as usize];
                let Slot::W(boff, _) = bind else { return Err("SeqFilter binding a non-encoded temp".into()) };
                let pred = self.bool_(pred)?;
                let stage = self.alloc(n);
                let out = self.alloc(n);
                self.fresh(
                    Box::new(move |fx| {
                        let l = s.at(fx)?;
                        fx.copy_to(l, stage, n);
                        fx.fr.w[out..out + n].fill(0);
                        let mut len = 0usize;
                        for i in 0..fx.fr.w[stage] as usize {
                            let at = stage + 1 + i * k;
                            move_words(&mut fx.fr.w, at, k, boff);
                            if pred(fx)? {
                                move_words(&mut fx.fr.w, at, k, out + 1 + len * k);
                                len += 1;
                            }
                        }
                        fx.fr.w[out] = len as u64;
                        Ok(Loc::frame(out))
                    }),
                    n,
                    out,
                )
            }
            ExprKind::SeqConcat { a, b, elem_words, max_len } => {
                let n = self.words(ty)?;
                let (a, _) = self.enc(a)?;
                let (b, nb) = self.enc(b)?;
                let (k, max_len) = (*elem_words as usize, *max_len as usize);
                let out = self.alloc(n);
                self.fresh(
                    Box::new(move |fx| {
                        let la = a.at(fx)?;
                        fx.copy_to(la, out, n);
                        let lb = b.at(fx)?;
                        let (x, y) = (fx.fr.w[out] as usize, fx.word(lb, 0) as usize);
                        if x + y > max_len {
                            return err(SEQ_TOO_LONG);
                        }
                        debug_assert!(y * k < nb);
                        fx.copy_to(lb.plus(1), out + 1 + x * k, y * k);
                        fx.fr.w[out] = (x + y) as u64;
                        Ok(Loc::frame(out))
                    }),
                    n,
                    out,
                )
            }
            _ => unreachable!("not an encoded-value expression"),
        })
    }
}
