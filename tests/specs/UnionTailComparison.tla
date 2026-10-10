---- MODULE UnionTailComparison ----
EXTENDS Naturals, FiniteSets
VARIABLE d
Init == d = 1..6
Next == UNCHANGED d
R(i) == [a |-> i, b |-> IF i = 6 THEN 0..2147483647 ELSE {}]
Inv == LET S == {R(i) : i \in d}
           U == S \cup {R(1)}
       IN Cardinality({S, U}) = 1
====
