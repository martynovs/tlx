//! The experimental value store (`TLX_VALUE_STORE`, docs/configuration.md):
//! its byte budget (the run's `-maxmem`, never a fraction of it) is a hard
//! limit, and it has no checkpoint format yet: it is off when checkpoints
//! are asked for, and a run with only the default checkpoint interval takes
//! none.

use tlx_testkit::cli::counts;
use tlx_testkit::TempDir;

/// A bag keyed by records with a sequence field (a `Dyn` shape, so a value
/// store slot) growing by one key per step: every state's bag is a new
/// node, so the store grows quadratically.
const SPEC: &str = r#"---- MODULE Store ----
EXTENDS Naturals, Sequences, TLC
VARIABLES bag, n
K(i) == [id |-> i, log |-> <<i>>]
Init == bag = (K(0) :> 0) /\ n = 0
Next == n < 1000 /\ bag' = bag @@ (K(n + 1) :> n) /\ n' = n + 1
====
"#;
const CFG: &str = "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n";

fn check(dir: &TempDir, store: &str, args: &[&str]) -> tlx_testkit::cli::Run {
    tlx_testkit::tlx!()
        .check("Store.tla")
        .cfg("Store.cfg")
        .args(["-workers", "1", "-compile"])
        .args(args.iter().copied())
        .env("TLX_VALUE_STORE", store)
        .dir(dir.path())
        .run()
}

const COUNTS: [&str; 2] =
    ["1001 states generated, 1001 distinct states found, 0 states left on queue.", "The depth of the complete state graph search is 1001."];

#[test]
fn the_budget_is_a_hard_limit() {
    let dir = TempDir::with_spec("value-store-budget", SPEC, CFG);
    // 4 MiB: the bags of the first few hundred states (the budget used to
    // be a quarter of the memory budget).
    let out = check(&dir, "1", &["-maxmem", "4M"]);
    let text = out.text();
    assert_ne!(out.code, 0, "{text}");
    assert!(text.contains("the value store (TLX_VALUE_STORE) exceeded its budget of 4 MiB"), "{text}");
    assert!(text.contains("turn the store off (TLX_VALUE_STORE=0)"), "{text}");
    // Without the store the run completes in the same memory budget, and
    // with the default budget the store holds it.
    let out = check(&dir, "0", &["-maxmem", "4M"]);
    assert_eq!(counts(&out.text()), COUNTS, "{}", out.text());
    let out = check(&dir, "1", &[]);
    assert_eq!(counts(&out.text()), COUNTS, "{}", out.text());
}

#[test]
fn checkpoints_turn_the_store_off_or_are_not_taken() {
    let dir = TempDir::with_spec("value-store-checkpoint", SPEC, CFG);
    let out = check(&dir, "1", &["-checkpoint", "60"]);
    let text = out.text();
    assert!(text.contains("value store (TLX_VALUE_STORE) disabled: checkpoints keep Dyn values"), "{text}");
    assert_eq!(counts(&text), COUNTS, "{text}");
    let out = check(&dir, "1", &[]);
    let text = out.text();
    assert!(text.contains("value store (TLX_VALUE_STORE): this run takes no checkpoints"), "{text}");
    assert!(!text.contains("disabled: checkpoints"), "{text}");
}
