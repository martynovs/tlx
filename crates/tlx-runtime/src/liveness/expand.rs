//! The expansion of a state for checking liveness TLC's way, model-generic
//! (moved from tlx-eval's `model/liveness.rs`).

use crate::AtomBits;

/// The action index of stuttering steps (`ConcreteStep::action`, counterexample
/// steps).
pub const STUTTER: u32 = u32::MAX;

/// `f` applied to every state, by `workers` threads (results in order).
pub fn par_map<S: Sync, T: Send, E: Send>(states: &[S], workers: usize, f: &(dyn Fn(&S) -> Result<T, E> + Sync)) -> Result<Vec<T>, E> {
    const CHUNK: usize = 64;
    let workers = workers.max(1).min(states.len().div_ceil(CHUNK).max(1));
    if workers == 1 {
        return states.iter().map(f).collect();
    }
    // (The evaluation stack: `f` evaluates TLA+.)
    let pool = tlx_base::par::Pool { threads: workers, stack: tlx_base::par::Stack::Eval };
    pool.map_chunks(states, CHUNK, |_, part| part.iter().map(f).collect())
}

/// A step from a state, for checking liveness TLC's way when several
/// states share a fingerprint (VIEW, SYMMETRY; see `tlx_live::check_concrete`).
pub struct ConcreteStep<S> {
    /// The successor's fingerprint.
    pub fp: u64,
    pub state: S,
    /// The bits of the tableau state atoms on the successor (the others are
    /// 0: they only occur in conditions evaluated on expanded states; see
    /// `AtomBits::tableau_bits`).
    pub bits: Vec<u64>,
    /// Action-atom bits of the step.
    pub step: Vec<u64>,
    /// The action (`STUTTER` for the stuttering step).
    pub action: u32,
}

/// A state's state-atom bits and steps, the stuttering step first (as TLC).
pub struct ConcreteExpansion<S> {
    pub bits: Vec<u64>,
    pub succ: Vec<ConcreteStep<S>>,
}

/// The successors of a state in the model (`concrete_successors`): each
/// with its action, its fingerprint and whether the step satisfies the
/// constraints (a step that does not is left out of the graph but counts for
/// ENABLED).
pub struct ConcreteSuccessors<S> {
    pub all: Vec<(S, u32, u64, bool)>,
}

/// Expand state `s` of `m`. As in the search, steps to states violating a
/// CONSTRAINT or violating an ACTION_CONSTRAINT are left out (but count for
/// ENABLED).
pub fn expand_concrete<M: crate::Model>(
    m: &M,
    bits: &(dyn AtomBits<M::State> + Sync),
    s: &M::State,
) -> Result<ConcreteExpansion<M::State>, crate::ModelError> {
    concrete_bits(m, bits, s, concrete_successors(m, s)?)
}

/// The successors of `s` in `m`, in generation order (the first half of
/// `expand_concrete`).
pub fn concrete_successors<M: crate::Model>(m: &M, s: &M::State) -> Result<ConcreteSuccessors<M::State>, crate::ModelError> {
    let mut all = Vec::new();
    m.successors(s, &mut |t, a| {
        all.push((t, a));
        true
    })?;
    let mut out = Vec::with_capacity(all.len());
    for (t, a) in all {
        let ok = crate::admits(m, s, &t)?;
        let fp = m.fingerprint(&t);
        out.push((t, a, fp, ok));
    }
    Ok(ConcreteSuccessors { all: out })
}

/// The atom bits of `s` and its successors `succ` (the second half of
/// `expand_concrete`).
pub fn concrete_bits<M: crate::Model>(
    m: &M,
    bits: &(dyn AtomBits<M::State> + Sync),
    s: &M::State,
    succ: ConcreteSuccessors<M::State>,
) -> Result<ConcreteExpansion<M::State>, crate::ModelError> {
    let stutter = bits.action(s, s)?;
    let mut cut = Vec::new();
    let mut steps = Vec::with_capacity(succ.all.len() + 1);
    steps.push(ConcreteStep { fp: m.fingerprint(s), state: s.clone(), bits: bits.tableau_bits(s)?, step: stutter, action: STUTTER });
    for (t, a, fp, ok) in succ.all {
        let step = bits.action(s, &t)?;
        if !ok {
            cut.push(step);
            continue;
        }
        steps.push(ConcreteStep { fp, bits: bits.tableau_bits(&t)?, state: t, step, action: a });
    }
    let outs: Vec<&[u64]> = steps.iter().map(|c| c.step.as_slice()).chain(cut.iter().map(|b| b.as_slice())).collect();
    let sbits = bits.state(s, &outs)?;
    Ok(ConcreteExpansion { bits: sbits, succ: steps })
}

#[cfg(test)]
mod tests {
    use super::par_map;

    /// A panicking worker is re-raised by `par_map` (the other workers stop
    /// early; the call never hangs or swallows the panic).
    #[test]
    fn par_map_re_raises_a_worker_panic() {
        let states: Vec<usize> = (0..10_000).collect();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            par_map::<usize, usize, ()>(&states, 4, &|s| {
                if *s == 100 {
                    panic!("injected par_map panic");
                }
                Ok(*s)
            })
        }));
        assert_eq!(r.unwrap_err().downcast_ref::<&str>().copied(), Some("injected par_map panic"));
    }
}
