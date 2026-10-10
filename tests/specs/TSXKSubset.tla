---- MODULE TSXKSubset ----
\* FiniteSetsExt!kSubset as TLC's lazy KSubsetValue (TLC's
\* KSubsetAssumeTest): decided from k and the base set without enumerating
\* (membership, size, emptiness, comparisons), elements in normalized
\* order, printed as its elements when it has fewer than 64.
EXTENDS FiniteSets, FiniteSetsExt, Integers, TLC, TLCExt
\* (Its errors are checked by crates/tlx-cli/tests/java_only.rs: TLC's
\* texts of them carry the markers of -tool.)

ASSUME kSubset(2, 1..3) = {{1, 2}, {1, 3}, {2, 3}}
ASSUME kSubset(4, 1..3) = {} /\ kSubset(-1, 1..3) = {}
ASSUME kSubset(0, 1..3) = kSubset(0, 1..4)
ASSUME kSubset(1, 1..3) # kSubset(2, 1..3)
ASSUME kSubset(2, 1..4) = {s \in SUBSET (1..4) : Cardinality(s) = 2}
ASSUME {s \in SUBSET (1..4) : Cardinality(s) = 3} = kSubset(3, 1..4)
\* Infinite bases.
ASSUME Cardinality(kSubset(0, Nat)) = 1
ASSUME kSubset(0, Nat) = {{}} /\ kSubset(-1, Nat) = {}
ASSUME {1} \in kSubset(1, Nat) /\ {} \notin kSubset(1, Nat)
ASSUME ~IsFiniteSet(kSubset(1, Nat)) /\ IsFiniteSet(kSubset(0, Nat))
ASSUME Cardinality({kSubset(0, Nat), kSubset(1, Nat)}) = 2
\* Large bases.
ASSUME {1, 2} \in kSubset(2, 1..64)
ASSUME Cardinality(kSubset(2, 1..64)) = 2016
ASSUME Cardinality(kSubset(999999, 1..1000000)) = 1000000
ASSUME Cardinality({kSubset(31, 1..64), kSubset(32, 1..64)}) = 2
ASSUME kSubset(2, 1..64) # SUBSET (1..64)
ASSUME [n1 : kSubset(32, 1..64)] = [n1 : kSubset(32, 1..64)]
\* Fingerprints and printing.
ASSUME TLCFP(kSubset(2, 1..3)) = TLCFP({{1, 2}, {1, 3}, {2, 3}})
ASSUME ToString(kSubset(2, 1..4)) = "{{1, 2}, {1, 3}, {1, 4}, {2, 3}, {2, 4}, {3, 4}}"
ASSUME ToString(kSubset(2, 1..12)) = "{s \\in SUBSET (1..12) : Cardinality(s) = 2}"

\* In a model: an initial state per 2-subset, steps to the first 2-subset
\* that is different (CHOOSE: the normalized order).
Nodes == {"a", "b", "c", "d"}
VARIABLE q
Init == q \in kSubset(2, Nodes \ {"d"})
Next == \/ q' = CHOOSE s \in kSubset(2, Nodes) : s # q
        \/ \E s \in kSubset(3, Nodes) : q \subseteq s /\ q' = s \ {CHOOSE e \in q : TRUE}
Inv == Cardinality(q) = 2
====
