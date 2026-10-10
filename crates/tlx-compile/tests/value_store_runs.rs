//! The value store (`TLX_VALUE_STORE`) is run-owned: runs in one process,
//! of one model or of two, one after the other or at the same time, each
//! get their own store, and runs of one model, whose native code is one
//! cached library, do not share one through it (each load calls back into
//! its own run's store). A handle of another store is rejected: the
//! action falls back to the interpreter, which the runs must not need.

use std::sync::{Arc, Once};

use tlx_runtime::{Options, Outcome};
use tlx_testkit::spec::{load, run_mode, Mode};

/// A bag keyed by records with a sequence field (key universe too large
/// for a fixed layout: a value store slot).
const BAG: &str = r#"---- MODULE Bag ----
EXTENDS Naturals, Sequences, TLC
VARIABLES bag, n
K(i) == [id |-> i % 3, log |-> <<i, 100 * i>>]
Init == bag = (K(0) :> 0) /\ n = 0
Next == n < 8 /\ \E k \in DOMAIN bag :
          /\ bag' = [bag EXCEPT ![k] = @ + 1] @@ (K(n + 1) :> n)
          /\ n' = n + 1
====
"#;

/// Messages in flight as a function of records with logs to counts, with a
/// different shape of keys from `BAG`'s.
const MSGS: &str = r#"---- MODULE Msgs ----
EXTENDS Naturals, Sequences, TLC
VARIABLES msgs, t
M(s, l) == [src |-> s, entries |-> l]
Init == msgs = (M(1, <<>>) :> 1) /\ t = 0
Send(m) == msgs' = IF m \in DOMAIN msgs THEN [msgs EXCEPT ![m] = @ + 1] ELSE msgs @@ (m :> 1)
Next == t < 4 /\ t' = t + 1 /\ \E m \in DOMAIN msgs, s \in 1..2 :
          Send(M(s, Append(m.entries, 10 * t + s)))
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n";

/// The store on, with native code loaded before each search (so that the
/// generated code runs the store operations).
fn store_on() {
    static ON: Once = Once::new();
    ON.call_once(|| {
        std::env::set_var("TLX_VALUE_STORE", "1");
        std::env::set_var("TLX_NATIVE_SYNC", "1");
    });
}

/// The run's store id and (generated, distinct, depth).
fn compiled(src: &str) -> (u64, (u64, u64, u32)) {
    store_on();
    let model = load(src, CFG);
    model.ip.set_run_config(1, false);
    let opts = Options { workers: 1, check_deadlock: false, ..Default::default() };
    let (m, r) = tlx_compile::enc::check(Arc::new(model), &opts, 16).unwrap();
    assert!(matches!(r.outcome, Outcome::Ok), "{:?}", r.outcome);
    let store = m.value_store().expect("a value store slot");
    assert!(store.stats().nodes > 0, "the run stored no values");
    // (A handle of another store fails the store's checks, and the
    // interpreter redoes the action: correct, but not the run's own store.)
    assert_eq!(m.bailouts.get(), 0, "action runs fell back to the interpreter");
    (store.uid(), (r.stats.generated, r.stats.distinct, r.stats.depth))
}

fn interpreted(src: &str) -> (u64, u64, u32) {
    let s = run_mode(src, CFG, Mode::Interp);
    assert_eq!(s.outcome, "Ok");
    (s.generated, s.distinct, s.depth)
}

#[test]
fn runs_in_one_process_have_their_own_stores() {
    let (bag, msgs) = (interpreted(BAG), interpreted(MSGS));
    let mut uids = Vec::new();
    // One model twice (the second run loads the first one's library), then
    // another model, then both models twice at the same time.
    for (src, want) in [(BAG, bag), (BAG, bag), (MSGS, msgs)] {
        let (uid, got) = compiled(src);
        assert_eq!(got, want);
        uids.push(uid);
    }
    let runs: Vec<_> = [BAG, MSGS, BAG, MSGS].into_iter().map(|src| std::thread::spawn(move || compiled(src))).collect();
    for (r, want) in runs.into_iter().zip([bag, msgs, bag, msgs]) {
        let (uid, got) = r.join().unwrap();
        assert_eq!(got, want);
        uids.push(uid);
    }
    let n = uids.len();
    uids.sort_unstable();
    uids.dedup();
    assert_eq!(uids.len(), n, "two runs shared a value store");
}
