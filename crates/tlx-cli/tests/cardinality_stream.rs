//! A quantifier over a set whose `size()` overflows TLC's int is streamed, not
//! materialized: the interpreter materialized the domain of a bound variable
//! and failed with the overflow, while the compiled tier already streamed its
//! quantifiers, so `TLX_PARANOID` reported the divergence.
//! `SpanningTree/SpanTreeTest5Nodes` and
//! `CarTalkPuzzle/CarTalkPuzzle.toolbox/Model_3/MC` failed the same way
//! through `x' \in S` and through printing a set filter, which TLC streams too
//! (the default order of `Enumerable.elements()`). For a *bound*, TLC asks for
//! the normalized order and materializes every domain but `SUBSET S`, so only
//! the `SUBSET` quantifier here is a claim of agreement with TLC; the
//! `[S -> T]` one documents tlx's streaming where TLC stops at `-maxSetSize`
//! (`docs/compat.md` section 5).

use std::path::Path;
use std::time::Duration;

use tlx_testkit::cli::counts;

/// Both quantifiers stop at an early element of TLC's enumeration order:
/// `SUBSET (1..31)` (2^31 elements) starts with `{}`, and in
/// `[1..5 -> 1..121]` (121^5 functions) the last domain element varies
/// fastest, so the third function is the constant 1 except `f[5] = 3`. A
/// materializing enumeration of either set would not finish.
const SPEC: &str = r#"---- MODULE CardStream ----
EXTENDS Naturals, FiniteSets

VARIABLE x

Init == x = 0

Next == x' = x

BigFcn == \E f \in [1..5 -> 1..121] : f[1] = 1 /\ f[5] = 3

BigSubset == \E s \in SUBSET (1..31) : s = {}

Inv == x = 0 /\ BigSubset /\ BigFcn
====
"#;

const CFG: &str = "INIT Init\nNEXT Next\nINVARIANT Inv\nCHECK_DEADLOCK FALSE\n";

/// Whether to run the default tier (else the `-tlc -interp` parity tier of
/// `tlx!`). The timeout turns a materializing enumeration into a failure
/// instead of a hang.
fn run(dir: &Path, default_tier: bool, args: &[&str], env: &[(&str, &str)]) -> (i32, String) {
    let mut cmd = if default_tier { tlx_testkit::tlx_default!() } else { tlx_testkit::tlx!() };
    cmd.args(["check", "CardStream.tla"]).args(args).dir(dir).timeout(Duration::from_secs(60));
    let out = cmd.envs(env.iter().copied()).run();
    (out.code, out.text())
}

/// Paranoid runs with forced misses (a one-state pre-run), on the IR executor
/// and on native code.
const PARANOID_MODES: [&[(&str, &str)]; 2] = [
    &[("TLX_PARANOID", "1"), ("TLX_PRERUN", "1"), ("TLX_NO_NATIVE", "1")],
    &[("TLX_PARANOID", "1"), ("TLX_PRERUN", "1"), ("TLX_NATIVE_SYNC", "1")],
];

#[test]
fn overflowing_bound_domains_are_streamed() {
    let dir = tlx_testkit::TempDir::with_spec("card_stream", SPEC, CFG);
    let (code, plain) = run(&dir, true, &[], &[]);
    assert_eq!(code, 0, "{plain}");
    assert!(!plain.contains("Overflow"), "{plain}");
    assert_eq!(
        counts(&plain),
        ["2 states generated, 1 distinct states found, 0 states left on queue.", "The depth of the complete state graph search is 1."]
    );
    // The interpreter (the parity tier) and the compiled tier agree.
    for (default_tier, args) in [(false, &[][..]), (true, &["-interp"][..]), (true, &["-compile"][..])] {
        let (code, out) = run(&dir, default_tier, args, &[]);
        assert_eq!((code, counts(&out)), (0, counts(&plain)), "default_tier={default_tier} {args:?}:\n{out}");
    }
    for env in PARANOID_MODES {
        let (code, out) = run(&dir, true, &["-compile"], env);
        assert_eq!((code, counts(&out)), (0, counts(&plain)), "{env:?}:\n{out}");
    }
}
