//! Action composition `A \cdot B` (moved out of `enumerate.rs`).

use super::*;

impl<'a> Enumerator<'a> {
    /// `A \cdot B` (`\E t : A(s0, t) /\ B(t, s1)`): the successors of B
    /// from each successor of A; each continues with `k` (it must agree with
    /// the variables already assigned). A variable A leaves undetermined
    /// keeps its value in the intermediate state; one B leaves undetermined
    /// stays unassigned, for the conjuncts after the composition. (TLC
    /// instead fixes the variables assigned before the composition at their
    /// current values in the intermediate state and checks the conjuncts
    /// after it on both steps: compat.md D-19.)
    pub(super) fn compose(&mut self, a: &Expr, b: &Expr, env: &Env, k: &Cont) -> EResult<bool> {
        let s0 = self.s0.expect("composition in Next");
        let mids = composition_mids(self.ip, s0, self.level, a, env)?;
        // TLC counts the intermediate states as generated.
        if self.count_composed {
            self.ip.count_composed_states(mids.len() as u64);
        }
        for mid in mids {
            for fin in partial_successors_at_most(self.ip, &mid, self.level, b, env, usize::MAX)?.unwrap_or_default() {
                if self.s1.iter().zip(&fin).any(|(x, y)| matches!((x, y), (Some(x), Some(y)) if x != y)) {
                    continue;
                }
                let merged: Vec<Option<Value>> = self.s1.iter().zip(fin).map(|(x, y)| y.or_else(|| x.clone())).collect();
                let unassigned = merged.iter().filter(|v| v.is_none()).count();
                let saved = std::mem::replace(&mut self.s1, merged);
                let saved_unassigned = std::mem::replace(&mut self.unassigned, unassigned);
                let go_on = self.cont(k);
                self.s1 = saved;
                self.unassigned = saved_unassigned;
                if !go_on? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
