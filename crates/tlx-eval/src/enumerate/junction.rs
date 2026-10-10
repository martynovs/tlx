//! The logical connectives of the next-state relation: `\land`, `\lor`
//! and `=>` (moved out of `enumerate.rs`). A conjunction extends the
//! pending conjuncts by its tail and enumerates its head; a disjunction
//! enumerates every disjunct with the same continuation (in generation
//! mode in a random order and stride, until the action has a successor);
//! an implication tests its antecedent before enumerating its consequent.

use super::*;

impl<'a> Enumerator<'a> {
    /// `A /\ B /\ ...`: enumerate the head with the rest pending.
    pub(super) fn conjunction(&mut self, es: &[Expr], env: &Env, k: &Cont) -> EResult<bool> {
        let k2 = Cont::push(Seg::Conj(&es[1..], env), k);
        self.go(&es[0], env, &k2)
    }

    /// `A \/ B \/ ...` in generation mode (TLC's `-generate`).
    pub(super) fn disjunction_random(&mut self, es: &[Expr], env: &Env, k: &Cont) -> EResult<bool> {
        // TLC: from a random disjunct, with a random stride.
        let n = es.len();
        let mut i = tlx_runtime::live::gen_below(n);
        let stride = tlx_runtime::live::gen_stride();
        for _ in 0..n {
            if !self.go(&es[i], env, k)? {
                return Ok(false);
            }
            if tlx_runtime::live::has_states() {
                break;
            }
            i = (i + stride) % n;
        }
        Ok(true)
    }

    /// `A \/ B \/ ...`: enumerate every disjunct.
    pub(super) fn disjunction(&mut self, es: &[Expr], env: &Env, k: &Cont) -> EResult<bool> {
        for c in es {
            if !self.go(c, env, k)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `A => B`: test the antecedent, then enumerate the consequent or
    /// drop the conjunct.
    pub(super) fn implies(&mut self, args: &[Expr], env: &Env, k: &Cont) -> EResult<bool> {
        if self.eval_bool(&args[0], env)? {
            self.go(&args[1], env, k)
        } else {
            self.cont(k)
        }
    }
}
