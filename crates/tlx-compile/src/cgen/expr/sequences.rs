//! C generation of the value family of encoded expressions: slices, indexing, EXCEPT updates, the loop builders and the
//! sequence operations (moved verbatim from `encoded.rs`).

use tlx_layout::layout::bitset_universe;
use tlx_layout::Shape;

use super::super::{no, temp, CGen, R};
use crate::ir::*;

impl CGen<'_> {
    /// The value family.
    pub(super) fn seq_expr(&mut self, e: &Expr) -> R<String> {
        Ok(match &*e.kind {
            ExprKind::Slice { base, offset } => format!("({} + {offset})", self.expr(base)?),
            ExprKind::Index { base, arg, map, elem_words } => {
                let b = self.expr(base)?;
                let slot = self.slot(arg, map)?;
                format!("({b} + ({slot}) * {elem_words})")
            }
            ExprKind::ExceptAt { base, offset, value } => {
                let Ty::Enc(bsid) = e.ty else { return no("boxed EXCEPT") };
                let Ty::Enc(vsid) = value.ty else { return no("boxed EXCEPT value") };
                let (n, vn) = (self.words(bsid), self.words(vsid));
                let buf = self.buf(n);
                let b = self.expr(base)?;
                let v = self.expr(value)?;
                format!("({{ const uint64_t *v_ = {v}; memcpy({buf}, {b}, {n} * 8); memcpy({buf} + {offset}, v_, {vn} * 8); {buf}; }})")
            }
            ExprKind::ExceptIndex { base, arg, map, elem_words, value } => {
                let (n, buf) = self.result_buf(e.ty, "boxed EXCEPT")?;
                let b = self.expr(base)?;
                let slot = self.slot_or(arg, map, "-1")?;
                let v = self.expr(value)?;
                format!(
                    "({{ int64_t sl_ = {slot}; memcpy({buf}, {b}, {n} * 8); if (sl_ >= 0) {{ const uint64_t *v_ = {v}; memcpy({buf} + sl_ * {elem_words}, v_, {elem_words} * 8); }} {buf}; }})"
                )
            }
            ExprKind::SeqLen(x) => format!("((int64_t)({})[0])", self.expr(x)?),
            ExprKind::Filter { bind, set, pred } => {
                let Ty::Enc(sid) = e.ty else { return no("boxed filter") };
                let Shape::Set { elem, .. } = self.ty.shape(sid).clone() else { return no("filter over a non-set") };
                let n = self.words(sid);
                let buf = self.buf(n);
                let h0 = self.handles;
                let p = self.bool_expr(pred)?;
                let p = self.scoped_scalar(&p, h0);
                let t = temp(*bind);
                let s_ = self.expr(set)?;
                let k = self.fresh();
                if bitset_universe(&elem).is_some() {
                    let (kk, digits) = self.unrank_code(&elem, k)?;
                    let mask = self.mask_of(&elem, n, *bind, Some(pred), k);
                    format!(
                        "({{ const uint64_t *S{k} = {s_}; memset({buf}, 0, {n} * 8); for (uint32_t w{k} = 0; w{k} < {n}; w{k}++) {{ uint64_t B{k} = S{k}[w{k}]{mask}; \
                         while (B{k}) {{ uint64_t r{k} = w{k} * 64 + (uint64_t)__builtin_ctzll(B{k}); B{k} &= B{k} - 1; uint64_t q{k} = r{k}; uint64_t R{k}[{kk}]; {digits}const uint64_t *{t} = R{k}; \
                         if ({p}) {buf}[r{k} >> 6] |= 1ULL << (r{k} & 63); }} }} (const uint64_t *){buf}; }})"
                    )
                } else {
                    let esid = self.find_sid(&elem)?;
                    let en = self.words(esid);
                    format!(
                        "({{ const uint64_t *S{k} = {s_}; memset({buf}, 0, {n} * 8); uint64_t L{k} = 0; for (uint64_t i{k} = 0; i{k} < S{k}[0]; i{k}++) {{ \
                         const uint64_t *{t} = S{k} + 1 + i{k} * {en}; if ({p}) {{ memcpy({buf} + 1 + L{k} * {en}, {t}, {en} * 8); L{k}++; }} }} {buf}[0] = L{k}; (const uint64_t *){buf}; }})"
                    )
                }
            }
            ExprKind::MapSet { bind, set, elem, cond } => {
                let Ty::Enc(sid) = e.ty else { return no("boxed set") };
                let Shape::Set { elem: es, max_card } = self.ty.shape(sid).clone() else { return no("map into a non-set") };
                let n = self.words(sid);
                let buf = self.buf(n);
                let h0 = self.loop_start(set)?;
                let body = if let Some(u) = bitset_universe(&es) {
                    let r = self.int_expr(elem)?;
                    format!("      {{ int64_t r_ = {r}; if (r_ < 0 || r_ >= {u}) return TLX_ERR; {buf}[r_ >> 6] |= 1ULL << (r_ & 63); }}\n")
                } else {
                    let x = self.expr(elem)?;
                    let ew = self.words(self.find_sid(&es)?);
                    format!("      if (!tlx_sorted_add({buf}, {x}, {ew}, {max_card})) return TLX_ERR;\n")
                };
                let body = match cond {
                    Some(c) => format!("      if ({}) {{\n{body}      }}\n", self.bool_expr(c)?),
                    None => body,
                };
                let lp = self.foreach(*bind, set, &body, 3, h0, None, cond.as_ref())?;
                format!("({{ memset({buf}, 0, {n} * 8); {{\n{lp} }} (const uint64_t *){buf}; }})")
            }
            ExprKind::Fold { product, bind, set, body } => {
                let h0 = self.loop_start(set)?;
                let r = format!("q{}", self.fresh());
                let b = self.int_expr(body)?;
                let inner = if *product {
                    format!("      if ({r} == 0) goto {r}_done; {{ int64_t v_ = {b}; if (!tlx_mul({r}, v_, &{r})) return TLX_ERR; }}\n")
                } else {
                    format!("      {{ int64_t v_ = {b}; if (!tlx_add({r}, v_, &{r})) return TLX_ERR; }}\n")
                };
                let lp = self.foreach(*bind, set, &inner, 3, h0, None, None)?;
                format!("({{ int64_t {r} = {}; {{\n{lp} }} {r}_done: ; {r}; }})", *product as i32)
            }
            ExprKind::UnionFold { bind, set, body } => {
                let Ty::Enc(sid) = e.ty else { return no("boxed union") };
                let n = self.words(sid);
                let buf = self.buf(n);
                let h0 = self.loop_start(set)?;
                let b = self.expr(body)?;
                let k = self.fresh();
                let inner = format!(
                    "      {{ const uint64_t *u{k}_ = {b}; for (uint32_t i{k}_ = 0; i{k}_ < {n}; i{k}_++) {buf}[i{k}_] |= u{k}_[i{k}_]; }}\n"
                );
                let lp = self.foreach(*bind, set, &inner, 3, h0, None, None)?;
                format!("({{ memset({buf}, 0, {n} * 8); {{\n{lp} }} (const uint64_t *){buf}; }})")
            }
            ExprKind::Collect { bind, set, elem } => {
                let Ty::Enc(sid) = e.ty else { return no("boxed sequence") };
                let Shape::Seq { elem: es, max_len } = self.ty.shape(sid).clone() else { return no("collect into a non-sequence") };
                let ew = self.words(self.find_sid(&es)?);
                let n = self.words(sid);
                let buf = self.buf(n);
                let h0 = self.loop_start(set)?;
                let x = self.expr(elem)?;
                let body = format!("      {{ const uint64_t *x_ = {x}; if ({buf}[0] >= {max_len}) return TLX_ERR; memcpy({buf} + 1 + {buf}[0] * {ew}, x_, {ew} * 8); {buf}[0]++; }}\n");
                let lp = self.foreach(*bind, set, &body, 3, h0, None, None)?;
                format!("({{ memset({buf}, 0, {n} * 8); {{\n{lp} }} (const uint64_t *){buf}; }})")
            }
            ExprKind::SeqIndex { base, arg, elem_words } => {
                let b = self.expr(base)?;
                let i = self.int_expr(arg)?;
                format!("({{ const uint64_t *s_ = {b}; int64_t i_ = {i}; if (i_ < 1 || (uint64_t)i_ > s_[0]) return TLX_ERR; s_ + 1 + (i_ - 1) * {elem_words}; }})")
            }
            ExprKind::SeqTail { base, elem_words } => {
                let (n, buf) = self.result_buf(e.ty, "boxed sequence")?;
                let b = self.expr(base)?;
                format!(
                    "({{ const uint64_t *s_ = {b}; if (s_[0] == 0) return TLX_ERR; memcpy({buf}, s_, {n} * 8); uint64_t l_ = s_[0]; memmove({buf} + 1, {buf} + 1 + {elem_words}, (l_ - 1) * {elem_words} * 8); memset({buf} + 1 + (l_ - 1) * {elem_words}, 0, {elem_words} * 8); {buf}[0] = l_ - 1; (const uint64_t *){buf}; }})"
                )
            }
            ExprKind::SeqSub { base, lo, hi, elem_words } => {
                let (n, buf) = self.result_buf(e.ty, "boxed sequence")?;
                let b = self.expr(base)?;
                let (m, k) = (self.int_expr(lo)?, self.int_expr(hi)?);
                format!(
                    "({{ const uint64_t *s_ = {b}; int64_t m_ = {m}, n_ = {k}; memset({buf}, 0, {n} * 8); \
                     if (n_ >= m_) {{ if (m_ < 1 || n_ > (int64_t)s_[0]) return TLX_ERR; memcpy({buf} + 1, s_ + 1 + (m_ - 1) * {elem_words}, (n_ - m_ + 1) * {elem_words} * 8); {buf}[0] = n_ - m_ + 1; }} (const uint64_t *){buf}; }})"
                )
            }
            ExprKind::SeqFilter { bind, seq, pred, elem_words } => {
                let (n, buf) = self.result_buf(e.ty, "boxed sequence")?;
                let s_ = self.expr(seq)?;
                let h0 = self.handles;
                let p = self.bool_expr(pred)?;
                let p = self.scoped_scalar(&p, h0);
                let t = temp(*bind);
                let k = self.fresh();
                format!(
                    "({{ const uint64_t *S{k} = {s_}; memset({buf}, 0, {n} * 8); uint64_t L{k} = 0; for (uint64_t i{k} = 0; i{k} < S{k}[0]; i{k}++) {{ \
                     const uint64_t *{t} = S{k} + 1 + i{k} * {elem_words}; if ({p}) {{ memcpy({buf} + 1 + L{k} * {elem_words}, {t}, {elem_words} * 8); L{k}++; }} }} {buf}[0] = L{k}; (const uint64_t *){buf}; }})"
                )
            }
            ExprKind::SeqConcat { a, b, elem_words, max_len } => {
                let (n, buf) = self.result_buf(e.ty, "boxed sequence")?;
                let (a, b) = (self.expr(a)?, self.expr(b)?);
                format!(
                    "({{ const uint64_t *a_ = {a}; const uint64_t *b_ = {b}; if (a_[0] + b_[0] > {max_len}) return TLX_ERR; memcpy({buf}, a_, {n} * 8); memcpy({buf} + 1 + a_[0] * {elem_words}, b_ + 1, b_[0] * {elem_words} * 8); {buf}[0] = a_[0] + b_[0]; (const uint64_t *){buf}; }})"
                )
            }
            ExprKind::SeqAppend { base, value, elem_words, max_len } => {
                let (n, buf) = self.result_buf(e.ty, "boxed sequence")?;
                let b = self.expr(base)?;
                let v = self.expr(value)?;
                format!(
                    "({{ const uint64_t *s_ = {b}; const uint64_t *v_ = {v}; if (s_[0] >= {max_len}) return TLX_ERR; memcpy({buf}, s_, {n} * 8); memcpy({buf} + 1 + s_[0] * {elem_words}, v_, {elem_words} * 8); {buf}[0] = s_[0] + 1; (const uint64_t *){buf}; }})"
                )
            }
            _ => unreachable!("not an encoded-value expression"),
        })
    }
}
