//! The disk layer in searches (docs/scale.md §10): trace logs spilled to
//! disk give the same results and traces; IO errors stop the search with a
//! clear message; pressure from the memory budget spills.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use tlx_runtime::io::{Spill, SpillConfig, SpillMode};
use tlx_runtime::{explore, FpConfig, Model, ModelError, Options, Outcome};

use tlx_testkit::toy::Counters;

/// A fresh spill directory for a test.
fn test_dir(name: &str) -> PathBuf {
    tlx_testkit::toy::spill_dir("tlx-spill-test", name)
}

fn spill(name: &str, mode: SpillMode, f: impl FnOnce(&mut SpillConfig)) -> Spill {
    let mut cfg = SpillConfig { mode, dir: Some(test_dir(name)), ..Default::default() };
    f(&mut cfg);
    Spill::new(cfg)
}

/// Options with small blocks (4 KiB: 256 trace-log entries).
fn opts(workers: usize, spill: Spill) -> Options {
    let mut o = Options { workers, check_deadlock: false, spill, ..Default::default() };
    o.queue.seg_bytes = 4096;
    o
}

#[test]
fn spilled_trace_logs_give_the_same_results() {
    let m = Counters::new(120, Some((100, 37)), 1);
    for workers in [1, 4] {
        let plain = explore(&m, &opts(workers, Spill::new(SpillConfig { mode: SpillMode::Off, ..Default::default() })));
        let sp = spill("same", SpillMode::Always, |_| {});
        let r = explore(&m, &opts(workers, sp.clone()));
        assert_eq!(r.outcome, Outcome::InvariantViolated("NotBad".into()));
        assert_eq!(r.outcome, plain.outcome);
        // (Several workers stop at a violation after a varying amount of
        // work.)
        if workers == 1 {
            assert_eq!(r.stats, plain.stats);
        }
        assert_eq!(r.stats.depth, plain.stats.depth);
        let states = |t: &[tlx_runtime::TraceStep<(u32, u32)>]| t.iter().map(|s| s.state).collect::<Vec<_>>();
        assert_eq!(r.trace.len(), 138);
        if workers == 1 {
            assert_eq!(states(&r.trace), states(&plain.trace));
        }
        assert_eq!(r.trace.last().unwrap().state, (100, 37));
        let c = sp.counters();
        assert!(c.blocks >= 10 && c.read > 0, "{c:?}");
        // The trace was read back; the directory is gone with the logs.
        assert!(sp.dir().is_none());
        assert!(sp.error().is_none());
        // A complete search: the same statistics.
        let ok = Counters::new(120, None, 1);
        let sp = spill("same-ok", SpillMode::Always, |_| {});
        let r = explore(&ok, &opts(workers, sp.clone()));
        assert_eq!(r.outcome, Outcome::Ok);
        assert_eq!(r.stats, explore(&ok, &opts(workers, Spill::new(SpillConfig { mode: SpillMode::Off, ..Default::default() }))).stats);
        assert!(sp.counters().blocks > 0);
    }
}

#[test]
fn write_error_stops_the_search() {
    let m = Counters::new(200, None, 1);
    for workers in [1, 4] {
        let sp = spill("inject", SpillMode::Always, |c| c.fail_after = Some(64 << 10));
        let r = explore(&m, &opts(workers, sp.clone()));
        match &r.outcome {
            Outcome::Io(msg) => assert!(msg.contains("injected write error") && msg.contains("trace-"), "{msg}"),
            o => panic!("{o:?}"),
        }
        assert!(r.trace.is_empty());
        assert!(r.stats.distinct < 201 * 201, "the search stopped early");
    }
}

#[test]
fn disk_quota_stops_the_search() {
    let m = Counters::new(200, None, 1);
    let sp = spill("quota", SpillMode::Always, |c| c.maxdisk = Some(32 << 10));
    let r = explore(&m, &opts(2, sp.clone()));
    match &r.outcome {
        Outcome::Io(msg) => assert!(msg.contains("-maxdisk"), "{msg}"),
        o => panic!("{o:?}"),
    }
    assert!(sp.counters().peak <= 32 << 10);
}

#[test]
fn memory_pressure_spills_trace_logs() {
    // A budget far below what the process uses: stage 2 starts at once.
    let m = Counters::new(1000, Some((999, 990)), 1);
    let plain = explore(&m, &opts(1, Spill::new(SpillConfig { mode: SpillMode::Off, ..Default::default() })));
    for workers in [1, 3] {
        let sp = spill("pressure", SpillMode::Auto, |_| {});
        let mut o = opts(workers, sp.clone());
        o.maxmem = 1 << 20;
        o.fp = FpConfig::fixed_mib(32);
        let r = explore(&m, &o);
        assert_eq!(r.outcome, plain.outcome);
        if workers == 1 {
            assert_eq!(r.stats, plain.stats);
        }
        assert_eq!(r.trace.len(), plain.trace.len());
        assert!(sp.counters().blocks > 0, "{:?}", sp.counters());
        assert!(sp.error().is_none());
        assert!(sp.dir().is_none());
    }
    // `-spill off`: nothing written.
    let sp = Spill::new(SpillConfig { mode: SpillMode::Off, dir: Some(test_dir("off")), ..Default::default() });
    let mut o = opts(1, sp.clone());
    o.maxmem = 1 << 20;
    o.fp = FpConfig::fixed_mib(32);
    let r = explore(&m, &o);
    assert_eq!(r.outcome, plain.outcome);
    assert_eq!(sp.counters().blocks, 0);
}

/// `n` initial states (counters `(i, 0)`) and their successors `(i, 1)`;
/// records the blocks written to the spill directory when the initial
/// states are all generated.
struct ManyInits {
    n: u32,
    spill: Spill,
    blocks_after_init: AtomicU64,
    inv: Vec<String>,
    acts: Vec<String>,
}

impl Model for ManyInits {
    type State = (u32, u32);
    tlx_runtime::model_without_checks!();
    fn init_states(&self, out: &mut dyn FnMut((u32, u32))) -> Result<(), ModelError> {
        for i in 0..self.n {
            out((i, 0));
        }
        self.blocks_after_init.store(self.spill.counters().blocks, Ordering::SeqCst);
        Ok(())
    }
    fn successors(&self, s: &(u32, u32), out: &mut dyn FnMut((u32, u32), u32) -> bool) -> Result<(), ModelError> {
        if s.1 == 0 {
            out((s.0, 1), 0);
        }
        Ok(())
    }
    fn fingerprint(&self, s: &(u32, u32)) -> u64 {
        ((s.0 as u64) << 32 | s.1 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x1234
    }
    fn check_invariants(&self, _: &(u32, u32)) -> Result<Option<usize>, ModelError> {
        Ok(None)
    }
    fn invariant_names(&self) -> &[String] {
        &self.inv
    }
    fn action_names(&self) -> &[String] {
        &self.acts
    }
    fn render(&self, s: &(u32, u32)) -> String {
        format!("/\\ x = {}\n/\\ y = {}", s.0, s.1)
    }
}

#[test]
fn memory_pressure_applies_while_generating_initial_states() {
    // A budget far below what the process uses: the trace log of the
    // initial states goes to disk while they are generated, not only once
    // the workers start.
    for workers in [1, 3] {
        let sp = spill("init", SpillMode::Auto, |_| {});
        let m =
            ManyInits { n: 200_000, spill: sp.clone(), blocks_after_init: AtomicU64::new(0), inv: Vec::new(), acts: vec!["Next".into()] };
        let mut o = opts(workers, sp.clone());
        o.maxmem = 1 << 20;
        o.fp = FpConfig::fixed_mib(32);
        let r = explore(&m, &o);
        assert_eq!(r.outcome, Outcome::Ok);
        assert_eq!((r.stats.distinct, r.stats.depth), (400_000, 2));
        assert!(m.blocks_after_init.load(Ordering::SeqCst) > 0, "nothing spilled during the initial states: {:?}", sp.counters());
        assert!(sp.error().is_none());
    }
}
