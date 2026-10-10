//! Search observers (`explore_observed`): what they see, and that watching
//! a search does not change it.

use std::sync::Mutex;

use tlx_runtime::{explore, explore_observed, Model, ModelError, Options, Outcome, SearchObserver};
use tlx_testkit::toy::Counters;

type S = (u32, u32);

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Init(S, u64),
    New(S, u32, Option<u64>),
    Edge(u64, u64, u32),
    Expanded(S, Vec<(S, u32)>, bool),
}

/// Records what it sees; fails `on_init` at the `fail_init`-th initial
/// state and `on_expanded` (buffering) at state `fail_at`.
#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<Event>>,
    buffers: bool,
    fail_init: Option<u64>,
    fail_at: Option<S>,
}

impl SearchObserver<S> for Recorder {
    fn buffers(&self) -> bool {
        self.buffers
    }

    fn on_init(&self, s: &S, k: u64) -> Result<(), ModelError> {
        self.events.lock().unwrap().push(Event::Init(*s, k));
        if self.fail_init == Some(k) {
            return Err(ModelError::new("observer: init"));
        }
        Ok(())
    }

    fn on_new_state(&self, s: &S, _fp: u64, depth: u32, parent: Option<u64>) {
        self.events.lock().unwrap().push(Event::New(*s, depth, parent));
    }

    fn on_edge(&self, from: u64, to: u64, action: u32) {
        self.events.lock().unwrap().push(Event::Edge(from, to, action));
    }

    fn on_expanded(&self, s: &S, succ: &[(S, u32)], complete: bool) -> Result<(), ModelError> {
        self.events.lock().unwrap().push(Event::Expanded(*s, succ.to_vec(), complete));
        if self.fail_at == Some(*s) {
            return Err(ModelError::new("observer: expanded"));
        }
        Ok(())
    }
}

fn opts(workers: usize) -> Options {
    Options { workers, check_deadlock: false, ..Options::default() }
}

#[test]
fn an_observed_search_is_the_search_and_sees_its_states_and_edges() {
    let m = Counters::new(2, None, 2);
    for workers in [1, 4] {
        let plain = explore(&m, &opts(workers));
        let obs = Recorder::default();
        let r = explore_observed(&m, &opts(workers), &obs);
        assert_eq!((r.stats.clone(), r.outcome.clone()), (plain.stats.clone(), plain.outcome.clone()), "workers {workers}");
        let events = obs.events.into_inner().unwrap();
        let count = |f: &dyn Fn(&Event) -> bool| events.iter().filter(|e| f(e)).count() as u64;
        assert_eq!(count(&|e| matches!(e, Event::New(..))), r.stats.distinct);
        assert_eq!(count(&|e| matches!(e, Event::Edge(..))), r.stats.generated - r.stats.init_generated);
        assert_eq!(count(&|e| matches!(e, Event::Init(..))), 1);
        if workers == 1 {
            let fp = |s: S| m.fingerprint(&s);
            let expected = [
                Event::New((0, 0), 1, None),
                Event::Init((0, 0), 0),
                Event::Edge(fp((0, 0)), fp((1, 0)), 0),
                Event::New((1, 0), 2, Some(fp((0, 0)))),
                Event::Edge(fp((0, 0)), fp((0, 1)), 1),
                Event::New((0, 1), 2, Some(fp((0, 0)))),
                Event::Edge(fp((1, 0)), fp((2, 0)), 0),
                Event::New((2, 0), 3, Some(fp((1, 0)))),
                Event::Edge(fp((1, 0)), fp((1, 1)), 1),
                Event::New((1, 1), 3, Some(fp((1, 0)))),
                Event::Edge(fp((0, 1)), fp((1, 1)), 0),
                Event::Edge(fp((0, 1)), fp((0, 2)), 1),
                Event::New((0, 2), 3, Some(fp((0, 1)))),
            ];
            assert_eq!(events[..expected.len()], expected);
        }
    }
}

#[test]
fn an_error_of_the_observer_on_an_initial_state_stops_the_search_there() {
    let m = Counters::new(2, None, 2);
    let obs = Recorder { fail_init: Some(0), ..Recorder::default() };
    let r = explore_observed(&m, &opts(1), &obs);
    assert_eq!(r.outcome, Outcome::Error(ModelError::new("observer: init")));
    assert!(r.in_init);
    // (The second copy of the initial state is not generated.)
    assert_eq!((r.stats.init_generated, r.stats.distinct), (1, 1));
}

#[test]
fn a_buffering_observer_sees_the_successors_before_they_are_processed() {
    let m = Counters::new(2, None, 1);
    let obs = Recorder { buffers: true, fail_at: Some((1, 0)), ..Recorder::default() };
    let r = explore_observed(&m, &opts(1), &obs);
    // (In the context of the successors, unless it has its own.)
    assert_eq!(r.outcome, Outcome::Error(ModelError::new("observer: expanded").within(tlx_runtime::ErrorContext::NextState)));
    assert_eq!(r.trace.iter().map(|t| t.state).collect::<Vec<_>>(), [(0, 0), (1, 0)]);
    let events = obs.events.into_inner().unwrap();
    let fp = |s: S| m.fingerprint(&s);
    // The successors of (1, 0) are processed (found) before the search
    // stops at the observer's error.
    let expected = [
        Event::New((0, 0), 1, None),
        Event::Init((0, 0), 0),
        Event::Expanded((0, 0), vec![((1, 0), 0), ((0, 1), 1)], true),
        Event::Edge(fp((0, 0)), fp((1, 0)), 0),
        Event::New((1, 0), 2, Some(fp((0, 0)))),
        Event::Edge(fp((0, 0)), fp((0, 1)), 1),
        Event::New((0, 1), 2, Some(fp((0, 0)))),
        Event::Expanded((1, 0), vec![((2, 0), 0), ((1, 1), 1)], true),
        Event::Edge(fp((1, 0)), fp((2, 0)), 0),
        Event::New((2, 0), 3, Some(fp((1, 0)))),
        Event::Edge(fp((1, 0)), fp((1, 1)), 1),
        Event::New((1, 1), 3, Some(fp((1, 0)))),
    ];
    assert_eq!(events, expected);
    assert_eq!(r.stats.distinct, 5);

    // A violation among the successors is reported first.
    let m = Counters::new(2, Some((2, 0)), 1);
    let obs = Recorder { buffers: true, fail_at: Some((1, 0)), ..Recorder::default() };
    let r = explore_observed(&m, &opts(1), &obs);
    assert_eq!(r.outcome, Outcome::InvariantViolated("NotBad".into()));
    assert_eq!(r.trace.last().map(|t| t.state), Some((2, 0)));
}

/// Stops the search once it found `at` distinct states (with `init`:
/// among the initial states); quiet if `quiet`.
struct Stopper {
    at: u64,
    init: bool,
    quiet: bool,
    seen: Mutex<Vec<S>>,
}

impl SearchObserver<S> for Stopper {
    fn quiet(&self) -> bool {
        self.quiet
    }

    fn on_new_state(&self, s: &S, _fp: u64, _depth: u32, _parent: Option<u64>) {
        self.seen.lock().unwrap().push(*s);
    }

    fn stop(&self, init: bool, distinct: u64) -> bool {
        (init || !self.init) && distinct >= self.at
    }
}

#[test]
fn an_observer_stops_the_search_after_a_successor_or_an_initial_state() {
    // After the successor that makes 4 distinct states, (2, 0): the rest of
    // the successors of (1, 0) are not processed, and the states found
    // before are left.
    let m = Counters::new(2, None, 1);
    let obs = Stopper { at: 4, init: false, quiet: false, seen: Mutex::default() };
    let r = explore_observed(&m, &opts(1), &obs);
    assert_eq!(r.outcome, Outcome::Ok);
    assert_eq!((r.stats.distinct, r.stats.generated, r.stats.left), (4, 4, 2));
    assert_eq!(obs.seen.into_inner().unwrap(), [(0, 0), (1, 0), (0, 1), (2, 0)]);
    // Before the first state is expanded.
    let obs = Stopper { at: 1, init: false, quiet: false, seen: Mutex::default() };
    let r = explore_observed(&m, &opts(1), &obs);
    assert_eq!((r.stats.distinct, r.stats.generated, r.stats.left), (1, 1, 1));
    // Among the initial states: no more are processed (the copies of the
    // initial state are not generated again).
    let m = Counters::new(2, None, 3);
    let obs = Stopper { at: 1, init: true, quiet: false, seen: Mutex::default() };
    let r = explore_observed(&m, &opts(1), &obs);
    assert_eq!((r.stats.init_generated, r.stats.distinct, r.stats.left), (1, 1, 1));
}

#[test]
fn a_quiet_search_rebuilds_no_trace() {
    let m = Counters::new(2, Some((1, 1)), 1);
    for quiet in [false, true] {
        let obs = Stopper { at: u64::MAX, init: false, quiet, seen: Mutex::default() };
        let r = explore_observed(&m, &opts(1), &obs);
        assert_eq!(r.outcome, Outcome::InvariantViolated("NotBad".into()));
        assert_eq!(r.trace.len(), if quiet { 0 } else { 3 }, "quiet {quiet}");
    }
}
