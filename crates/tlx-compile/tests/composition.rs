//! Action composition retains independent intermediate and outer states.
use tlx_testkit::{diff::Harness, module};

#[test]
fn composition_is_compiled() {
    let src = module(
        "CompiledComposition",
        r#"
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
A == x < 2 /\ x' = x + 1
B == x' = x /\ y' = x
Step == (A \cdot B) /\ y' = x + 1
Reset == x = 2 /\ x' = 0 /\ y' = 0
Next == Step \/ Reset
"#,
    );
    let h = Harness::new(&src, "INIT Init\nNEXT Next\n");
    assert!(h.actions.iter().all(Option::is_some), "{:?}", h.notes);
    h.run_reachable(20).assert_ok("compiled composition");
}

#[test]
fn composition_contexts_and_multiple_paths() {
    for (name, action) in [
        ("Before", "y' = y /\\ ((x' = 1 /\\ y' = 2) \\cdot (x' = x + 1))"),
        ("After", "((x' \\in {0, 1}) \\cdot (x' = x + 1)) /\\ y' = x"),
        ("Nested", "(((x' = 0) \\cdot (x' = x + 1)) \\cdot (x' = x + 1)) /\\ y' = x"),
        ("Captured", "LET v == x + y IN ((x' = 0) \\cdot (x' = v)) /\\ y' = 0"),
        ("PartialBranches", "((x' = 0) \\cdot ((x' = 1) \\/ (x' = 2 /\\ y' = 1))) /\\ y' = 1"),
        ("Enabled", "(ENABLED ((x' = 0) \\cdot (x' = x + 1))) /\\ x' = 1 /\\ y' = 1"),
        ("Disabled", "(FALSE \\cdot (x' = 1)) /\\ y' = 0"),
    ] {
        let body = format!("EXTENDS Naturals\nVARIABLES x, y\nInit == x = 1 /\\ y = 1\nNext == {action}\n");
        let h = Harness::with_shapes(&module(name, &body), "INIT Init\nNEXT Next\n", vec![tlx_layout::Shape::Int { lo: 0, hi: 4 }; 2]);
        assert!(h.actions.iter().all(Option::is_some), "{name}: {:?}", h.notes);
        let mut report = h.run_reachable(30);
        h.run_random(&mut tlx_testkit::Rng::new(911), 100, &mut report);
        report.assert_ok(name);
    }
}
