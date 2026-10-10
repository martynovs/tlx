//! A search of several workers with a coarse fingerprint (as under VIEW or
//! SYMMETRY: `Model::coarse_fingerprint`) chooses the representative of
//! every fingerprint class as the single-worker BFS does (the first in BFS
//! order), so that its results are those of one worker.

use tlx_runtime::{explore, explore_resumable, FpConfig, Model, ModelError, Options, Outcome, Report};
use tlx_testkit::report::{project, ComparisonPolicy, Counts, Projection};

type S = (u32, u32, u32, u32);

/// `tests/specs/VR1` (wider): counters `a, b, c` below `n` and a hidden
/// `h` that the fingerprint ignores but the guards read. Expanding a state
/// costs about `spin` iterations of a busy loop, so that levels are
/// expanded in parallel rather than by a single worker.
struct Hidden {
    n: u32,
    spin: u32,
    /// The invariant: the state is not one of these (only representatives
    /// are checked).
    bad: Vec<S>,
    /// Checking the invariant on this state fails (an evaluation error).
    trap: Option<S>,
    inv: Vec<String>,
    acts: Vec<String>,
}

impl Hidden {
    fn new(n: u32, spin: u32) -> Hidden {
        Hidden { n, spin, bad: Vec::new(), trap: None, inv: vec!["Inv".into()], acts: vec!["A".into(), "B".into(), "C".into(), "D".into()] }
    }
}

impl Model for Hidden {
    type State = S;
    tlx_runtime::model_without_checks!(but coarse_fingerprint);
    fn init_states(&self, out: &mut dyn FnMut(S)) -> Result<(), ModelError> {
        out((0, 0, 0, 0));
        out((0, 0, 0, 3));
        out((1, 0, 0, 5));
        out((0, 0, 0, 6));
        Ok(())
    }
    fn successors(&self, s: &S, out: &mut dyn FnMut(S, u32) -> bool) -> Result<(), ModelError> {
        let mut x = 0u64;
        for i in 0..self.spin {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(i as u64));
        }
        let (a, b, c, h) = *s;
        let n = self.n;
        if a < n && h != 4 && !out((a + 1, b, c, (h * 3 + b + 1) % 7), 0) {
            return Ok(());
        }
        if b < n && h != 5 && !out((a, b + 1, c, (h + a + c) % 7), 1) {
            return Ok(());
        }
        if c < n && h < 5 && !out((a, b, c + 1, (h * 2 + a) % 7), 2) {
            return Ok(());
        }
        if h == 6 {
            out(((a + b) % (n + 1), b, c, 0), 3);
        }
        Ok(())
    }
    fn fingerprint(&self, s: &S) -> u64 {
        ((s.0 as u64) << 40 | (s.1 as u64) << 20 | s.2 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x1234
    }
    fn coarse_fingerprint(&self) -> bool {
        true
    }
    fn check_invariants(&self, s: &S) -> Result<Option<usize>, ModelError> {
        if Some(*s) == self.trap {
            return Err(ModelError::new("trapped"));
        }
        Ok(self.bad.contains(s).then_some(0))
    }
    fn invariant_names(&self) -> &[String] {
        &self.inv
    }
    fn action_names(&self) -> &[String] {
        &self.acts
    }
    fn render(&self, s: &S) -> String {
        format!("/\\ a = {}\n/\\ b = {}\n/\\ c = {}\n/\\ h = {}", s.0, s.1, s.2, s.3)
    }
}

/// What must not depend on the number of workers.
fn summary(r: &Report<S>) -> Projection<S> {
    project(r, ComparisonPolicy { outdegree: Counts::Always, ..ComparisonPolicy::COUNTS_AND_LENGTH })
}

fn opts(workers: usize) -> Options {
    Options { workers, check_deadlock: false, ..Default::default() }
}

/// The representatives (hence counts and outdegrees) of one worker, with
/// levels expanded in parallel or by one worker, with the fingerprint set
/// growing at safepoints and test safepoints.
#[test]
fn several_workers_choose_bfs_representatives() {
    let m = Hidden::new(14, 3000);
    let one = summary(&explore(&m, &opts(1)));
    assert_eq!(one.outcome, Outcome::Ok);
    let tiny = FpConfig::growing(1 << 20).with_initial(1024);
    for workers in [2, 3, 8] {
        for run in 0..6 {
            let o = Options {
                fp: if run % 2 == 1 { tiny } else { FpConfig::default() },
                safepoint_every: if run % 3 == 2 { 7 } else { 0 },
                ..opts(workers)
            };
            assert_eq!(summary(&explore(&m, &o)), one, "{workers} workers, run {run}");
        }
    }
    // Narrow levels expanded by one worker.
    let fast = Hidden::new(14, 0);
    let one = summary(&explore(&fast, &opts(1)));
    for workers in [2, 8] {
        assert_eq!(summary(&explore(&fast, &opts(workers))), one, "{workers} workers");
    }
}

/// Only the representatives are checked against the invariant: states of
/// their classes found later (in BFS order) that violate it are not
/// reported, whichever worker finds them first.
#[test]
fn invariant_of_representatives_only() {
    let mut m = Hidden::new(14, 3000);
    let r1 = explore(&m, &opts(1));
    // States of the same class (a, b, c) as a representative, with another h.
    let mut probe = Hidden::new(14, 0);
    probe.bad = (0..7).flat_map(|h| [(1, 0, 1, h), (0, 1, 1, h), (1, 1, 1, h), (2, 1, 1, h)]).collect();
    let reps: Vec<S> = probe
        .bad
        .iter()
        .copied()
        .filter(|s| explore(&Hidden { bad: vec![*s], ..Hidden::new(14, 0) }, &opts(1)).outcome != Outcome::Ok)
        .collect();
    assert!(!reps.is_empty());
    m.bad = probe.bad.iter().copied().filter(|s| !reps.contains(s)).collect();
    assert_eq!(summary(&explore(&m, &opts(1))), summary(&r1));
    for workers in [2, 4, 8] {
        for _ in 0..4 {
            assert_eq!(summary(&explore(&m, &opts(workers))), summary(&r1), "{workers} workers");
        }
    }
    // A representative violating it is found.
    m.bad = reps.clone();
    for workers in [1, 4] {
        let r = explore(&m, &opts(workers));
        assert_eq!(r.outcome, Outcome::InvariantViolated("Inv".into()), "{workers} workers");
        assert!(reps.contains(&r.trace.last().unwrap().state));
    }
}

/// The representative of class `(a, b, c)` (with one worker).
fn representative((a, b, c): (u32, u32, u32)) -> S {
    (0..7)
        .map(|h| (a, b, c, h))
        .find(|&s| explore(&Hidden { bad: vec![s], ..Hidden::new(14, 0) }, &opts(1)).outcome != Outcome::Ok)
        .expect("a reachable class")
}

/// A search stopped by an evaluation error while checking the
/// representatives of a level, resumed from its resume point, ends like an
/// uninterrupted one.
#[test]
fn resumed_search_keeps_bfs_representatives() {
    let m = Hidden::new(14, 3000);
    for workers in [3, 8] {
        let want = summary(&explore(&m, &opts(workers)));
        let mut from = None;
        for trap in [(2, 1, 0), (5, 3, 2), (9, 7, 4)].map(representative) {
            let tm = Hidden { trap: Some(trap), ..Hidden::new(14, 3000) };
            let (r, c) = explore_resumable(&tm, &opts(workers), from.take(), Some(usize::MAX));
            let Outcome::Error(e) = &r.outcome else { panic!("trap {trap:?} not reached: {:?}", r.outcome) };
            assert_eq!(e.message, "trapped");
            from = Some(c.expect("resume point").try_map(2, Ok::<_, ()>).unwrap());
        }
        let (r, c) = explore_resumable(&m, &opts(workers), from, None);
        assert!(c.is_none());
        assert_eq!(summary(&r), want, "{workers} workers");
    }
}

/// A fingerprint set too small stops the search (also while offering
/// candidates) with `Outcome::FpSetFull`.
#[test]
fn full_fingerprint_set() {
    let m = Hidden::new(14, 0);
    for workers in [1, 4] {
        let r = explore(&m, &Options { fp: FpConfig::fixed(0), ..opts(workers) });
        assert!(matches!(r.outcome, Outcome::FpSetFull { entries: 922 }), "{:?} with {workers} workers", r.outcome);
    }
}

/// With the fingerprint set rotating to disk (the smallest tables, from the
/// start), the representatives and results do not change: the seen-set
/// insertion under the shard lock asks the disk tier too.
#[test]
fn representatives_with_the_disk_tier() {
    use tlx_runtime::io::{Spill, SpillConfig, SpillMode};
    let disk = |workers: usize, n: u64| Options {
        fp: FpConfig::default().with_disk_slots(1024),
        spill: Spill::new(SpillConfig {
            mode: SpillMode::Always,
            dir: Some(std::env::temp_dir().join(format!("tlx-coarse-disk-{}-{workers}-{n}", std::process::id()))),
            ..Default::default()
        }),
        ..opts(workers)
    };
    let m = Hidden::new(14, 300);
    let one = summary(&explore(&m, &opts(1)));
    assert_eq!(one.outcome, Outcome::Ok);
    assert!(one.counts.as_ref().unwrap().distinct > 4 * 768, "{one:?}");
    for workers in [1, 2, 8] {
        for run in 0..3 {
            assert_eq!(summary(&explore(&m, &disk(workers, run))), one, "{workers} workers, run {run}");
        }
    }
    // A representative violating the invariant, found deep.
    let rep = representative((14, 13, 12));
    let bad = Hidden { bad: vec![rep], ..Hidden::new(14, 300) };
    let r1 = explore(&bad, &opts(1));
    assert_eq!(r1.outcome, Outcome::InvariantViolated("Inv".into()));
    for workers in [1, 8] {
        let r = explore(&bad, &disk(workers, 9));
        // Preserve the failure policy: verdict, shortest length and the chosen representative.
        assert_eq!(r.outcome, r1.outcome, "{workers} workers");
        assert_eq!(r.trace.len(), r1.trace.len(), "{workers} workers");
        assert_eq!(r.trace.last().map(|t| t.state), Some(rep), "{workers} workers");
    }
}

/// The candidates buffered at a level count in the memory budget:
/// under a budget the search exceeds from the start (every pressure stage,
/// the fingerprint set on disk), the representatives and results do not
/// change.
#[test]
fn representatives_under_a_tiny_budget() {
    use tlx_runtime::io::{Spill, SpillConfig};
    let m = Hidden::new(60, 0);
    let one = summary(&explore(&m, &opts(1)));
    assert_eq!(one.outcome, Outcome::Ok);
    for workers in [2, 8] {
        let dir = std::env::temp_dir().join(format!("tlx-coarse-budget-{}-{workers}", std::process::id()));
        let o = Options { maxmem: 1 << 20, spill: Spill::new(SpillConfig { dir: Some(dir), ..Default::default() }), ..opts(workers) };
        assert_eq!(summary(&explore(&m, &o)), one, "{workers} workers");
    }
}
