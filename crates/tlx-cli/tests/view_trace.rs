//! Coarse replay must preserve the violation and print only a genuine prefix
//! when runtime-dependent generation makes reconstruction impossible (D-30).
use std::time::Duration;

#[test]
fn counter_dependent_next_reports_why_its_trace_is_incomplete() {
    let dir = tlx_testkit::TempDir::new("view-counter-guard");
    std::fs::write(dir.join("ViewCounterGuard.tla"), include_str!("fixtures/view-counter-guard/ViewCounterGuard.tla")).unwrap();
    std::fs::write(dir.join("ViewCounterGuard.cfg"), include_str!("fixtures/view-counter-guard/ViewCounterGuard.cfg")).unwrap();
    let oracle = include_str!("fixtures/view-counter-guard/ViewCounterGuard.tlc.out");
    assert!(oracle.contains("STARTMSG 2123") && oracle.contains("STARTMSG 2128"));
    for mode in [&[][..], &["-tlc"]] {
        for tier in ["-interp", "-compile"] {
            for workers in ["1", "4"] {
                let r = tlx_testkit::tlx_default!()
                    .args(["check", "ViewCounterGuard.tla", tier, "-workers", workers])
                    .args(mode)
                    .envs([("TLX_PARANOID", "1"), ("TLX_PRERUN_REUSE", "0")])
                    .dir(&dir)
                    .timeout(Duration::from_secs(30))
                    .run();
                assert_eq!(r.code, 12, "{}", r.text());
                assert!(r.stdout.contains("Invariant Inv is violated"), "{}", r.text());
                assert!(
                    r.stdout.contains("trace cannot be reconstructed: Next depends on run-time counters (TLCGet/TLCSet/Random) under VIEW"),
                    "{}",
                    r.text()
                );
                assert!(r.stdout.contains("x = 0"), "{}", r.text());
                assert!(!r.stdout.contains("State 2:"), "{}", r.text());
            }
        }
    }
}

#[test]
fn partial_error_trace_does_not_append_an_unconnected_incomplete_successor() {
    let dir = tlx_testkit::TempDir::new("view-counter-incomplete");
    std::fs::write(dir.join("ViewCounterIncomplete.tla"), include_str!("fixtures/view-counter-guard/ViewCounterIncomplete.tla")).unwrap();
    std::fs::write(dir.join("ViewCounterIncomplete.cfg"), include_str!("fixtures/view-counter-guard/ViewCounterIncomplete.cfg")).unwrap();
    assert!(include_str!("fixtures/view-counter-guard/ViewCounterIncomplete.tlc.out").contains("STARTMSG 2128"));
    for tier in ["-interp", "-compile"] {
        let r = tlx_testkit::tlx_default!()
            .args(["check", "ViewCounterIncomplete.tla", "-tlc", tier, "-workers", "1"])
            .envs([("TLX_PARANOID", "1"), ("TLX_PRERUN_REUSE", "0")])
            .dir(&dir)
            .timeout(Duration::from_secs(30))
            .run();
        assert_ne!(r.code, 0, "{}", r.text());
        assert!(r.stdout.contains("not completely specified"), "{}", r.text());
        assert!(r.stdout.contains("trace cannot be reconstructed"), "{}", r.text());
        assert!(r.stdout.contains("x = 0"), "{}", r.text());
        assert!(!r.stdout.contains("State 2:"), "{}", r.text());
        assert!(!r.stdout.contains("x = 2"), "{}", r.text());
    }
}

#[test]
fn counter_dependent_constraint_reports_reconstruction_limit() {
    let dir = tlx_testkit::TempDir::new("view-counter-constraint");
    std::fs::write(dir.join("ViewCounterConstraint.tla"), include_str!("fixtures/view-counter-guard/ViewCounterConstraint.tla")).unwrap();
    std::fs::write(dir.join("ViewCounterConstraint.cfg"), include_str!("fixtures/view-counter-guard/ViewCounterConstraint.cfg")).unwrap();
    assert!(include_str!("fixtures/view-counter-guard/ViewCounterConstraint.tlc.out").contains("STARTMSG 2110"));
    for tier in ["-interp", "-compile"] {
        let r = tlx_testkit::tlx_default!()
            .args(["check", "ViewCounterConstraint.tla", "-tlc", tier, "-workers", "1"])
            .envs([("TLX_PARANOID", "1"), ("TLX_PRERUN_REUSE", "0")])
            .dir(&dir)
            .timeout(Duration::from_secs(30))
            .run();
        assert_eq!(r.code, 12, "{}", r.text());
        assert!(r.stdout.contains("Invariant Inv is violated"), "{}", r.text());
        assert!(r.stdout.contains("a replayed predicate depends on run-time counters"), "{}", r.text());
        assert!(!r.stdout.contains("State 2:"), "{}", r.text());
    }
}
