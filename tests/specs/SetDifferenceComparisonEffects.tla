---- MODULE SetDifferenceComparisonEffects ----
EXTENDS Naturals, TLC
VARIABLE d
Init == d = {1}
Next == UNCHANGED d
Inv == TLCSet(0, 0) /\ (LET Q == {q \in SUBSET d : IF TLCGet(0) < 4 THEN TLCSet(0, TLCGet(0) + 1) ELSE 1 \div 0 = 0} IN {Q} \ {Q}) = {}
====
