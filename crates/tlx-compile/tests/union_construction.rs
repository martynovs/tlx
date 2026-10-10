//! A set assignment consumes the normalized union, even when an operand is
//! lazy. It can construct directly into the destination bitset.
use tlx_testkit::diff::Harness;
use tlx_testkit::module;

#[test]
fn union_of_record_set_map_assigns_without_host_regions() {
    let src = module(
        "UnionConstruction",
        r#"EXTENDS Naturals
VARIABLES s, n
Init == s = {} /\ n = 1
Next == /\ s' = s \cup {[term |-> n, lsn |-> i] : i \in 1..n}
        /\ n' = 3 - n
"#,
    );
    let h = Harness::new(&src, "INIT Init\nNEXT Next\n");
    let report = h.run_reachable(20);
    report.assert_ok("union construction");
    let (_, native) = report.totals();
    assert_eq!(native.unsupported, 0, "{}", report.render());
    assert_eq!(native.bailout, 0, "{}", report.render());
    assert!(native.agree > 0);
}
