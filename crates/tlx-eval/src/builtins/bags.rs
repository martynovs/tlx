//! The `Bags`/`BagsExt` families (`Bags.java`, `BagsExt.java`):
//! `BagOfAll` and `FoldBag`, which build on the value helpers in the
//! crate's `bags` module.

use std::sync::Arc;

use tlx_sema::hir::{Expr, Span};
use tlx_value::Value;

use crate::env::Env;
use crate::error::{EResult, EvalError};
use crate::eval::Ctx;
use crate::interp::Interp;

impl Interp {
    /// `BagOfAll(F, B)` (Bags.java): the bag of the images of B's elements.
    pub(super) fn bag_of_all(self: &Arc<Self>, f: &Expr, b: &Expr, env: &Env, cx: &Ctx, span: Span) -> EResult<Value> {
        let f = self.native_op_val(f, env, "first", "BagOfAll", span)?;
        let b = self.eval(b, env, cx)?;
        let mut images = Vec::new();
        for (d, n) in crate::bags::bag_pairs(&b, "BagOfAll")? {
            images.push((self.apply_opval_values(&f, vec![d], cx, span)?, n));
        }
        crate::bags::bag_of_images(images, &b).map_err(|e| e.with_span(span))
    }

    /// `FoldBag(op, base, B)` (BagsExt.java): `op(acc, e)` applied once per
    /// copy of each element, elements in domain order.
    pub(super) fn fold_bag(self: &Arc<Self>, args: &[Expr], env: &Env, cx: &Ctx, span: Span) -> EResult<Value> {
        let op = self.native_op_val(&args[0], env, "first", "FoldBag", span)?;
        let mut acc = self.eval(&args[1], env, cx)?;
        let b = self.eval(&args[2], env, cx)?;
        let Some(pairs) = crate::bags::bag_pairs(&b, "FoldBag").ok() else {
            let msg = format!("The third argument of FoldBag should be a bag, but instead it is:\n{b}");
            return Err(EvalError::of_kind(tlx_runtime::ErrorKind::ArgumentError, msg).with_span(span));
        };
        for (d, n) in pairs {
            let n = match n {
                Value::Int(n) if n > 0 => n,
                n => {
                    let msg = format!("Applying FoldBag to the following value,\nwhich is not an element of Nat:\n{n} (in: {d}:>{n})");
                    return Err(EvalError::of_kind(tlx_runtime::ErrorKind::ArgumentError, msg).with_span(span));
                }
            };
            for _ in 0..n {
                acc = self.apply_opval_values(&op, vec![acc, d.clone()], cx, span)?;
            }
        }
        Ok(acc)
    }
}
