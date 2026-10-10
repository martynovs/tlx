//! The value store with the switch on (`TLX_VALUE_STORE=1`,
//! docs/configuration.md; it is off by default): a run with a `Dyn` slot
//! keeps its values in the store, the two paths that stay special behave as
//! `docs/value-store.md` documents (a full budget is a hard error naming the
//! way out; checkpoints keep `Dyn` values, so the store is off when they are
//! asked for and the run takes no checkpoint when only the default interval
//! is on), and a run whose slots all have fixed shapes stores nothing.
//!
//! The switch itself (its values, the unset default, the interaction with
//! `TLX_ESCAPE`) is parsed in `tlx_compile::settings` and tested there: this
//! binary runs one process with one environment.

use std::sync::{Arc, Once};
use std::time::Duration;

use tlx_runtime::{AfterCommit, CheckpointConfig, Checkpointer, Options, Outcome};
use tlx_testkit::spec::{load, run_mode, Mode};

/// A bag keyed by records with a sequence field: the key universe has no
/// fixed bound, so the slot's shape has a `Dyn` part and the store holds
/// it. Every step adds one key, so the states' bags are all new nodes.
const BAG: &str = r#"---- MODULE Bag ----
EXTENDS Naturals, Sequences, TLC
VARIABLES bag, n
K(i) == [id |-> i, log |-> <<i>>]
Init == bag = (K(0) :> 0) /\ n = 0
Next == n < 1000 /\ bag' = bag @@ (K(n + 1) :> n) /\ n' = n + 1
====
"#;

/// Every slot of this run has a fixed shape (an integer): nothing of it
/// is a handle.
const FIXED: &str = r#"---- MODULE Fixed ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 6 /\ x' = x + 1
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n";

/// The environment of every test in this file: the store on, the escape
/// experiment off. (The compiled tier parses the settings once per process,
/// so a test that wants another environment needs another process - the CLI
/// tests' business, not this binary's.)
fn store_on_environment() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("TLX_VALUE_STORE", "1");
        std::env::remove_var("TLX_ESCAPE");
        // Run the generated code, not only the closure IR.
        std::env::set_var("TLX_NATIVE_SYNC", "1");
    });
}

/// The compiled run of `src` under `opts`: its model and its counts.
fn compiled(src: &str, opts: &Options) -> Result<(tlx_compile::enc::EncModel, (u64, u64, u32)), String> {
    store_on_environment();
    let model = load(src, CFG);
    model.ip.set_run_config(opts.workers, opts.check_deadlock);
    let (m, r) = tlx_compile::enc::check(Arc::new(model), opts, 16)?;
    assert!(matches!(r.outcome, Outcome::Ok), "{:?}", r.outcome);
    Ok((m, (r.stats.generated, r.stats.distinct, r.stats.depth)))
}

/// The interpreter's counts, the semantic reference (compat.md §0).
fn interpreted(src: &str) -> (u64, u64, u32) {
    let s = run_mode(src, CFG, Mode::Interp);
    assert_eq!(s.outcome, "Ok");
    (s.generated, s.distinct, s.depth)
}

fn options() -> Options {
    Options { workers: 1, check_deadlock: false, ..Default::default() }
}

/// The checkpointer of a run that takes checkpoints (or, with `explicit`
/// false, one that holds only TLC's default interval).
fn checkpointer(explicit: bool) -> Arc<Checkpointer> {
    let every = (!explicit).then(|| Duration::from_secs(30 * 60));
    Checkpointer::new(CheckpointConfig {
        every,
        every_states: None,
        after: AfterCommit::Continue,
        identity: Vec::new(),
        report: None,
        explicit,
    })
}

#[test]
fn a_dyn_slot_is_a_store_slot_with_the_switch_on() {
    let want = interpreted(BAG);
    let (m, got) = compiled(BAG, &options()).unwrap();
    assert_eq!(got, want);
    let store = m.value_store().expect("the switch holds a Dyn slot in the value store");
    assert!(store.stats().nodes > 0, "the run stored no values");
    assert_eq!(m.bailouts.get(), 0, "action runs fell back to the interpreter");
    assert!(m.layout.vars.iter().any(|s| matches!(s, tlx_layout::Shape::Ref(_))), "no slot is a handle");
}

#[test]
fn a_run_of_fixed_shapes_stores_nothing() {
    let want = interpreted(FIXED);
    let (m, got) = compiled(FIXED, &options()).unwrap();
    assert_eq!(got, want);
    // No slot is a handle and no value was stored: a run whose shapes are
    // all fixed neither encodes nor fingerprints through the store.
    assert!(m.value_store().is_none(), "a fixed-shape run took the store path");
    assert!(!m.layout.vars.iter().any(|s| matches!(s, tlx_layout::Shape::Ref(_))), "a slot became a handle");
}

/// The values of the `BAG` run, in bytes: what its store holds once the
/// whole search is done. The budget tests below are relative to it, so a
/// larger spec does not invalidate their numbers.
fn bag_bytes() -> usize {
    let (m, _) = compiled(BAG, &options()).unwrap();
    let bytes = m.value_store().expect("the run's store").stats().node_bytes as usize;
    assert!(bytes > 0, "the run stored no values");
    bytes
}

#[test]
fn the_budget_is_the_runs_own_budget_not_a_fraction_of_it() {
    // The store replaces the `Dyn` interner, which had no limit of its
    // own: a value set that fits the run's memory budget must not end the
    // run (it used to stop at a quarter of it, which this budget is below).
    let bytes = bag_bytes();
    let budget = bytes + bytes / 4;
    let opts = Options { maxmem: budget, ..options() };
    let (m, got) = compiled(BAG, &opts).unwrap();
    assert_eq!(got, interpreted(BAG));
    assert_eq!(m.value_store().expect("the run's store").budget(), budget, "the store's budget is not the run's");
}

#[test]
fn a_full_store_is_a_hard_error_naming_the_way_out() {
    // Half of what the values need: the store refuses and the run ends.
    let opts = Options { maxmem: bag_bytes() / 2, ..options() };
    let err = match compiled(BAG, &opts) {
        Err(e) => e,
        Ok(_) => panic!("a full value store must end the run"),
    };
    assert!(err.contains("the value store (TLX_VALUE_STORE) exceeded its budget of"), "{err}");
    assert!(err.contains("turn the store off (TLX_VALUE_STORE=0)"), "the error does not name the way out: {err}");
}

#[test]
fn explicit_checkpoints_keep_dyn_values() {
    let want = interpreted(BAG);
    let opts = Options { checkpoint: Some(checkpointer(true)), ..options() };
    let (m, got) = compiled(BAG, &opts).unwrap();
    assert_eq!(got, want);
    assert!(m.value_store().is_none(), "the store must be off when checkpoints are asked for");
}

#[test]
fn the_default_checkpoint_interval_keeps_the_store_and_takes_no_checkpoints() {
    let want = interpreted(BAG);
    let ck = checkpointer(false);
    let opts = Options { checkpoint: Some(ck.clone()), ..options() };
    let (m, got) = compiled(BAG, &opts).unwrap();
    assert_eq!(got, want);
    let store = m.value_store().expect("the default interval keeps the store");
    assert!(store.stats().nodes > 0, "the run stored no values");
    assert_eq!(ck.count(), 0, "the run must not take a checkpoint");
}
