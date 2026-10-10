//! Industrial hot paths must retain typed operations through quantifiers.
use tlx_testkit::{diff::Harness, module};

#[test]
fn log_entry_membership() {
    let src = module(
        "LogEntryMembership",
        r#"
EXTENDS Naturals, Sequences
VARIABLES log, committed
Init == log = <<[term |-> 1], [term |-> 2]>> /\ committed = {[index |-> 1, term |-> 1]}
Next == log' = Tail(log) /\ UNCHANGED committed
Entries(l) == {<<i, l[i].term>> : i \in DOMAIN l}
Contains(l, i, t) == <<i, t>> \in Entries(l)
Inv == \A e \in committed : Contains(log, e.index, e.term)
"#,
    );
    let h = Harness::new(&src, "INIT Init\nNEXT Next\nINVARIANT Inv\n");
    let mut report = h.run_reachable(10);
    h.run_random(&mut tlx_testkit::Rng::new(411), 100, &mut report);
    report.assert_ok("log entry membership");
    assert!(h.native.invariants.iter().all(|n| *n), "{:?}", h.not_native());
    assert!(!h.dump("Inv").contains("interp"), "{}", h.dump("Inv"));
}

#[test]
fn mapped_membership_retains_errors() {
    for (name, query) in [
        ("EmptyNeedleError", "<<1, 1 \\div 0>> \\in {<<i, i>> : i \\in DOMAIN q}"),
        ("LaterImageError", "<<1, 0>> \\in {<<i, 0 \\div (2 - i)>> : i \\in DOMAIN q}"),
        ("TupleTailError", "<<3, 0>> \\in {<<i, 0 \\div (2 - i)>> : i \\in DOMAIN q}"),
    ] {
        let body = format!(
            "EXTENDS Naturals, Sequences\nVARIABLE q\nInit == q = <<>>\nNext == Len(q) < 2 /\\ q' = Append(q, 1)\nInv == {query}\n"
        );
        let h = Harness::new(&module(name, &body), "INIT Init\nNEXT Next\nINVARIANT Inv\n");
        h.run_reachable(10).assert_ok(name);
        assert!(!h.dump("Inv").contains("interp"), "{}", h.dump("Inv"));
    }
}

#[test]
fn quorum_elements_stay_typed() {
    let src = module(
        "TypedQuorums",
        r#"
EXTENDS Naturals, FiniteSets
VARIABLES members, votes
Nodes == {1, 2, 3}
Quorums(S) == {q \in SUBSET S : Cardinality(q) * 2 > Cardinality(S)}
Init == members = Nodes /\ votes = {}
Next == \E q \in Quorums(members) : votes' = q /\ UNCHANGED members
Inv == \A q \in Quorums(members) : \A v \in q : v \in Nodes
"#,
    );
    let h = Harness::new(&src, "INIT Init\nNEXT Next\nINVARIANT Inv\n");
    assert!(h.not_native().is_empty(), "{:?}\n{}", h.not_native(), h.dump("Next"));
    h.run_reachable(20).assert_ok("typed quorums");
}

#[test]
fn powerset_filter_preserves_lazy_errors() {
    let src = module(
        "LazyQuorums",
        r#"
EXTENDS Naturals, FiniteSets
VARIABLE members
Init == members = {1, 2}
Next == UNCHANGED members
Q == {q \in SUBSET members : IF q = {} THEN TRUE ELSE 1 \div 0 = 0}
Inv == \E q \in Q : q = {}
"#,
    );
    let h = Harness::new(&src, "INIT Init\nNEXT Next\nINVARIANT Inv\n");
    h.run_reachable(5).assert_ok("lazy quorum filter");
}

#[test]
fn mapped_membership_retains_image_normalization_errors() {
    let src = module(
        "MappedNormalization",
        r#"
EXTENDS Naturals, Sequences
CONSTANT a
VARIABLE q
Init == q = <<>>
Next == Len(q) < 2 /\ q' = Append(q, 1)
Inv == ~(a \in {IF i = 1 THEN TRUE ELSE 1 : i \in DOMAIN q})
"#,
    );
    let h = Harness::new(&src, "INIT Init\nNEXT Next\nCONSTANT a = a\nINVARIANT Inv\n");
    h.run_reachable(5).assert_ok("mapped image normalization errors");
    let src = module(
        "MappedSetNormalization",
        r#"
EXTENDS Naturals
CONSTANT a
VARIABLE q
Init == q = {1, 2}
Next == UNCHANGED q
Images(s) == {IF i = 1 THEN TRUE ELSE 1 : i \in s}
Inv == ~(a \in Images(q))
"#,
    );
    let h = Harness::new(&src, "INIT Init\nNEXT Next\nCONSTANT a = a\nINVARIANT Inv\n");
    h.run_reachable(5).assert_ok("parameterized mapped set normalization errors");
}
