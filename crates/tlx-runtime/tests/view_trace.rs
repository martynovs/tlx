//! Safety traces retain concrete states under VIEW, even when replay
//! enumerates the same states in a different order (the Model contract).
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tlx_runtime::{explore, Model, ModelError, Options, Outcome};

#[derive(Default)]
struct View {
    initial_violation: bool,
    initial_error: bool,
    constraint_error: bool,
    names: Vec<String>,
    reversed: AtomicBool,
}

impl Model for View {
    type State = (u32, u32);
    fn state_values(&self, _: &Self::State) -> Option<std::sync::Arc<[tlx_value::Value]>> {
        panic!("coarse trace logging must not decode or hash complete states")
    }
    fn check_init_property(&self, _: &Self::State) -> Result<Option<String>, ModelError> {
        Ok(None)
    }
    fn action_constraint(&self, _: &Self::State, t: &Self::State) -> Result<bool, ModelError> {
        if self.constraint_error && t.1 == 2 {
            Err(ModelError::new("decoy constraint"))
        } else {
            Ok(true)
        }
    }

    fn extra_generated(&self) -> u64 {
        0
    }
    fn stop_requested(&self) -> bool {
        false
    }

    fn init_states(&self, out: &mut dyn FnMut(Self::State)) -> Result<(), ModelError> {
        if self.initial_violation {
            let states =
                if !self.initial_error && self.reversed.fetch_xor(true, Ordering::Relaxed) { [(0, 1), (0, 0)] } else { [(0, 0), (0, 1)] };
            for state in states {
                out(state);
            }
        } else {
            out((0, 0));
        }
        Ok(())
    }
    fn successors(&self, s: &Self::State, out: &mut dyn FnMut(Self::State, u32) -> bool) -> Result<(), ModelError> {
        let mut states = vec![(*s, 0), ((s.0, 1), 1)];
        if self.constraint_error {
            states.push(((s.0, 2), 2));
        }
        if self.reversed.fetch_xor(true, Ordering::Relaxed) {
            states.reverse();
        }
        for (state, action) in states {
            if !out(state, action) {
                break;
            }
        }
        Ok(())
    }
    fn fingerprint(&self, s: &Self::State) -> u64 {
        s.0 as u64 + 1
    }
    fn coarse_fingerprint(&self) -> bool {
        true
    }
    fn constraint(&self, _: &Self::State) -> Result<bool, ModelError> {
        Ok(!self.initial_violation)
    }
    fn check_invariants(&self, s: &Self::State) -> Result<Option<usize>, ModelError> {
        if self.initial_error && s.1 == 1 {
            return Err(ModelError::new("hidden error"));
        }
        Ok((self.initial_violation && s.1 == 1).then_some(0))
    }
    fn check_transition(&self, s: &Self::State, t: &Self::State) -> Result<Option<usize>, ModelError> {
        Ok((!self.initial_violation && s != t).then_some(0))
    }
    fn invariant_names(&self) -> &[String] {
        &self.names
    }
    fn action_property_names(&self) -> &[String] {
        &self.names
    }
    fn action_names(&self) -> &[String] {
        &self.names
    }
    fn render(&self, s: &Self::State) -> String {
        format!("{s:?}")
    }
}

#[test]
fn action_property_trace_ends_in_the_actual_successor() {
    let model = View { names: vec!["Property".into()], ..Default::default() };
    for workers in [1, 4] {
        let r = explore(&model, &Options { workers, ..Default::default() });
        assert_eq!(r.outcome, Outcome::ActionPropertyViolated("Property".into()));
        assert_eq!(r.trace.iter().map(|t| t.state).collect::<Vec<_>>(), [(0, 0), (0, 1)]);
        assert_eq!(r.trace[1].action, Some(1));
    }
}

#[test]
fn initial_invariant_trace_shows_the_violating_state() {
    let model = View { initial_violation: true, names: vec!["Property".into()], ..Default::default() };
    for workers in [1, 4] {
        let r = explore(&model, &Options { workers, ..Default::default() });
        assert_eq!(r.outcome, Outcome::InvariantViolated("Property".into()));
        assert_eq!(r.trace.iter().map(|t| t.state).collect::<Vec<_>>(), [(0, 1)]);
    }
}

#[test]
fn initial_error_trace_does_not_select_a_non_failing_state() {
    for workers in [1, 4] {
        let model = View { initial_violation: true, initial_error: true, names: vec!["Property".into()], ..Default::default() };
        let r = explore(&model, &Options { workers, ..Default::default() });
        assert!(matches!(&r.outcome, Outcome::Error(e) if e.message == "hidden error"));
        assert_eq!(r.trace.iter().map(|t| t.state).collect::<Vec<_>>(), [(0, 1)]);
    }
}

#[test]
fn terminal_constraint_error_is_not_a_valid_action_property_counterexample() {
    for workers in [1, 4] {
        let model = View { constraint_error: true, names: vec!["Property".into()], ..Default::default() };
        let r = explore(&model, &Options { workers, ..Default::default() });
        assert_eq!(r.outcome, Outcome::ActionPropertyViolated("Property".into()));
        assert_eq!(r.trace.iter().map(|t| t.state).collect::<Vec<_>>(), [(0, 0), (0, 1)]);
    }
}

#[derive(Default)]
struct Branches {
    init_calls: AtomicUsize,
    names: Vec<String>,
    deadlock: bool,
    stats: bool,
}

impl Model for Branches {
    type State = (u32, u32);
    fn check_init_property(&self, _: &Self::State) -> Result<Option<String>, ModelError> {
        Ok(None)
    }
    fn constraint(&self, s: &Self::State) -> Result<bool, ModelError> {
        Ok(!self.deadlock || *s != (2, 1))
    }
    fn action_constraint(&self, _: &Self::State, _: &Self::State) -> Result<bool, ModelError> {
        Ok(true)
    }
    fn check_transition(&self, _: &Self::State, _: &Self::State) -> Result<Option<usize>, ModelError> {
        Ok(None)
    }
    fn action_property_names(&self) -> &[String] {
        &[]
    }
    fn extra_generated(&self) -> u64 {
        0
    }
    fn stop_requested(&self) -> bool {
        false
    }

    fn coarse_fingerprint(&self) -> bool {
        true
    }
    fn fingerprint(&self, s: &Self::State) -> u64 {
        s.0 as u64 + 1
    }
    fn init_states(&self, out: &mut dyn FnMut(Self::State)) -> Result<(), ModelError> {
        if self.stats {
            self.init_calls.fetch_add(1, Ordering::Relaxed);
            out((0, 0));
            return Ok(());
        }
        let mut initial = [(0, 0), (0, 1)];
        if self.init_calls.fetch_add(1, Ordering::Relaxed) > 0 {
            initial.reverse();
        }
        for s in initial {
            out(s);
        }
        Ok(())
    }
    fn successors(&self, s: &Self::State, out: &mut dyn FnMut(Self::State, u32) -> bool) -> Result<(), ModelError> {
        let mut next = match s {
            (0, 0) => vec![(1, 0), (1, 1)],
            (0, 1) => vec![(1, 1)],
            (1, hidden) => vec![(2, *hidden)],
            _ => Vec::new(),
        };
        if self.init_calls.load(Ordering::Relaxed) > 1 {
            next.reverse();
        }
        for t in next {
            if !out(t, 0) {
                break;
            }
        }
        Ok(())
    }
    fn check_invariants(&self, s: &Self::State) -> Result<Option<usize>, ModelError> {
        let violated = if self.deadlock {
            false
        } else if self.stats {
            tlx_runtime::live::current().unwrap_or_default().generated >= 2
        } else {
            *s == (2, 0)
        };
        Ok(violated.then_some(0))
    }
    fn invariant_names(&self) -> &[String] {
        &self.names
    }
    fn action_names(&self) -> &[String] {
        &self.names
    }
    fn render(&self, s: &Self::State) -> String {
        format!("{s:?}")
    }
}

#[test]
fn replay_backtracks_over_initial_and_intermediate_classes() {
    for workers in [1, 4] {
        let model = Branches { names: vec!["Inv".into()], ..Default::default() };
        let r = explore(&model, &Options { workers, ..Default::default() });
        assert_eq!(r.outcome, Outcome::InvariantViolated("Inv".into()));
        assert_eq!(r.trace.iter().map(|t| t.state).collect::<Vec<_>>(), [(0, 0), (1, 0), (2, 0)]);
    }
}

#[test]
fn deadlock_trace_ends_in_an_admitted_state() {
    for workers in [1, 4] {
        let model = Branches { deadlock: true, names: vec!["Inv".into()], ..Default::default() };
        let r = explore(&model, &Options { workers, ..Default::default() });
        assert_eq!(r.outcome, Outcome::Deadlock);
        assert_eq!(r.trace.iter().map(|t| t.state).collect::<Vec<_>>(), [(0, 0), (1, 0), (2, 0)]);
    }
}

#[test]
fn replay_checks_the_terminal_property_with_live_search_statistics() {
    for workers in [1, 4] {
        let model = Branches { stats: true, names: vec!["Inv".into()], ..Default::default() };
        let r = explore(&model, &Options { workers, ..Default::default() });
        assert_eq!(r.outcome, Outcome::InvariantViolated("Inv".into()));
        assert_eq!(r.trace.len(), 2);
    }
}
