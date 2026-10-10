//! The end of a run: liveness and POSTCONDITION after the search, then
//! the TLC-style report of the outcome, its message codes and TLC's exit
//! status.

use super::*;

mod diagnostic;
mod presentation;

use presentation::report;

/// Exit codes (TLC's `EC.ExitStatus`).
/// VIOLATION_ASSUMPTION: an assumption or the POSTCONDITION is violated.
pub(crate) const EXIT_ASSUMPTION: u8 = 10;
pub(crate) const EXIT_DEADLOCK: u8 = 11;
pub(crate) const EXIT_INVARIANT: u8 = 12;
/// VIOLATION_LIVENESS (also an action property violated by a simulation).
pub(crate) const EXIT_LIVENESS: u8 = 13;
/// VIOLATION_ASSERT.
pub(crate) const EXIT_ASSERT: u8 = 14;
/// FAILURE_SPEC_EVAL.
pub(crate) const EXIT_EVAL_ERROR: u8 = 75;
/// FAILURE_LIVENESS_EVAL (e.g. a temporal property that is a tautology).
pub(crate) const EXIT_LIVENESS_EVAL: u8 = 77;
/// FAILURE_SAFETY_EVAL (an invariant could not be evaluated, or is not a
/// state predicate).
pub(crate) const EXIT_SAFETY_EVAL: u8 = 76;
pub(crate) const EXIT_SETUP_ERROR: u8 = 150;
/// SYSTEM_ERROR_CONFIG (e.g. an action-level PROPERTY).
pub(crate) const EXIT_CONFIG_ERROR: u8 = 151;
/// TLC's exit status for other errors (`EC.ExitStatus.ERROR`).
pub(crate) const EXIT_GENERAL_ERROR: u8 = 255;

/// A run being reported.
pub(crate) struct Run {
    pub(crate) workers: usize,
    /// The names of the actions (TLC's), as in errors.
    pub(crate) actions: Vec<String>,
    pub(crate) started: Instant,
    pub(crate) sim: Option<Sim>,
    /// What limits the fingerprint set (a full one reports it).
    pub(crate) fp_mem: FpMem,
    /// The options of a model-checking search.
    pub(crate) opts: Option<Options>,
    /// `-coverage`: the coverage entries of the model.
    pub(crate) coverage: Option<Arc<coverage::Table>>,
}

/// A simulation being reported.
pub(crate) struct Sim {
    pub(crate) seed: u64,
    /// The label of initial states in traces.
    pub(crate) init_label: String,
    /// The last progress of the simulation.
    pub(crate) progress: SimProgress,
}

/// After the search: liveness, POSTCONDITION, then the TLC-style report.
pub(crate) fn finish<T: Tier>(m: &T, mut r: Report<T::State>, run: &Run) -> ExitCode {
    let base = m.base();
    let init = run.sim.as_ref().map_or_else(|| "Initial predicate".to_string(), |sim| sim.init_label.clone());
    let view = live::TraceView::new(m, init);
    explain_incomplete(m, &mut r);
    let recorded = match &r.live_graph {
        Some(Ok(g)) => g.bytes(),
        _ => 0,
    };
    // (Liveness, if any, starts here.)
    let checks = run.sim.is_none() && matches!(r.outcome, Outcome::Ok) && !base.live.is_empty();
    let guard =
        tlx_runtime::mem::RssGuard::new(if checks { run.opts.as_ref().map_or(usize::MAX, |o| o.maxmem) } else { usize::MAX }, recorded);
    let mut lv = if run.sim.is_none() { liveness(m, &mut r, run, &view, guard) } else { None };
    // TLC evaluates the liveness predicates during its search: an error in
    // one of them stops it where it occurs, maybe before the end of the
    // search or the failure tlx found. So does a violation of some
    // properties that TLC detects during its search. Search again
    // to find them then.
    if let (Some(opts), false) = (&run.opts, base.live.is_empty()) {
        let early = live::reports_early(base);
        let again = match (&r.outcome, &lv) {
            (Outcome::Ok, Some((live::Liveness::Error(_), _))) => true,
            (Outcome::Ok, Some((live::Liveness::Violated(..), _))) => early,
            (Outcome::Ok | Outcome::FpSetFull { .. } | Outcome::Io(_), _) => false,
            // A violation: the search found none of these errors where TLC
            // evaluates the predicates before it, if the initial
            // states were expanded (a state at depth 3 was found). (After an
            // evaluation error in the next-state relation TLC still checks
            // the state, and an error there decides its exit status.)
            (Outcome::InvariantViolated(_) | Outcome::ActionPropertyViolated(_) | Outcome::Deadlock, _)
                if !early && r.live_atoms_ok && r.stats.depth >= 3 =>
            {
                false
            }
            _ => true,
        };
        let found = again.then(|| {
            let t0 = Instant::now();
            let r2 = live::find_error(m, opts, early, &view);
            stats::phase("liveness: search again (TLC's errors during its search)", t0.elapsed());
            r2
        });
        if let Some(r2) = found.flatten() {
            r = Report { elapsed: r.elapsed, ..r2 };
            lv = None;
        }
    }
    let post = postcondition(base, &r, run.sim.is_some());
    // -dumpTrace: TLC writes the counterexample of a violation (not after
    // an evaluation error).
    let steps: Option<Vec<_>> = (dump_trace::requested()
        && matches!(r.outcome, Outcome::InvariantViolated(_) | Outcome::ActionPropertyViolated(_) | Outcome::Deadlock))
    .then(|| r.trace.iter().map(|t| (m.values(&t.state), t.action)).collect());
    // (And of a liveness violation, the lasso shown.)
    let lasso = match (&r.outcome, &lv) {
        (Outcome::TemporalViolated { counterexample, .. }, _) | (_, Some((live::Liveness::Violated(_, counterexample), _))) => {
            dump_trace::kept_lasso(counterexample)
        }
        _ => None,
    };
    let dump = || {
        if let Some(steps) = &steps {
            dump_trace::write(base, steps);
        } else if let Some(l) = &lasso {
            dump_trace::write_lasso(base, l);
        }
    };
    report::<T>(r, lv, post, &view, run, &dump)
}

/// An incomplete state reported by a compiled tier (by its message only):
/// found again with the interpreter, which shows the state.
pub(crate) fn explain_incomplete<T: Tier>(m: &T, r: &mut Report<T::State>) {
    let base = m.base();
    let Outcome::Error(e) = &r.outcome else { return };
    if matches!(e.context, ErrorContext::IncompleteState { .. })
        || !e.message.contains(" is not completely specified. The following variable(s) are not defined: ")
    {
        return;
    }
    let found = if e.message.starts_with("The initial state ") {
        base.init_states(&mut |_| {}).err()
    } else if let Some(last) = r.trace.last() {
        let vals: Vec<Option<tlx_value::Value>> = m.values(&last.state).iter().cloned().map(Some).collect();
        let Ok(s) = base.make_state(&vals, r.trace.len() as u32, "the last state") else { return };
        (0..base.action_names().len()).find_map(|i| base.successors_of(&s, i, &mut |_, _| true).err())
    } else {
        None
    };
    if let Some(found) = found.filter(|f| matches!(f.context, ErrorContext::IncompleteState { .. })) {
        r.outcome = Outcome::Error(found);
    }
}

/// Record the statistics of the search of `r` for `TLCGet("stats")` (and
/// `"queue"`, ...) evaluated after it: in the POSTCONDITION, and in the
/// properties a liveness counterexample is found to violate (TLC evaluates
/// them when it reports the violation, with the statistics of its search
/// then).
pub(crate) fn record_search_stats<S>(base: &InterpModel, r: &Report<S>) {
    base.ip.set_final_stats(tlx_eval::RunStats {
        distinct: r.stats.distinct,
        generated: r.stats.generated,
        queue: r.stats.left,
        diameter: r.stats.depth as u64,
        duration_secs: r.elapsed.as_secs(),
        initial: r.stats.init_distinct,
        ..Default::default()
    });
}

/// The POSTCONDITION after a search that ran to completion (TLC also
/// checks it after a deadlock, and its simulator after any behavior but
/// a failure of the initial states): the message code and text if it is
/// violated or cannot be evaluated.
pub(crate) fn postcondition<S>(base: &InterpModel, r: &Report<S>, simulation: bool) -> Option<(u32, String)> {
    if !matches!(r.outcome, Outcome::Ok | Outcome::Deadlock) && (!simulation || r.in_init) {
        return None;
    }
    if !simulation {
        // (The statistics of a simulation are already recorded.)
        record_search_stats(base, r);
    }
    match base.check_postcondition() {
        Ok(msg) => msg.map(|m| (code::TLC_POSTCONDITION_FALSE, m)),
        Err(e) => {
            let label = base.postcondition_label().unwrap_or_default();
            let text = format!("Evaluating postcondition {label} failed.\n{}", out::nested(out::error_kind(&e), &e.message));
            Some((code::TLC_POSTCONDITION_EVALUATION_ERROR, text))
        }
    }
}

/// Liveness checking after a search without errors, if the model has
/// temporal properties: the outcome and the time it took.
/// Its time counts in the run's elapsed time (as with TLC).
pub(crate) fn liveness<T: Tier>(
    m: &T,
    r: &mut Report<T::State>,
    run: &Run,
    view: &live::TraceView<T::State>,
    guard: tlx_runtime::mem::RssGuard,
) -> Option<(live::Liveness, Duration)> {
    let base = m.base();
    let (workers, opts) = (run.workers, run.opts.as_ref());
    if !matches!(r.outcome, Outcome::Ok) || base.live.is_empty() {
        return None;
    }
    let st = &r.stats;
    print_progress(st.depth as u64, st.generated, st.distinct, st.left, None);
    // TLC checks each disjunct of the negated properties (its "branches")
    // on its own copy of the state graph.
    let branches = live_branches(base);
    let text = if branches > 1 {
        format!(
            "Checking {branches} branches of temporal properties for the complete state space with {} total distinct states",
            st.distinct * branches as u64
        )
    } else {
        format!("Checking temporal properties for the complete state space with {} total distinct states", st.distinct)
    };
    out::info(code::TLC_CHECKING_TEMPORAL_PROPS, &format!("{text} at ({})", out::now()));
    let start = Instant::now();
    record_search_stats(base, r);
    // A search stopped by TLCSet("exit", TRUE): TLC checks the graph of the
    // states it expanded (those left on the queue are not).
    let expanded = if m.stop_requested() { (st.distinct - st.left) as usize } else { usize::MAX };
    // (An atom failed during the search: TLC reports it as it evaluates
    // them there, `live_error`.)
    let mut lv = match r.live_error.take() {
        Some(e) => live::Liveness::Error(e),
        None => live::check(m, workers, expanded, view, r.live_graph.take(), guard),
    };
    if matches!(&lv, live::Liveness::Error(e) if e.is_layout_miss()) {
        // A compiled model cannot widen its layout after the search: the
        // interpreter checks the properties instead (on the graph its own
        // search records; with VIEW or SYMMETRY the check explores).
        tlx_base::diag::emit(
            tlx_base::diag::Topic::Compile,
            "layout miss checking temporal properties; checking them with the interpreter",
        );
        let view = live::TraceView::new(base, "Initial predicate".into());
        let graph = match opts {
            Some(opts) if !base.has_view_or_symmetry() => Some(interpreted_search(base, opts, workers)),
            _ => None,
        };
        lv = match graph {
            Some(Err(e)) => live::Liveness::Error(e),
            graph => live::check(base, workers, expanded, &view, graph.map(|g| g.expect("checked above")), guard),
        };
    }
    r.elapsed += start.elapsed();
    Some((lv, start.elapsed()))
}

/// The interpreter's own search of the model, for the behavior graph it
/// records (liveness after a compiled model's layout miss): the graph (or
/// the memory error), or the error of an atom or of the search. The output
/// of the spec stays suppressed (the first search printed it).
fn interpreted_search(
    base: &InterpModel,
    opts: &Options,
    workers: usize,
) -> Result<Result<tlx_runtime::LiveGraph, String>, tlx_runtime::ModelError> {
    let printing = !base.ip.print_suppressed();
    base.ip.set_suppress_print(true);
    // (The first search's request to stop would end this one at once.)
    base.ip.undo_exit();
    let r = tlx_runtime::explore(
        base,
        &Options { progress: None, coverage: false, coverage_report: None, checkpoint: None, workers, ..opts.clone() },
    );
    base.ip.set_suppress_print(!printing);
    match (r.live_error, r.live_graph, r.outcome) {
        (Some(e), _, _) => Err(e),
        (None, Some(g), Outcome::Ok) => Ok(g),
        (None, _, Outcome::Error(e)) => Err(e),
        _ => Err(tlx_runtime::ModelError::new("internal error: the interpreter's search recorded no liveness graph")),
    }
}

/// The number of disjuncts of the negated temporal properties (TLC's
/// "branches"), each checked on its own.
pub(crate) fn live_branches(base: &InterpModel) -> usize {
    tlx_live::branches(base.live.props.iter().map(|(_, f)| f))
}
