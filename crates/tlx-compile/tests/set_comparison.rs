//! Compare fallible set operations in the interpreter and both lowered executors.

use tlx_layout::{store::StoreRef, Shape};
use tlx_testkit::diff::Harness;

#[test]
fn union_assignment_preserves_comparison_errors() {
    let source = include_str!("../../../tests/specs/UnionAssignmentComparisonError.tla");
    let h = Harness::new(source, "INIT Init\nNEXT Next\nCHECK_DEADLOCK FALSE\n");
    let report = h.run_reachable(5);
    report.assert_ok(source);
    let (exec, native) = report.tallies.get("action Next").expect("action evaluated");
    assert!(exec.both_fail > 0 && native.both_fail + native.unsupported > 0, "{}", report.render());
}

#[test]
fn set_comparisons_preserve_errors_and_effects() {
    for (source, vars, error) in [
        (include_str!("../../../tests/specs/UnionComparisonError.tla"), 2, true),
        (include_str!("../../../tests/specs/UnionSubsetComparison.tla"), 2, false),
        (include_str!("../../../tests/specs/UnionTailComparison.tla"), 1, true),
        (include_str!("../../../tests/specs/SetDifferenceComparisonEffects.tla"), 1, false),
    ] {
        for shape in [Shape::Dyn, Shape::Ref(StoreRef::new(1024 * 1024))] {
            let h = Harness::with_shapes(source, "INIT Init\nNEXT Next\nINVARIANT Inv\nCHECK_DEADLOCK FALSE\n", vec![shape; vars]);
            let report = h.run_reachable(5);
            report.assert_ok(source);
            let (exec, native) = report.tallies.get("invariant Inv").expect("invariant evaluated");
            assert_eq!(exec.wrong + native.wrong + exec.unsupported, 0, "{}", report.render());
            // A boxed collection may use a host region in the complete CLI
            // tier. This direct harness runs only wholly native predicates.
            if error {
                assert!(exec.both_fail > 0 && native.both_fail + native.unsupported > 0, "{}", report.render());
            } else {
                assert!(exec.agree > 0 && native.agree + native.unsupported > 0, "{}", report.render());
            }
        }
    }
}
