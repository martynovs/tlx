//! C generation of the set family of encoded expressions: bit sets, sorted sets, the ranks,
//! the recodes, union field access and the boxed builders (moved verbatim from
//! `encoded.rs`).

use super::super::{no, CGen, R};
use crate::ir::encoding::{RecodeTable, UnionArms};
use crate::ir::*;

impl CGen<'_> {
    /// The set family.
    pub(super) fn set_expr(&mut self, e: &Expr) -> R<String> {
        Ok(match &*e.kind {
            ExprKind::BitMember { set, rank } => {
                let Ty::Enc(sid) = set.ty else { return no("boxed membership") };
                let n = self.words(sid);
                format!("tlx_bit({}, {n}, {})", self.expr(set)?, self.int_expr(rank)?)
            }
            ExprKind::BitSetOp(op, a, b) => {
                let (n, buf) = self.result_buf(e.ty, "boxed set")?;
                let o = match op {
                    SetOp::Union => "x_[i_] | y_[i_]",
                    SetOp::Inter => "x_[i_] & y_[i_]",
                    SetOp::Diff => "x_[i_] & ~y_[i_]",
                };
                format!(
                    "({{ const uint64_t *x_ = {}, *y_ = {}; for (int i_ = 0; i_ < {n}; i_++) {buf}[i_] = {o}; {buf}; }})",
                    self.expr(a)?,
                    self.expr(b)?
                )
            }
            ExprKind::BitCard(s) => {
                let Ty::Enc(sid) = s.ty else { return no("boxed cardinality") };
                let n = self.words(sid);
                format!("tlx_card({}, {n})", self.expr(s)?)
            }
            ExprKind::Rank(x) => {
                // A zero-word encoding (a set over an empty universe) has a
                // single value: rank 0.
                if matches!(x.ty, Ty::Enc(sid) if self.words(sid) == 0) {
                    format!("((void)({}), (int64_t)0)", self.expr(x)?)
                } else {
                    format!("((int64_t)({})[0])", self.expr(x)?)
                }
            }
            ExprKind::Concat(xs) => {
                let (_, buf) = self.result_buf(e.ty, "boxed construction")?;
                let mut code = String::from("({ ");
                let mut off = 0;
                for x in xs {
                    let Ty::Enc(xs_) = x.ty else { return no("boxed part") };
                    let n = self.words(xs_);
                    let v = self.expr(x)?;
                    code.push_str(&format!("memcpy({buf} + {off}, {v}, {n} * 8); "));
                    off += n;
                }
                code.push_str(&format!("(const uint64_t *){buf}; }})"));
                code
            }
            ExprKind::BitSet(rs) => {
                let (n, buf) = self.result_buf(e.ty, "boxed set")?;
                let u = self.universe(e.ty)?;
                let mut code = format!("({{ memset({buf}, 0, {n} * 8); ");
                for r in rs {
                    let r = self.int_expr(r)?;
                    code.push_str(&format!(
                        "{{ int64_t r_ = {r}; if (r_ < 0 || r_ >= {u}) return TLX_ERR; {buf}[r_ >> 6] |= 1ULL << (r_ & 63); }} "
                    ));
                }
                code.push_str(&format!("(const uint64_t *){buf}; }})"));
                code
            }
            ExprKind::UnionField { base, offsets } => {
                let tbl = format!("uo{}", self.fresh_static());
                let items: Vec<String> = offsets.iter().map(|o| o.map_or("-1".to_string(), |x| x.to_string())).collect();
                self.static_array(&tbl, "int32_t", &items, "");
                let b = self.expr(base)?;
                format!("({{ const uint64_t *u_ = {b}; int32_t o_ = u_[0] < {} ? {tbl}[u_[0]] : -1; if (o_ < 0) return TLX_ERR; u_ + 1 + o_; }})", items.len())
            }
            ExprKind::UnionFieldInt { base, entries } => {
                let k = self.fresh_static();
                let (to, tl) = (format!("ufo{k}"), format!("ufl{k}"));
                let offs: Vec<String> = entries.iter().map(|o| o.map_or("-1".to_string(), |x| x.0.to_string())).collect();
                let los: Vec<String> = entries.iter().map(|o| o.map_or("0".to_string(), |x| format!("{}LL", x.1))).collect();
                self.static_array(&to, "int32_t", &offs, "");
                self.static_array(&tl, "int64_t", &los, "");
                let b = self.expr(base)?;
                format!("({{ const uint64_t *u_ = {b}; int32_t o_ = u_[0] < {} ? {to}[u_[0]] : -1; if (o_ < 0) return TLX_ERR; (int64_t)u_[1 + o_] + {tl}[u_[0]]; }})", offs.len())
            }
            ExprKind::SortedMember { set, elem, elem_words } => {
                let (s_, x) = (self.expr(set)?, self.expr(elem)?);
                format!("tlx_sorted_member({s_}, {x}, {elem_words})")
            }
            ExprKind::Recode { x, arm_base, table, words } => {
                let v = self.expr(x)?;
                let tbl = self.static_words(table);
                let codes = RecodeTable { arm_base, table, words: *words as usize }.codes();
                let code = if arm_base.is_empty() {
                    "x_[0]".to_string()
                } else {
                    let base = self.static_words(arm_base);
                    format!("(x_[0] < {} ? {base}[x_[0]] + x_[1] : (uint64_t)-1)", arm_base.len())
                };
                format!("({{ const uint64_t *x_ = {v}; uint64_t c_ = {code}; if (c_ >= {codes} || {tbl}[c_ * {words}] == (uint64_t)-1) return TLX_ERR; {tbl} + c_ * {words}; }})")
            }
            ExprKind::RankUnion { x, arms } => {
                let v = self.expr(x)?;
                let mut code = format!("({{ const uint64_t *x_ = {v}; uint64_t r_ = 0; switch (x_[0]) {{ ");
                for (i, (base, radices)) in UnionArms::new(arms).enumerate() {
                    code.push_str(&format!("case {i}: r_ = {base}ULL; "));
                    // The arm offset plus the mixed-radix rank within the arm.
                    let mut inner = String::from("0ULL");
                    for (j, c) in radices.iter().enumerate() {
                        inner = format!("(({inner}) * {c}ULL + x_[{}])", j + 1);
                    }
                    code.push_str(&format!("r_ += {inner}; break; "));
                }
                code.push_str("default: return TLX_ERR; } (int64_t)r_; })");
                code
            }
            ExprKind::Lookup { x, table } => {
                let v = self.expr(x)?;
                let tbl = format!("tbl{}", self.fresh_static());
                let items: Vec<String> = table.iter().map(|x| x.to_string()).collect();
                self.static_array(&tbl, "int32_t", &items, "-1");
                format!("({{ uint64_t r_ = ({v})[0]; r_ < {} ? (int64_t){tbl}[r_] : (int64_t)-1; }})", table.len())
            }
            ExprKind::RankFlat { x, radices } => {
                let v = self.expr(x)?;
                let mut code = format!("({{ const uint64_t *x_ = {v}; uint64_t r_ = 0; ");
                for (i, c) in radices.iter().enumerate() {
                    code.push_str(&format!("r_ = r_ * {c}ULL + x_[{i}]; "));
                }
                code.push_str("(int64_t)r_; })");
                code
            }
            ExprKind::SortedInsert { set, elem, elem_words, max_card } => {
                let (n, buf) = self.result_buf(e.ty, "boxed set")?;
                let (s_, x) = (self.expr(set)?, self.expr(elem)?);
                format!("({{ if (!tlx_sorted_insert({buf}, {n}, {s_}, {x}, {elem_words}, {max_card})) return TLX_ERR; (const uint64_t *){buf}; }})")
            }
            ExprKind::SortedOp { op, a, b, elem_words, max_card } => {
                let opn = match op {
                    SetOp::Union => 0,
                    SetOp::Inter => 1,
                    SetOp::Diff => 2,
                };
                let (n, buf) = self.result_buf(e.ty, "boxed set")?;
                let (a, b) = (self.expr(a)?, self.expr(b)?);
                format!("({{ if (!tlx_sorted_op({opn}, {buf}, {n}, {a}, {b}, {elem_words}, {max_card})) return TLX_ERR; (const uint64_t *){buf}; }})")
            }
            _ => unreachable!("not an encoded-value expression"),
        })
    }
}
