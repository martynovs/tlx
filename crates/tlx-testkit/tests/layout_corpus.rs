//! Encoding round trip on the corpus (design.md §6.5.5 "验证策略"): for every
//! model that TLC checks successfully, collect up to `MAX_STATES` reachable
//! states with the interpreter, infer shapes from them, build a layout and
//! check that every state encodes, decodes back to itself, and that distinct
//! states get distinct encodings. Also checks that the layout tree
//! (`Layout::encode_state` / `decode_state`) gives exactly the encodings of
//! the `Shape`-based `layout::encode` / `decode`, and times both.
//!
//! Needs the corpus results (`corpus/out/tlc.json`) and `third_party/`
//! (see `tlx_testkit::require_corpus` for the skip/fail policy and the
//! `TLX_CORPUS_OUT` / `TLX_THIRD_PARTY` overrides); with the data present,
//! an empty selection or a run that tests no model fails. Slow in debug
//! builds, hence ignored by default:
//!
//!     cargo test --release -p tlx-testkit --test layout_corpus \
//!         --features interpreted,values -- --ignored --nocapture
//!
//! Environment: `CORPUS_JOBS` (models in parallel), `CORPUS_FILTER` (model
//! id substring), `CORPUS_SHAPES` (print every variable's shape).

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tlx_layout::infer::{Hints, Observer};
use tlx_layout::layout::{decode, encode};
use tlx_layout::{Layout, Shape};
use tlx_value::Value;

const MAX_STATES: usize = 3000;
/// Exploration budget per model (states found so far are still checked).
const TIME_BUDGET: Duration = Duration::from_secs(20);

enum Outcome {
    Skipped(String),
    Tested { states: usize, shapes: Vec<Shape>, failures: Vec<String>, secs: f64, timing: (Duration, Duration) },
}

fn check_model(tla: &Path, cfg: &Path) -> Outcome {
    let t0 = Instant::now();
    let states = match catch_unwind(AssertUnwindSafe(|| tlx_testkit::corpus::sample_states(tla, cfg, MAX_STATES, TIME_BUDGET))) {
        Ok(Ok(s)) if !s.is_empty() => s,
        Ok(Ok(_)) => return Outcome::Skipped("no states".into()),
        Ok(Err(e)) => return Outcome::Skipped(e),
        Err(_) => return Outcome::Skipped("interpreter panicked".into()),
    };
    let nvars = states[0].len();
    let mut obs = Observer::new(nvars);
    for s in &states {
        obs.observe(s);
    }
    let shapes = obs.finalize(&Hints::default());
    let layout = Layout::new(shapes.clone());
    let mut failures = Vec::new();
    let mut encs: HashMap<Vec<u64>, usize> = HashMap::new();
    let mut buf = vec![0u64; layout.words];
    for (i, s) in states.iter().enumerate() {
        for (v, sh) in s.iter().zip(&shapes) {
            if !sh.contains(v) {
                failures.push(format!("state {i}: contains false for {v}"));
            }
        }
        if let Err(m) = layout.encode_state(s, &mut buf) {
            failures.push(format!("state {i}: miss at {}: {}", m.path, m.value));
            continue;
        }
        let back = layout.decode_state(&buf);
        let norm: Vec<Value> = s.iter().map(|v| v.normalize().unwrap()).collect();
        if back != norm {
            failures.push(format!("state {i}: decode mismatch"));
        }
        if let Some(j) = encs.insert(buf.clone(), i) {
            failures.push(format!("states {j} and {i} share an encoding"));
        }
        if failures.len() > 5 {
            break;
        }
    }
    if failures.is_empty() && encs.len() != states.len() {
        failures.push(format!("{} distinct encodings for {} states", encs.len(), states.len()));
    }
    let timing = if failures.is_empty() { compare_tree(&layout, &states, &mut failures) } else { Default::default() };
    Outcome::Tested { states: states.len(), shapes, failures, secs: t0.elapsed().as_secs_f64(), timing }
}

/// The layout tree encodes and decodes every state exactly as the
/// `Shape`-based functions; returns the time of an encode + decode pass over
/// all states with the `Shape`-based functions and with the tree (best of 3).
fn compare_tree(layout: &Layout, states: &[Arc<[Value]>], failures: &mut Vec<String>) -> (Duration, Duration) {
    let mut tree = vec![0u64; layout.words];
    let mut old = vec![0u64; layout.words];
    for (i, s) in states.iter().enumerate() {
        layout.encode_state(s, &mut tree).unwrap();
        for (j, (sh, v)) in layout.vars.iter().zip(s.iter()).enumerate() {
            encode(sh, v, &mut old[layout.offsets[j]..]).unwrap();
        }
        if tree != old {
            failures.push(format!("state {i}: the layout tree's encoding differs"));
            return Default::default();
        }
        let back: Vec<Value> = layout.vars.iter().zip(&layout.offsets).map(|(sh, &o)| decode(sh, &old[o..])).collect();
        if back != layout.decode_state(&tree) {
            failures.push(format!("state {i}: the layout tree's decoding differs"));
            return Default::default();
        }
    }
    let (mut t_old, mut t_new) = (Duration::MAX, Duration::MAX);
    for _ in 0..3 {
        let t = Instant::now();
        for s in states {
            for (j, (sh, v)) in layout.vars.iter().zip(s.iter()).enumerate() {
                encode(sh, v, &mut old[layout.offsets[j]..]).unwrap();
            }
            let back: Vec<Value> = layout.vars.iter().zip(&layout.offsets).map(|(sh, &o)| decode(sh, &old[o..])).collect();
            std::hint::black_box(back);
        }
        t_old = t_old.min(t.elapsed());
        let t = Instant::now();
        for s in states {
            layout.encode_state(s, &mut tree).unwrap();
            std::hint::black_box(layout.decode_state(&tree));
        }
        t_new = t_new.min(t.elapsed());
    }
    (t_old, t_new)
}

fn has_dyn(s: &Shape) -> bool {
    match s {
        Shape::Dyn | Shape::Ref(_) => true,
        Shape::Never | Shape::Bool | Shape::Int { .. } | Shape::Atom(_) => false,
        Shape::Set { elem, .. } | Shape::Seq { elem, .. } => has_dyn(elem),
        Shape::Fcn { range, .. } | Shape::Map { range, .. } => has_dyn(range),
        Shape::Record(fs) => fs.iter().any(|f| has_dyn(&f.1)),
        Shape::Tuple(es) | Shape::Union(es) => es.iter().any(has_dyn),
    }
}

/// `s` cut to at most `max` bytes (plus the ellipsis), on a character
/// boundary: shapes print model values, whose text is arbitrary UTF-8.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

fn brief(s: &Shape) -> String {
    truncate(&format!("{s:?}"), 300)
}

#[test]
fn truncate_keeps_char_boundaries() {
    // 'ñ' is two bytes: a cut at byte 300 lands inside the first one.
    let long = format!("{}{}", "a".repeat(299), "ñ".repeat(10));
    assert_eq!(truncate(&long, 300), format!("{}...", "a".repeat(299)));
    assert_eq!(truncate(&"ñ".repeat(10), 3), format!("{}...", "ñ"));
    assert_eq!(truncate("abc", 300), "abc");
}

#[test]
#[ignore]
fn corpus_round_trip() {
    let Some(models) = tlx_testkit::corpus::models("corpus_round_trip", std::env::var("CORPUS_FILTER").ok().as_deref()) else { return };

    // `CORPUS_JOBS` overrides the number of models checked in parallel.
    let default_workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let workers = tlx_testkit::corpus::workers_from_env("CORPUS_JOBS", default_workers);
    let results = tlx_testkit::corpus::map_models(&models, workers, check_model);

    let (mut tested, mut total_states, mut dyn_models, mut dyn_vars, mut total_vars) = (0, 0, 0, 0, 0);
    let mut failed = Vec::new();
    let (mut sum_old, mut sum_new) = (Duration::ZERO, Duration::ZERO);
    let mut slowest: Vec<(Duration, Duration, &str)> = Vec::new();
    println!("\n=== corpus encode/decode round trip ===");
    for (id, r) in &results {
        match r {
            Outcome::Skipped(why) => println!("SKIP {id}: {why}"),
            Outcome::Tested { states, shapes, failures, secs, timing } => {
                tested += 1;
                sum_old += timing.0;
                sum_new += timing.1;
                slowest.push((timing.0, timing.1, id));
                total_states += states;
                total_vars += shapes.len();
                let nd = shapes.iter().filter(|s| has_dyn(s)).count();
                dyn_vars += nd;
                let words = Layout::new(shapes.clone()).words;
                println!("OK   {id}: {states} states, {} vars, {words} words, {nd} vars with Dyn ({secs:.1}s)", shapes.len());
                if std::env::var("CORPUS_SHAPES").is_ok() {
                    for s in shapes.iter() {
                        println!("       {}", brief(s));
                    }
                }
                if nd > 0 {
                    dyn_models += 1;
                    for (i, s) in shapes.iter().enumerate().filter(|(_, s)| has_dyn(s)) {
                        println!("       var {i}: {}", brief(s));
                    }
                }
                if !failures.is_empty() {
                    failed.push(format!("{id}: {}", failures.join("; ")));
                }
            }
        }
    }
    println!(
        "summary: {} models, {tested} tested, {} skipped, {total_states} states, {dyn_models} models / {dyn_vars} of {total_vars} vars with Dyn",
        results.len(),
        results.len() - tested,
    );
    slowest.sort_by(|a, b| b.0.cmp(&a.0));
    println!(
        "encode + decode of all states, Shape-based vs layout tree: {:.1} ms vs {:.1} ms",
        sum_old.as_secs_f64() * 1e3,
        sum_new.as_secs_f64() * 1e3
    );
    for (o, n, id) in slowest.iter().take(15) {
        println!(
            "  {id}: {:.2} ms vs {:.2} ms ({:.2}x)",
            o.as_secs_f64() * 1e3,
            n.as_secs_f64() * 1e3,
            o.as_secs_f64() / n.as_secs_f64().max(1e-9)
        );
    }
    assert!(failed.is_empty(), "round-trip failures:\n{}", failed.join("\n"));
    tlx_testkit::corpus::require_tested("corpus_round_trip", results.len(), tested);
}
