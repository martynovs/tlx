//! A constant that holds set values keeps its structure when a component is
//! the empty set: an all-empty set family becomes a set over the empty
//! element shape, and the components of a tuple whose set components differ
//! only by an empty one share a single shape. Membership in and iteration
//! over such a set then stay native instead of falling back to the
//! interpreter, and the element is still evaluated (TLC evaluates it before
//! the set).
use tlx_testkit::diff::Harness;
use tlx_testkit::module;

/// The components are all empty: the tuple holds one shape for every
/// component, so an index into it selects a set instead of falling back to
/// a boxed application, and membership in such a component (always the
/// empty set) is decided natively.
#[test]
fn all_empty_constant_component_is_a_set() {
    let src = module(
        "EmptyConstSets",
        r#"EXTENDS Naturals
CONSTANT S
VARIABLES n
MCS == <<{}, {}>>
Init == n = 1
Next == n' = 3 - n
Inv == /\ n \in 1..2
       /\ \A x \in {"a", "b"} : x \notin S[n]
"#,
    );
    let cfg = "CONSTANT S <- MCS\nINIT Init\nNEXT Next\nINVARIANT Inv\n";
    let h = Harness::new(&src, cfg);
    assert!(h.not_native().is_empty(), "empty-set constant needs host regions: {:?}", h.not_native());
    let report = h.run_reachable(4);
    report.assert_ok("all-empty constant component");
    let (ir, native) = report.totals();
    assert!(ir.agree > 0 && native.agree > 0, "{}", report.render());
    assert_eq!(native.unsupported, 0, "{}", report.render());
}

/// One component is empty and another is not: both take the shape of the
/// non-empty one, so the tuple has one component shape.
#[test]
fn mixed_empty_and_nonempty_components_share_a_shape() {
    let src = module(
        "MixedConstSets",
        r#"EXTENDS Naturals
CONSTANT S
VARIABLES n, seen
MCS == <<{"a"}, {}>>
Init == n = 1 /\ seen = FALSE
Next == /\ n' = 3 - n
        /\ seen' = (seen \/ (\A e \in S[n] : e = "a"))
Inv == /\ n \in 1..2
       /\ \A i \in 1..2 : \A x \in {"a", "b"} : (x \in S[i]) = (i = 1 /\ x = "a")
"#,
    );
    let cfg = "CONSTANT S <- MCS\nINIT Init\nNEXT Next\nINVARIANT Inv\n";
    let h = Harness::new(&src, cfg);
    assert!(h.not_native().is_empty(), "mixed constant components need host regions: {:?}", h.not_native());
    let report = h.run_reachable(4);
    report.assert_ok("mixed empty and non-empty constant components");
    let (ir, native) = report.totals();
    assert!(ir.agree > 0 && native.agree > 0, "{}", report.render());
    assert_eq!(native.unsupported, 0, "{}", report.render());
}

/// An element that fails is still evaluated: TLC evaluates the element
/// before it looks at the set, so membership in an always-empty set is an
/// error, not false.
#[test]
fn membership_in_an_all_empty_constant_still_evaluates_the_element() {
    let src = module(
        "EmptyConstSetError",
        r#"EXTENDS Naturals
CONSTANT S
VARIABLES n
MCS == <<{}>>
Init == n = 1
Next == n' = n
Err == (1 \div (n - n)) \in S[1]
"#,
    );
    let cfg = "CONSTANT S <- MCS\nINIT Init\nNEXT Next\nINVARIANT Err\n";
    let h = Harness::new(&src, cfg);
    let report = h.run_reachable(2);
    report.assert_ok("empty-set membership evaluates its element");
    let (ir, native) = report.totals();
    assert!(ir.both_fail > 0, "the interpreter must fail the element: {}", report.render());
    assert_eq!(native.wrong, 0, "{}", report.render());
    assert!(native.both_fail + native.unsupported > 0, "{}", report.render());
}
