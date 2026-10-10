//! The crate's feature tiers, asserted where they are compiled.
//!
//! `scripts/dependency_check.py` reads the manifests and the resolved
//! `cargo tree` closures; this target asserts the same policy from the other
//! side, inside the build: which tiers `rustc` was actually told to compile,
//! whether each tier's own fixtures are reachable without the higher ones,
//! and - through a known-bad `#[cfg]` probe compiled by `rustc` itself - that
//! a `feature = "..."` cfg really selects code (ARCH-04, ARCH-05).
//!
//! It runs in every configuration, including the empty one: a tier's
//! assertions are compiled only when that tier is on, so
//! `cargo test -p tlx-testkit --no-default-features --features values` and up
//! each exercise a different subset of this target.

use std::path::Path;
use std::process::{Command, Output};

/// The crate's tiers, a tier implying every one earlier in this list.
/// `models` is the back-compat aggregate of the model tiers (`compiled`).
const TIERS: [&str; 6] = ["runtime", "values", "interpreted", "compiled", "models", "jit"];

/// Whether this build compiled `tier`.
fn tier(tier: &str) -> bool {
    match tier {
        "runtime" => cfg!(feature = "runtime"),
        "values" => cfg!(feature = "values"),
        "interpreted" => cfg!(feature = "interpreted"),
        "compiled" => cfg!(feature = "compiled"),
        "models" => cfg!(feature = "models"),
        "jit" => cfg!(feature = "jit"),
        other => panic!("no such tier: {other}"),
    }
}

/// The cfgs a `rustc` invocation needs to select exactly this build's tiers.
fn this_build_cfgs() -> Vec<String> {
    TIERS.iter().filter(|t| tier(t)).map(|t| format!("feature=\"{t}\"")).collect()
}

/// `rustc` as this build's cargo runs it.
fn rustc() -> String {
    std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string())
}

/// Compile `source` (in `dir`) as a metadata-only library with `cfgs` added.
fn probe(dir: &Path, source: &Path, cfgs: &[String]) -> Output {
    let mut command = Command::new(rustc());
    command
        .arg("--crate-type=lib")
        .arg("--edition=2021")
        .arg("--emit=metadata")
        .arg("--out-dir")
        .arg(dir)
        .args(cfgs.iter().flat_map(|cfg| ["--cfg", cfg.as_str()]))
        .arg(source);
    command.output().unwrap_or_else(|error| panic!("cannot run {}: {error}", rustc()))
}

/// A command's diagnostics, for an assertion message.
fn diagnostics(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

#[test]
fn every_enabled_tier_brings_its_prerequisites() {
    // A manifest edit that drops one forwarding - say `jit` losing
    // `tlx-compile/jit` - fails here first: the tier is on but what it
    // promises to bring is not.
    let order: [(&str, &str); 5] =
        [("jit", "models"), ("models", "compiled"), ("compiled", "interpreted"), ("compiled", "values"), ("interpreted", "runtime")];
    for (higher, lower) in order {
        assert!(!tier(higher) || tier(lower), "feature {higher} is on without {lower}: the manifest's forwarding is broken");
    }
    let on: Vec<&str> = TIERS.iter().copied().filter(|t| tier(t)).collect();
    tlx_testkit::note(&format!("feature policy: this target compiled the tiers {on:?}"));
}

// Each tier's own fixtures must work alone; that they are selectable alone
// (`--no-default-features --features values` without the tiers above) is the
// closure assertions of `scripts/dependency_check.py`, which no build of this
// target can state about itself. Selecting the tier below the one under test
// must not be required, so the acceptance runs this target once per tier.

#[cfg(feature = "values")]
#[test]
fn the_values_tier_shapes_values() {
    let mut rng = tlx_testkit::Rng::new(1);
    let value = tlx_testkit::values::random_value(&tlx_layout::Shape::Bool, &mut rng);
    assert!(matches!(value, tlx_value::Value::Bool(_)), "{value:?}");
}

#[cfg(feature = "interpreted")]
#[test]
fn the_interpreted_tier_runs_a_spec() {
    let source = tlx_testkit::module("Tier", "VARIABLE x\nInit == x = 0\nNext == x' = x");
    let summary = tlx_testkit::spec::run_mode(&source, "INIT Init\nNEXT Next\n", tlx_testkit::spec::Mode::Interp);
    assert_eq!((summary.outcome.as_str(), summary.distinct), ("Ok", 1));
}

#[cfg(feature = "compiled")]
#[test]
fn the_compiled_tier_agrees_with_its_interpreter_reference() {
    let source = tlx_testkit::module("Tier", "VARIABLE x\nInit == x = 0\nNext == x' = x");
    let summary = tlx_testkit::spec::assert_modes_agree(&source, "INIT Init\nNEXT Next\n");
    assert_eq!(summary.distinct, 1);
    assert_eq!(tlx_testkit::diff::Tally::default().wrong, 0);
}

/// The known-bad cfg probe: a source that cannot compile when the `jit` cfg is
/// set. Compiling it with the cfg unset must succeed (nothing was selected)
/// and compiling it with `--cfg feature="jit"` must fail on that very message
/// (the branch was compiled). Asserting only the second half would let a
/// typo'd cfg - one that selects nothing at all - read as a proven feature
/// closure, and every `#[cfg]`-based configuration claim rests on this
/// (ARCH-04).
#[test]
fn a_feature_cfg_selects_the_branch_it_gates() {
    const SOURCE: &str = "#[cfg(feature = \"jit\")]\ncompile_error!(\"feature_policy probe: the jit branch was compiled\");\n";
    let dir = tlx_testkit::TempDir::new("feature_policy");
    let source = dir.write("probe.rs", SOURCE);

    let without = probe(&dir, &source, &[]);
    assert!(
        without.status.success(),
        "the known-bad probe must compile with its cfg unset, or a successful compile proves \
         nothing: {}",
        diagnostics(&without)
    );

    let with = probe(&dir, &source, &["feature=\"jit\"".to_string()]);
    assert!(
        !with.status.success(),
        "--cfg 'feature=\"jit\"' did not compile the branch it gates, so the feature closures this \
         crate asserts rest on a cfg that selects nothing"
    );
    assert!(diagnostics(&with).contains("feature_policy probe"), "the probe failed for an unrelated reason: {}", diagnostics(&with));

    // The other direction, for this build: a rustc run with exactly this
    // target's tiers must report the same `jit` state the compiled code sees.
    let own = probe(&dir, &source, &this_build_cfgs());
    assert_eq!(
        own.status.success(),
        !cfg!(feature = "jit"),
        "this target compiled with jit = {} but a rustc run with its own cfgs disagrees: {}",
        cfg!(feature = "jit"),
        diagnostics(&own)
    );
}
