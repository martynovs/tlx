//! `-coverage N`: TLC's coverage statistics at the level of actions, every
//! N minutes during the search and at its end (tlc2.tool.coverage.
//! CostModelCreator.report).
//!
//! TLC prints, per action, `<label>: distinct:generated` (the states the
//! action found first and those it generated, counted by the search, see
//! `tlx_runtime::Coverage`), and for the initial predicate
//! `<label>: generated:evaluated` (the initial states generated, and also
//! the complete assignments of the variables a conjunct after the last
//! assignment rejected, see `Interp::init_rejected`). Actions are TLC's
//! coverage entries: the instances of one action expression (with
//! different parameters) share one, in the order of their locations.
//!
//! Not printed (docs/compat.md): the number of distinct values of each
//! variable (TLC estimates it with a HyperLogLog of its fingerprints), the
//! lines of invariants, constraints and properties, and the evaluation
//! counts of every subexpression below these lines.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tlx_eval::model::InterpModel;
use tlx_eval::Interp;
use tlx_runtime::Coverage;

use crate::out::{self, code};

/// The coverage entries of a model, as TLC prints them.
pub struct Table {
    ip: Arc<Interp>,
    /// The labels of the initial predicate (TLC: one per conjunct of the
    /// SPECIFICATION that is a state predicate).
    init: Vec<String>,
    /// The label of each entry and the actions it counts, in TLC's order.
    entries: Vec<(String, Vec<usize>)>,
}

impl Table {
    pub fn new(base: &InterpModel) -> Table {
        let mut keyed: Vec<((String, (usize, usize), (usize, usize)), String, Vec<usize>)> = Vec::new();
        let n = tlx_runtime::Model::action_names(base).len();
        for (i, (label, at)) in (0..n).map(|i| (i, base.action_coverage(i))) {
            let key = (at.module, at.begin, at.end);
            match keyed.iter_mut().find(|(k, _, _)| *k == key) {
                Some((_, l, actions)) => {
                    // (TLC keeps one action per location: another one there,
                    // under the same INSTANCE statement, is not reported.)
                    if *l == label {
                        actions.push(i);
                    }
                }
                None => keyed.push((key, label, vec![i])),
            }
        }
        // (TLC sorts the actions by location: module name, then the
        // begin and end positions.)
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        Table { ip: base.ip.clone(), init: base.init_coverage(), entries: keyed.into_iter().map(|(_, l, a)| (l, a)).collect() }
    }

    /// The statistics `c`, as TLC prints them (one message per line) after
    /// `elapsed` of model checking.
    pub fn render(&self, c: &Coverage, elapsed: Duration) -> String {
        let mut text = out::format(
            code::TLC_COVERAGE_START,
            out::Class::None,
            &format!("The coverage statistics at {} (see https://explain.tlapl.us/module-coverage-statistics for how to interpret the following statistics).", out::now()),
        );
        let rejected = self.ip.init_rejected.load(Ordering::Relaxed);
        // (TLC counts the states for the last part.)
        for (k, label) in self.init.iter().enumerate() {
            let (g, e) = if k + 1 == self.init.len() { (c.init_generated, c.init_generated + rejected) } else { (0, 0) };
            text += &out::format(code::TLC_COVERAGE_INIT, out::Class::None, &format!("{label}: {g}:{e}"));
        }
        for (label, actions) in &self.entries {
            let sum = |v: &[u64]| actions.iter().map(|&a| v.get(a).copied().unwrap_or(0)).sum::<u64>();
            text +=
                &out::format(code::TLC_COVERAGE_NEXT, out::Class::None, &format!("{label}: {}:{}", sum(&c.distinct), sum(&c.generated)));
        }
        // (TLC's note after five minutes.)
        text += &if elapsed > Duration::from_secs(5 * 60) {
            out::format(
                code::TLC_COVERAGE_END_OVERHEAD,
                out::Class::None,
                "End of statistics (please note that for performance reasons large models\nare best checked with coverage and cost statistics disabled).",
            )
        } else {
            out::format(code::TLC_COVERAGE_END, out::Class::None, "End of statistics.")
        };
        text
    }
}

/// The periodic reports of `-coverage minutes` (TLC: the first one after
/// that many minutes, after the progress report; `-coverage 0`: after
/// every progress report).
pub fn reporter(table: Arc<Table>, interval: Duration, progress_every: Duration) -> tlx_runtime::CoverageReport {
    // (A little after the progress report due at the same time.)
    let after = Duration::from_millis(200);
    let (first, every) = if interval.is_zero() {
        (crate::PROGRESS_FIRST + after, progress_every)
    } else {
        // Validated whole minutes leave at least 15 seconds below Duration::MAX.
        (interval + after, interval)
    };
    let start = std::time::Instant::now();
    tlx_runtime::CoverageReport { first, every, report: Arc::new(move |c| print!("{}", table.render(c, start.elapsed()))) }
}
