//! Safepoints during a search (`Options::safepoint_every`: every worker
//! requests a no-op safepoint every that many expanded states, whose
//! executor checks that no worker inserts a state meanwhile) change nothing:
//! same statistics, depth and shortest traces, with any number of workers.

use tlx_runtime::explore::test_safepoints_served;
use tlx_runtime::{explore, explore_resumable, Model, ModelError, Options, Outcome, Report};
use tlx_testkit::report::{project, ComparisonPolicy, Counts, Projection, Trace, TracePolicy};

/// States `(l, i)` with `i < width(l)`: long narrow stretches (expanded by a
/// single worker while the others wait at the level barrier) and wide
/// levels (expanded in parallel); `bad` violates the invariant, and a state
/// of `trap` makes its parent's expansion fail.
struct Bulges {
    layers: u32,
    bad: Option<(u32, u32)>,
    trap: Option<(u32, u32)>,
    inv: Vec<String>,
    acts: Vec<String>,
}

impl Bulges {
    fn new(layers: u32, bad: Option<(u32, u32)>, trap: Option<(u32, u32)>) -> Bulges {
        Bulges { layers, bad, trap, inv: vec!["NotBad".into()], acts: vec!["Step".into(), "Jump".into()] }
    }

    fn width(l: u32) -> u32 {
        match l % 200 {
            50..=53 => 2000,
            120..=121 => 300,
            _ if l.is_multiple_of(5) => 4,
            _ => 1,
        }
    }
}

impl Model for Bulges {
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
        let (w, w1) = (Bulges::width(l), Bulges::width(l + 1));
        let mut succ: Vec<((u32, u32), u32)> =
            if w1 >= w { (i * w1 / w..(i + 1) * w1 / w).map(|j| ((l + 1, j), 0)).collect() } else { vec![((l + 1, i % w1), 0)] };
        if i == 0 && l % 3 == 0 && l + 2 < self.layers {
            succ.push(((l + 2, Bulges::width(l + 2) - 1), 1));
        }
        for (t, a) in succ {
            if Some(t) == self.trap {
                return Err(ModelError::new("trapped"));
            }
            if !out(t, a) {
                break;
            }
        }
        Ok(())
    }
    fn fingerprint(&self, s: &(u32, u32)) -> u64 {
        ((s.0 as u64) << 32 | s.1 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x9abc
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

fn summary(r: &Report<(u32, u32)>) -> Projection<(u32, u32)> {
    project(
        r,
        ComparisonPolicy {
            counts: Counts::OnSuccess,
            outdegree: Counts::OnSuccess,
            trace: TracePolicy::LengthAndLast,
            ..ComparisonPolicy::COUNTS_AND_LENGTH
        },
    )
}

#[test]
fn safepoints_change_nothing() {
    let served = test_safepoints_served();
    let opts = |workers| Options { workers, check_deadlock: false, safepoint_every: 5, ..Default::default() };
    let layers = 700;
    // (The only initial state is (0, 0).)
    let distinct: u64 = 1 + (1..layers).map(|l| Bulges::width(l) as u64).sum::<u64>();
    let cases = [Bulges::new(layers, None, None), Bulges::new(layers, Some((452, 1999)), None), Bulges::new(layers, Some((610, 0)), None)];
    for (k, m) in cases.iter().enumerate() {
        let one = summary(&explore(m, &opts(1)));
        if k == 0 {
            assert_eq!(one.outcome, Outcome::Ok);
            assert_eq!(one.counts.as_ref().unwrap().distinct, distinct);
        } else {
            assert_eq!(one.outcome, Outcome::InvariantViolated("NotBad".into()));
            assert!(matches!(one.trace, Trace::LengthAndLast { last, .. } if last == m.bad));
        }
        for workers in [2, 3, 8] {
            for _ in 0..3 {
                let r = explore(m, &opts(workers));
                assert_eq!(summary(&r), one, "{workers} workers");
            }
        }
    }
    // A search stopped by an error and resumed ends as an uninterrupted one.
    let plain = Bulges::new(layers, None, None);
    let trapped = Bulges::new(layers, None, Some((251, 1)));
    for workers in [1, 4, 8] {
        let opts = opts(workers);
        let want = summary(&explore(&plain, &Options { safepoint_every: 0, ..opts.clone() }));
        let (r, from) = explore_resumable(&trapped, &opts, None, Some(usize::MAX));
        assert!(matches!(&r.outcome, Outcome::Error(e) if e.message == "trapped"), "{:?}", r.outcome);
        let (r, rest) = explore_resumable(&plain, &opts, Some(from.expect("resume point")), None);
        assert!(rest.is_none());
        assert_eq!(summary(&r), want, "resumed, {workers} workers");
    }
    let served = test_safepoints_served() - served;
    assert!(served > 1000, "{served} safepoints");
}
