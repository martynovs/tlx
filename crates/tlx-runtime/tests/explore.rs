use tlx_runtime::{explore, FpConfig, Model, ModelError, Options, Outcome};
use tlx_testkit::report::{project, ComparisonPolicy, Counts, TracePolicy};

use tlx_testkit::toy::Counters;

#[test]
fn counts_and_deadlock() {
    for workers in [1, 4] {
        let m = Counters::new(30, None, 2);
        let r = explore(&m, &Options { workers, check_deadlock: false, ..Default::default() });
        assert_eq!(r.outcome, Outcome::Ok);
        assert_eq!(r.stats.distinct, 31 * 31);
        assert_eq!(r.stats.init_generated, 2);
        assert_eq!(r.stats.generated, 2 + 2 * 30 * 31);
        assert_eq!(r.stats.depth, 61);
        let r = explore(&m, &Options { workers, ..Default::default() });
        assert_eq!(r.outcome, Outcome::Deadlock);
        assert_eq!(r.trace.last().unwrap().state, (30, 30));
    }
}

#[test]
fn invariant_trace() {
    for workers in [1, 8] {
        let m = Counters::new(50, Some((3, 4)), 2);
        let r = explore(&m, &Options { workers, check_deadlock: false, ..Default::default() });
        assert_eq!(r.outcome, Outcome::InvariantViolated("NotBad".into()));
        let t = &r.trace;
        assert_eq!(t[0].state, (0, 0));
        assert_eq!(t.last().unwrap().state, (3, 4));
        assert_eq!(t.len(), 8, "BFS gives a shortest trace");
        for w in t.windows(2) {
            let (a, b) = (w[0].state, w[1].state);
            assert_eq!(a.0 + a.1 + 1, b.0 + b.1);
        }
    }
}

/// A layered graph: long stretches of narrow levels, a few wide ones, and
/// shortcut edges (so that the BFS depth differs from the layer number).
struct Layers {
    layers: u32,
    bad: Option<(u32, u32)>,
    inv: Vec<String>,
    acts: Vec<String>,
}

impl Layers {
    fn new(layers: u32, bad: Option<(u32, u32)>) -> Layers {
        Layers { layers, bad, inv: vec!["NotBad".into()], acts: vec!["Step".into(), "Jump".into()] }
    }

    fn width(l: u32) -> u32 {
        if l % 500 >= 100 && l % 500 < 104 {
            3000
        } else if l.is_multiple_of(7) {
            3
        } else {
            1
        }
    }
}

impl Model for Layers {
    type State = (u32, u32);
    tlx_runtime::model_without_checks!();
    fn init_states(&self, out: &mut dyn FnMut((u32, u32))) -> Result<(), ModelError> {
        out((0, 0));
        Ok(())
    }
    fn successors(&self, s: &(u32, u32), out: &mut dyn FnMut((u32, u32), u32) -> bool) -> Result<(), ModelError> {
        let (l, i) = *s;
        if l + 1 >= self.layers {
            return Ok(());
        }
        let (w, w1) = (Layers::width(l), Layers::width(l + 1));
        if w1 >= w {
            for j in i * w1 / w..(i + 1) * w1 / w {
                if !out((l + 1, j), 0) {
                    return Ok(());
                }
            }
        } else if !out((l + 1, i % w1), 0) {
            return Ok(());
        }
        if i == 0 && l % 3 == 0 && l + 2 < self.layers {
            out((l + 2, Layers::width(l + 2) - 1), 1);
        }
        Ok(())
    }
    fn fingerprint(&self, s: &(u32, u32)) -> u64 {
        ((s.0 as u64) << 32 | s.1 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5678
    }
    fn check_invariants(&self, s: &(u32, u32)) -> Result<Option<usize>, ModelError> {
        Ok(if Some(*s) == self.bad { Some(0) } else { None })
    }
    fn invariant_names(&self) -> &[String] {
        &self.inv
    }
    fn action_names(&self) -> &[String] {
        &self.acts
    }
    fn render(&self, s: &(u32, u32)) -> String {
        format!("/\\ l = {}\n/\\ i = {}", s.0, s.1)
    }
}

/// The level-synchronous search with several workers (narrow levels expanded
/// by one worker, wide ones in parallel) gives the depth and shortest traces
/// of the single-worker BFS.
#[test]
fn multi_worker_matches_bfs() {
    let summary = |m: &Layers, workers: usize, check_deadlock: bool| {
        let r = explore(m, &Options { workers, check_deadlock, ..Default::default() });
        project(
            &r,
            ComparisonPolicy { counts: Counts::OnSuccess, trace: TracePolicy::LengthAndLast, ..ComparisonPolicy::COUNTS_AND_LENGTH },
        )
    };
    let cases = [
        (Layers::new(2000, None), false),
        (Layers::new(2000, None), true),
        (Layers::new(2000, Some((1600, 2))), false),
        (Layers::new(700, Some((602, 1234))), false),
    ];
    for (m, deadlock) in &cases {
        let one = summary(m, 1, *deadlock);
        for workers in [2, 3, 8] {
            assert_eq!(summary(m, workers, *deadlock), one, "{workers} workers");
        }
    }
    let ok = summary(&cases[0].0, 1, false);
    assert_eq!(ok.outcome, Outcome::Ok);
    assert!(ok.depth < 2000, "shortcuts shorten the BFS depth");
    assert_eq!(summary(&cases[1].0, 1, true).outcome, Outcome::Deadlock);
    assert_eq!(summary(&cases[2].0, 1, false).outcome, Outcome::InvariantViolated("NotBad".into()));
    assert_eq!(summary(&cases[3].0, 1, false).outcome, Outcome::InvariantViolated("NotBad".into()));
}

/// `Layers` whose expansion fails when it generates one of `traps` (after
/// the successors generated before it), as a compiled model on a layout miss.
struct Trapped<'a> {
    inner: &'a Layers,
    traps: Vec<(u32, u32)>,
}

impl Model for Trapped<'_> {
    type State = (u32, u32);
    tlx_runtime::model_without_checks!();
    fn init_states(&self, out: &mut dyn FnMut((u32, u32))) -> Result<(), ModelError> {
        self.inner.init_states(out)
    }
    fn successors(&self, s: &(u32, u32), out: &mut dyn FnMut((u32, u32), u32) -> bool) -> Result<(), ModelError> {
        let mut trapped = false;
        self.inner.successors(s, &mut |t, a| {
            if self.traps.contains(&t) {
                trapped = true;
                return false;
            }
            out(t, a)
        })?;
        if trapped {
            return Err(ModelError::new("trapped"));
        }
        Ok(())
    }
    fn fingerprint(&self, s: &(u32, u32)) -> u64 {
        self.inner.fingerprint(s)
    }
    fn check_invariants(&self, s: &(u32, u32)) -> Result<Option<usize>, ModelError> {
        self.inner.check_invariants(s)
    }
    fn invariant_names(&self) -> &[String] {
        self.inner.invariant_names()
    }
    fn action_names(&self) -> &[String] {
        self.inner.action_names()
    }
    fn render(&self, s: &(u32, u32)) -> String {
        self.inner.render(s)
    }
}

/// A search stopped by an error and resumed from its resume point (possibly
/// several times) ends like an uninterrupted one: same statistics, same
/// shortest traces.
#[test]
fn resumed_search_matches_uninterrupted() {
    let summary = |r: &tlx_runtime::Report<(u32, u32)>| {
        project(
            r,
            ComparisonPolicy {
                counts: Counts::OnSuccess,
                outdegree: Counts::OnSuccess,
                init_distinct: true,
                trace: TracePolicy::LengthAndLast,
                ..ComparisonPolicy::COUNTS_AND_LENGTH
            },
        )
    };
    let cases = [
        (Layers::new(2000, None), vec![vec![(5, 0)], vec![(101, 7)], vec![(1500, 0), (1501, 0)]]),
        (Layers::new(700, Some((602, 1234))), vec![vec![(1, 0)], vec![(102, 2999)]]),
        (Layers::new(700, None), vec![vec![(1, 0)], vec![(2, 0)], vec![(3, 0)]]),
    ];
    for (m, traps) in &cases {
        for workers in [1, 3, 8] {
            let opts = Options { workers, check_deadlock: false, ..Default::default() };
            let want = summary(&explore(m, &opts));
            let mut from = None;
            for t in traps {
                let tm = Trapped { inner: m, traps: t.clone() };
                let (r, c) = tlx_runtime::explore_resumable(&tm, &opts, from.take(), Some(usize::MAX));
                assert!(matches!(&r.outcome, Outcome::Error(e) if e.message == "trapped"), "{t:?}");
                let c = c.expect("resume point");
                assert!(c.level() <= t[0].0, "the level of the trap's parent at most");
                from = Some(c.try_map(2, Ok::<_, ()>).unwrap());
            }
            let (r, c) = tlx_runtime::explore_resumable(m, &opts, from, None);
            assert!(c.is_none());
            assert_eq!(summary(&r), want, "{workers} workers, traps {traps:?}");
        }
    }
    // Too many states to retain: no resume point.
    let m = Layers::new(700, None);
    let tm = Trapped { inner: &m, traps: vec![(600, 0)] };
    let (r, c) = tlx_runtime::explore_resumable(&tm, &Options::default(), None, Some(5000));
    assert!(matches!(&r.outcome, Outcome::Error(e) if e.message == "trapped"));
    assert!(c.is_none());
}

/// A model whose successor function panics, once, on the first thread to
/// expand state `at` (as a `TLX_PARANOID` mismatch does); the others go on.
struct Panicky<'a, M> {
    inner: &'a M,
    at: (u32, u32),
    fired: std::sync::atomic::AtomicBool,
}

impl<M: Model<State = (u32, u32)>> Model for Panicky<'_, M> {
    type State = (u32, u32);
    tlx_runtime::model_without_checks!();
    fn init_states(&self, out: &mut dyn FnMut((u32, u32))) -> Result<(), ModelError> {
        self.inner.init_states(out)
    }
    fn successors(&self, s: &(u32, u32), out: &mut dyn FnMut((u32, u32), u32) -> bool) -> Result<(), ModelError> {
        if *s == self.at && !self.fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
            panic!("injected panic at {s:?}");
        }
        self.inner.successors(s, out)
    }
    fn action_successors(&self, s: &(u32, u32), a: u32, out: &mut dyn FnMut((u32, u32), u32) -> bool) -> Result<bool, ModelError> {
        if *s == self.at && !self.fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
            panic!("injected panic at {s:?}");
        }
        self.inner.action_successors(s, a, out)
    }
    fn fingerprint(&self, s: &(u32, u32)) -> u64 {
        self.inner.fingerprint(s)
    }
    fn check_invariants(&self, s: &(u32, u32)) -> Result<Option<usize>, ModelError> {
        self.inner.check_invariants(s)
    }
    fn invariant_names(&self) -> &[String] {
        self.inner.invariant_names()
    }
    fn action_names(&self) -> &[String] {
        self.inner.action_names()
    }
    fn render(&self, s: &(u32, u32)) -> String {
        self.inner.render(s)
    }
}

/// Run `f` on another thread: the message of its panic, or a failure if it
/// neither panics nor returns within a minute (a hang).
fn panic_message(f: impl FnOnce() + Send + 'static) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        let _ = tx.send(r.err().map(|p| p.downcast_ref::<String>().cloned().unwrap_or_default()));
    });
    rx.recv_timeout(std::time::Duration::from_secs(60)).expect("the search hangs after a worker panicked")
}

/// A panicking worker (e.g. a TLX_PARANOID mismatch) aborts the whole
/// search with its panic, with one or several workers (in a level expanded
/// alone or in parallel), and with progress reports: nobody waits for it
/// forever.
#[test]
fn worker_panic_aborts_search() {
    let progress = || {
        Some(tlx_runtime::Progress {
            first: std::time::Duration::from_millis(1),
            every: std::time::Duration::from_millis(1),
            report: std::sync::Arc::new(|_: &tlx_runtime::LiveStats| {}),
            init_done: std::sync::Arc::new(|_, _| {}),
        })
    };
    for at in [(5, 0), (101, 7), (1500, 0)] {
        for workers in [1, 3, 8] {
            let msg = panic_message(move || {
                let m = Layers::new(2000, None);
                let pm = Panicky { inner: &m, at, fired: Default::default() };
                explore(&pm, &Options { workers, check_deadlock: false, progress: progress(), ..Default::default() });
            });
            assert_eq!(msg.as_deref(), Some(&*format!("injected panic at {at:?}")), "{workers} workers, at {at:?}");
        }
    }
}

/// Likewise in simulation (whose behaviors would go on forever otherwise).
#[test]
fn worker_panic_aborts_simulation() {
    for workers in [1, 4] {
        let msg = panic_message(move || {
            let m = Counters::new(1000, None, 2);
            let pm = Panicky { inner: &m, at: (3, 2), fired: Default::default() };
            let opts = tlx_runtime::SimOptions { workers, check_deadlock: false, traces: u64::MAX, depth: 100, seed: 1, generate: false };
            tlx_runtime::simulate(&pm, &opts, &|_| {}, None);
        });
        assert_eq!(msg.as_deref(), Some("injected panic at (3, 2)"), "{workers} workers");
    }
}

/// A full fingerprint set (`fp` too small) stops the search with
/// `Outcome::FpSetFull`: a resource limit, reported with the number of
/// states the set holds and without a trace (not a violation of the
/// specification), also when the set fills while resuming from a resume point.
/// (A set that fills during the initial states is covered by the CLI test
/// `fpset_full.rs`, which has a spec with many initial states.)
#[test]
fn full_fingerprint_set() {
    // 31 * 31 = 961 states, more than the 922 of the smallest set
    // (`FpConfig::fixed(0)` gives the 1024-slot minimum).
    let m = Counters::new(30, None, 2);
    for workers in [1, 4] {
        let opts = Options { workers, check_deadlock: false, fp: FpConfig::fixed(0), ..Default::default() };
        let r = explore(&m, &opts);
        let Outcome::FpSetFull { entries } = r.outcome else { panic!("{:?} with {workers} workers", r.outcome) };
        assert_eq!(entries, 922);
        // Inserts of concurrent workers overshoot the load limit slightly.
        assert!((922..922 + workers as u64).contains(&r.stats.distinct), "{} distinct", r.stats.distinct);
        assert!(r.trace.is_empty(), "the state that could not be inserted is not a counterexample");
    }
    // A resume point's states are inserted again when the search resumes.
    let m = Layers::new(2000, None);
    let tm = Trapped { inner: &m, traps: vec![(101, 7)] };
    let opts = Options { workers: 4, check_deadlock: false, ..Default::default() };
    let (r, c) = tlx_runtime::explore_resumable(&tm, &opts, None, Some(usize::MAX));
    assert!(matches!(&r.outcome, Outcome::Error(e) if e.message == "trapped"), "{:?}", r.outcome);
    let from = c.expect("resume point").try_map(2, Ok::<_, ()>).unwrap();
    let small = Options { fp: FpConfig::fixed(0), ..opts };
    let r = tlx_runtime::explore_resumable(&m, &small, Some(from), None).0;
    assert!(matches!(r.outcome, Outcome::FpSetFull { entries } if entries == 922), "{:?}", r.outcome);
    assert!(r.in_init && r.trace.is_empty());
}

/// A fingerprint set that starts at the smallest table grows at safepoints
/// (in the middle of levels, with solo narrow levels, with workers also
/// parking for test safepoints) without changing the results; one that
/// cannot grow beyond its limit reports a full set without a disk tier
/// (`-spill off`), and rotates to disk with one. Resuming into a small
/// growing set grows it while inserting the resume point's states.
#[test]
fn growing_fingerprint_set() {
    let tiny = FpConfig::growing(1 << 20).with_initial(1024);
    let cases =
        [(Layers::new(2000, None), false), (Layers::new(2000, Some((1600, 2))), false), (Layers::new(700, Some((602, 1234))), false)];
    for (m, deadlock) in &cases {
        let r1 = explore(m, &Options { workers: 1, check_deadlock: *deadlock, ..Default::default() });
        for workers in [1, 3, 8] {
            for safepoint_every in [0, 7] {
                let r = explore(m, &Options { workers, check_deadlock: *deadlock, fp: tiny, safepoint_every, ..Default::default() });
                assert_eq!(r.outcome, r1.outcome, "{workers} workers");
                assert_eq!(r.stats.depth, r1.stats.depth, "{workers} workers");
                assert_eq!(r.trace.len(), r1.trace.len(), "{workers} workers");
                if r1.outcome == Outcome::Ok {
                    assert_eq!((r.stats.distinct, r.stats.generated), (r1.stats.distinct, r1.stats.generated), "{workers} workers");
                }
            }
        }
    }
    // 961 states; the table may grow from 1024 to 2048 slots only.
    let off = tlx_runtime::io::Spill::new(tlx_runtime::io::SpillConfig { mode: tlx_runtime::io::SpillMode::Off, ..Default::default() });
    for workers in [1, 4] {
        let opts = Options {
            workers,
            check_deadlock: false,
            fp: FpConfig::growing(2048).with_initial(1024),
            spill: off.clone(),
            ..Default::default()
        };
        let r = explore(&Counters::new(30, None, 2), &opts);
        assert_eq!((r.outcome, r.stats.distinct), (Outcome::Ok, 961));
        let r = explore(&Counters::new(60, None, 2), &opts);
        assert!(matches!(r.outcome, Outcome::FpSetFull { entries: 1844 }), "{:?} with {workers} workers", r.outcome);
        assert!(r.trace.is_empty());
        // (The disk tier, by default.)
        let r = explore(&Counters::new(60, None, 2), &Options { spill: Default::default(), ..opts });
        assert_eq!((r.outcome, r.stats.distinct), (Outcome::Ok, 61 * 61), "{workers} workers");
    }
    let m = Layers::new(2000, None);
    let full = explore(&m, &Options { workers: 4, check_deadlock: false, ..Default::default() });
    let tm = Trapped { inner: &m, traps: vec![(1600, 7)] };
    let opts = Options { workers: 4, check_deadlock: false, ..Default::default() };
    let (r, c) = tlx_runtime::explore_resumable(&tm, &opts, None, Some(usize::MAX));
    assert!(matches!(&r.outcome, Outcome::Error(e) if e.message == "trapped"), "{:?}", r.outcome);
    let from = c.expect("resume point").try_map(2, Ok::<_, ()>).unwrap();
    let r = tlx_runtime::explore_resumable(&m, &Options { fp: tiny, ..opts }, Some(from), None).0;
    assert_eq!((r.outcome, r.stats.distinct, r.stats.generated), (Outcome::Ok, full.stats.distinct, full.stats.generated));
}
