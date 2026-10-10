//! Serialization round trip on the corpus: for every model that TLC
//! checks successfully, collect up to `MAX_STATES` reachable states with the
//! interpreter and check that every state value, the inferred shapes and
//! the layout survive `tlx_value::ser` / `tlx_layout::ser` unchanged, and that
//! the `Dyn` interner snapshot covers the handles of the encoded states.
//!
//! Needs the corpus results (`corpus/out/tlc.json`) and `third_party/`
//! (see `tlx_testkit::require_corpus` for the skip/fail policy and the
//! `TLX_CORPUS_OUT` / `TLX_THIRD_PARTY` overrides); with the data present,
//! a run that tests no model fails. Ignored by default:
//!
//!     cargo test --release -p tlx-testkit --test layout_ser_corpus \
//!         --features interpreted,values -- --ignored --nocapture

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::Duration;

use tlx_layout::infer::{Hints, Observer};
use tlx_layout::{dynval, ser, Layout};
use tlx_value::ser::{Reader, Writer};

const MAX_STATES: usize = 3000;
const TIME_BUDGET: Duration = Duration::from_secs(20);

enum Outcome {
    Skipped(String),
    Tested { states: usize, bytes: usize, failures: Vec<String> },
}

fn check_model(tla: &Path, cfg: &Path) -> Outcome {
    let states = match catch_unwind(AssertUnwindSafe(|| tlx_testkit::corpus::sample_states(tla, cfg, MAX_STATES, TIME_BUDGET))) {
        Ok(Ok(s)) if !s.is_empty() => s,
        Ok(Ok(_)) => return Outcome::Skipped("no states".into()),
        Ok(Err(e)) => return Outcome::Skipped(e),
        Err(_) => return Outcome::Skipped("interpreter panicked".into()),
    };
    let mut failures = Vec::new();
    // All states as one stream (shared symbol dictionary).
    let mut w = Writer::new();
    for s in &states {
        for v in s.iter() {
            if let Err(e) = w.value(v) {
                failures.push(format!("encode {v}: {}", e.message()));
                return Outcome::Tested { states: states.len(), bytes: 0, failures };
            }
        }
    }
    let bytes = w.into_bytes();
    let mut r = Reader::new(&bytes);
    for (i, s) in states.iter().enumerate() {
        for v in s.iter() {
            match r.value() {
                Ok(back) => {
                    let norm = v.normalize().unwrap();
                    if back != norm || back.to_string() != norm.to_string() {
                        failures.push(format!("state {i}: {v} decoded as {back}"));
                    }
                }
                Err(e) => failures.push(format!("state {i}: {e}")),
            }
        }
        if failures.len() > 5 {
            return Outcome::Tested { states: states.len(), bytes: bytes.len(), failures };
        }
    }
    if let Err(e) = r.finish() {
        failures.push(e.to_string());
    }

    // Shapes and layout.
    let mut obs = Observer::new(states[0].len());
    for s in &states {
        obs.observe(s);
    }
    let layout = Layout::new(obs.finalize(&Hints::default()));
    let mut w = Writer::new();
    ser::write_layout(&mut w, &layout);
    let b = w.into_bytes();
    let mut r = Reader::new(&b);
    match ser::read_layout(&mut r) {
        Ok(back) if back.vars == layout.vars && back.offsets == layout.offsets && r.finish().is_ok() => {}
        Ok(_) => failures.push("layout round trip differs".into()),
        Err(e) => failures.push(format!("layout: {e}")),
    }

    // Dyn handles of the encoded states are in the interner snapshot, and
    // the snapshot round-trips.
    let mut buf = vec![0u64; layout.words];
    for s in &states {
        if layout.encode_state(s, &mut buf).is_err() {
            failures.push("state does not encode".into());
            break;
        }
    }
    let snap = dynval::snapshot();
    let mut w = Writer::new();
    match ser::write_dyn_table(&mut w, &snap) {
        Ok(()) => {
            let b = w.into_bytes();
            let mut r = Reader::new(&b);
            match ser::read_dyn_table(&mut r) {
                Ok(back) if back == snap => {}
                Ok(_) => failures.push("dyn table round trip differs".into()),
                Err(e) => failures.push(format!("dyn table: {e}")),
            }
        }
        Err(e) => failures.push(format!("dyn table encode: {}", e.message())),
    }
    Outcome::Tested { states: states.len(), bytes: bytes.len(), failures }
}

#[test]
#[ignore]
fn corpus_ser_round_trip() {
    let Some(models) = tlx_testkit::corpus::models("corpus_ser_round_trip", None) else { return };

    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(3);
    let results = tlx_testkit::corpus::map_models(&models, workers, check_model);
    let (mut tested, mut total_states, mut total_bytes) = (0, 0, 0);
    let mut failed = Vec::new();
    for (id, r) in &results {
        match r {
            Outcome::Skipped(why) => println!("SKIP {id}: {why}"),
            Outcome::Tested { states, bytes, failures } => {
                tested += 1;
                total_states += states;
                total_bytes += bytes;
                if !failures.is_empty() {
                    failed.push(format!("{id}: {}", failures.join("; ")));
                }
            }
        }
    }
    println!(
        "summary: {} models, {tested} tested, {total_states} states, {total_bytes} bytes ({:.1} bytes/state)",
        results.len(),
        total_bytes as f64 / total_states.max(1) as f64
    );
    assert!(failed.is_empty(), "serialization failures:\n{}", failed.join("\n"));
    tlx_testkit::corpus::require_tested("corpus_ser_round_trip", results.len(), tested);
}
