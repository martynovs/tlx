//! The trace log (docs/scale.md §7): for every distinct state, the entry of
//! its parent and its fingerprint, in the history shard that recorded it.
//! States are not kept; a trace is rebuilt by re-executing the model along
//! the fingerprint chain (as TLC does).

use super::{Outcome, TraceStep};
use crate::io::{IoError, Spill, SpillVec};
use crate::model::Model;

/// Reference to a trace-log entry: stable shard ID in the high 16 bits, index
/// into that shard's log in the low 48 bits.
pub(super) type LogRef = u64;

/// The parent of an initial state.
pub(super) const NO_PARENT: LogRef = u64::MAX;

pub(super) fn log_ref(shard: usize, idx: usize) -> LogRef {
    ((shard as u64) << 48) | idx as u64
}

/// The (shard, index) a `LogRef` designates.
pub(super) fn split_log_ref(r: LogRef) -> (usize, usize) {
    ((r >> 48) as usize, (r & ((1 << 48) - 1)) as usize)
}

/// One history shard's trace log: `(parent, fingerprint)` for every distinct state
/// it found, in order. Stored in fixed-size blocks (docs/scale.md §7, 1 MiB
/// by default: the queue's segment size): appending never moves entries,
/// and under memory pressure the full blocks are written to disk
/// (`spill`); entries are read back from there when a trace is rebuilt.
pub(super) struct TraceLog {
    entries: SpillVec<[u64; 2]>,
}

impl TraceLog {
    /// A log in blocks of `block_bytes`.
    pub(super) fn new(block_bytes: usize) -> TraceLog {
        TraceLog { entries: SpillVec::new(block_bytes, "trace") }
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Append an entry; returns its index.
    #[inline]
    pub(super) fn push(&mut self, parent: LogRef, fp: u64) -> usize {
        self.entries.push([parent, fp])
    }

    /// Entry `i`: `(parent, fingerprint)`.
    pub(super) fn get(&self, i: usize) -> Result<(LogRef, u64), IoError> {
        self.entries.get(i).map(|[p, f]| (p, f))
    }

    /// Set the fingerprint of entry `i` (a resumed search: the state
    /// re-encoded, see `ResumePoint`).
    pub(super) fn set_fp(&mut self, i: usize, fp: u64) -> Result<(), IoError> {
        let [p, _] = self.entries.get(i)?;
        self.entries.set(i, [p, fp])
    }

    /// The memory held (bytes).
    pub(super) fn bytes(&self) -> usize {
        self.entries.bytes()
    }

    /// Write the full blocks (with `all`, every block) to disk; see
    /// `SpillVec::spill`.
    pub(super) fn spill(&mut self, spill: &Spill, all: bool) -> Result<(), IoError> {
        self.entries.spill(spill, all)
    }

    /// Wait for the blocks being written.
    pub(super) fn settle(&mut self) -> Result<(), IoError> {
        self.entries.settle()
    }

    /// Before appending to a log spilled whole (`spill` with `all`).
    pub(super) fn unseal(&mut self) -> Result<(), IoError> {
        self.entries.unseal()
    }

    /// The blocks on disk.
    pub(super) fn spilled_blocks(&self) -> usize {
        self.entries.spilled_blocks()
    }

    /// Call `f` with the entries in order, in slices of at most `n`
    /// (spilled blocks are read back); stops at the first error.
    pub(super) fn for_each_chunk(
        &self,
        spill: &Spill,
        start: usize,
        n: usize,
        f: impl FnMut(&[[u64; 2]]) -> Result<(), String>,
    ) -> Result<(), String> {
        super::checkpoint::chunked(&self.entries, spill, start, n, f)
    }
}

impl Default for TraceLog {
    /// (A placeholder, e.g. for `std::mem::take`.)
    fn default() -> TraceLog {
        TraceLog::new(1 << 20)
    }
}

/// Re-execute the model along the fingerprint chain ending at `at` (None
/// if it fails); an error reading the logs.
pub(super) struct Rebuilt<S> {
    pub trace: Vec<TraceStep<S>>,
    pub complete: bool,
}

pub(super) fn rebuild_trace<M: Model>(
    model: &M,
    logs: &[TraceLog],
    at: LogRef,
    outcome: &Outcome,
) -> Result<Option<Rebuilt<M::State>>, IoError> {
    let mut fps = Vec::new();
    let mut cur = at;
    while cur != NO_PARENT {
        let (w, i) = split_log_ref(cur);
        let (parent, fp) = logs[w].get(i)?;
        fps.push(fp);
        cur = parent;
    }
    fps.reverse();
    let started = std::time::Instant::now();
    let rebuilt = if model.coarse_fingerprint() {
        replay_classes(model, &fps, outcome)
    } else {
        replay(model, &fps).map(|trace| Rebuilt { trace, complete: true })
    };
    crate::stats::set("trace reconstruction", crate::stats::Metric::Secs(started.elapsed().as_secs_f64()));
    Ok(rebuilt)
}

/// Re-execute a chain whose fingerprints identify concrete states.
fn replay<M: Model>(model: &M, fps: &[u64]) -> Option<Vec<TraceStep<M::State>>> {
    let (&first, rest) = fps.split_first()?;
    let mut found = None;
    // (An error in a later initial state does not matter: the search
    // stopped at the first failure, see `init_phase`.)
    let _ = model.init_states(&mut |s| {
        if found.is_none() && model.fingerprint(&s) == first {
            found = Some(s);
        }
    });
    let mut trace = vec![TraceStep { state: found?, action: None }];
    for &fp in rest {
        let prev = &trace.last().unwrap().state;
        let mut next = None;
        model
            .successors(prev, &mut |t, a| {
                if model.fingerprint(&t) == fp {
                    next = Some((t, a));
                    return false;
                }
                true
            })
            .ok()?;
        let (t, a) = next?;
        trace.push(TraceStep { state: t, action: Some(a) });
    }
    Some(trace)
}

/// VIEW/SYMMETRY logs identify classes. Follow actual successors from an
/// actual initial state, backtracking within each logged class until the
/// reported failure is reproduced. Iterative DFS bounds the depth by the
/// log. Alternatives are re-enumerated, retaining only hashes of previously
/// tried concrete states rather than buffering an entire class of states.
/// All extra work happens after failure; search-time logging is unchanged.
fn replay_classes<M: Model>(model: &M, fps: &[u64], outcome: &Outcome) -> Option<Rebuilt<M::State>> {
    use crate::live::{self, CurrentPath, PathNode};
    use std::hash::{Hash, Hasher};
    use std::sync::Arc;

    if fps.is_empty() {
        return None;
    }
    let mut tried: Vec<tlx_base::hash::FxSet<u64>> = vec![Default::default()];
    let mut trace: Vec<TraceStep<M::State>> = Vec::with_capacity(fps.len());
    let mut paths: Vec<Arc<PathNode>> = Vec::new();
    let limit = model.trace_replay_limit();
    let mut prefix = Vec::new();
    loop {
        let depth = trace.len();
        let terminal = depth + 1 == fps.len();
        let prev = trace.last().map(|s| &s.state);
        let mut next = None;
        let mut consider = |s: M::State, action| {
            if model.fingerprint(&s) != fps[depth] {
                return true;
            }
            let step = TraceStep { state: s, action };
            if terminal {
                if !matches_outcome(model, prev, &step, outcome) {
                    return true;
                }
            } else {
                let admitted = prev.map_or_else(|| model.constraint(&step.state), |p| crate::model::admits(model, p, &step.state));
                if admitted != Ok(true) {
                    return true;
                }
                // Print-time identity only: enumeration may change order on
                // every replay. The model's full rendering distinguishes
                // members of a class without changing search fingerprints.
                let mut hash = tlx_base::hash::StableHasher::new();
                model.render(&step.state).hash(&mut hash);
                if !tried[depth].insert(hash.finish()) {
                    return true;
                }
            }
            next = Some(step);
            false
        };
        live::with_path(paths.last().cloned().map(CurrentPath::Known), || {
            // A later generation error does not invalidate an earlier
            // successor: the original search may have stopped before it.
            if let Some(prev) = prev {
                let _ = model.successors(prev, &mut |s, a| consider(s, Some(a)));
            } else {
                let _ = model.init_states_while(&mut |s| consider(s, None));
            }
        });
        if let Some(step) = next {
            if model.uses_trace() && !terminal {
                paths.push(Arc::new(PathNode {
                    values: model.state_values(&step.state).expect("a model that uses TLCExt!Trace has state values"),
                    action: step.action,
                    parent: paths.last().cloned(),
                }));
            }
            trace.push(step);
            if terminal {
                return Some(Rebuilt { trace, complete: true });
            }
            tried.push(Default::default());
        } else {
            tried.pop();
            if limit.is_some() && trace.len() > prefix.len() {
                prefix = trace.clone();
            }
            if trace.pop().is_none() {
                return limit.map(|_| Rebuilt { trace: prefix, complete: false });
            }
            paths.pop();
        }
    }
}

/// Check the reported failure under the path to the predecessor, as in the
/// search. A class can contain both a bad action and stuttering, or both
/// failing and non-failing states. Constraints may reject the last state
/// (TLC still checks it), but an evaluation error precedes property checks.
fn matches_outcome<M: Model>(model: &M, prev: Option<&M::State>, last: &TraceStep<M::State>, outcome: &Outcome) -> bool {
    use crate::model::ErrorContext;
    let s = &last.state;
    let admitted = prev.map_or_else(|| model.constraint(s), |p| crate::model::admits(model, p, s));
    if let Outcome::Error(expected) = outcome {
        if let ErrorContext::InitialState(rendered) = &expected.context {
            return model.render(s) == *rendered;
        }
        let same = |r: Result<(), crate::ModelError>| r.err().as_ref() == Some(expected);
        if let Err(e) = admitted {
            return e == *expected;
        }
        if same(model.check_invariants(s).map(|_| ())) {
            return true;
        }
        if let Some(p) = prev {
            if same(model.check_transition(p, s).map(|_| ())) {
                return true;
            }
        } else if same(model.check_init_property(s).map(|_| ())) {
            return true;
        }
        return admitted == Ok(true)
            && with_last_path(model, last, || same(model.successors(s, &mut |_, _| true).map_err(|e| e.in_next_state())));
    }
    if admitted.is_err() {
        return false;
    }
    match outcome {
        Outcome::InvariantViolated(name) => {
            model.check_invariants(s).ok().flatten().is_some_and(|i| model.invariant_names().get(i) == Some(name))
        }
        Outcome::InitPropertyViolated(name) => {
            model.check_invariants(s) == Ok(None) && model.check_init_property(s).ok().flatten().as_ref() == Some(name)
        }
        Outcome::ActionPropertyViolated(name) => prev.is_some_and(|p| {
            model.check_transition(p, s).ok().flatten().is_some_and(|i| model.action_property_names().get(i) == Some(name))
        }),
        Outcome::Deadlock => {
            admitted == Ok(true)
                && with_last_path(model, last, || {
                    let mut enabled = false;
                    let result = model.successors(s, &mut |_, _| {
                        enabled = true;
                        false
                    });
                    result.is_ok() && !enabled
                })
        }
        _ => true,
    }
}

/// Expanding the terminal state (deadlock or Next error) needs its whole
/// path, while state/action checks above run under its predecessor's path.
fn with_last_path<M: Model, R>(model: &M, last: &TraceStep<M::State>, f: impl FnOnce() -> R) -> R {
    use crate::live::{self, CurrentPath, PathNode};
    if !model.uses_trace() {
        return f();
    }
    let parent = match live::current_path() {
        Some(CurrentPath::Known(p)) => Some(p),
        _ => None,
    };
    let path = std::sync::Arc::new(PathNode {
        values: model.state_values(&last.state).expect("a model that uses TLCExt!Trace has state values"),
        action: last.action,
        parent,
    });
    live::with_path(Some(CurrentPath::Known(path)), f)
}
