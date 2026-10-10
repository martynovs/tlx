---- MODULE SN4 ----
\* A tuple binder <<x, y>> accepts only a tuple, not a function
\* over the enumerated domain {1, 2} (TLC: "Argument mismatch"), although
\* the two are equal.
EXTENDS Naturals
VARIABLES n, f
Init == n = 0 /\ f = [k \in {1, 2} |-> 0]
Next == /\ n < 2
        /\ n' = n + 1
        /\ f' = IF n = 0 THEN f ELSE [k \in {1, 2} |-> CHOOSE v \in {0, 1} : \E <<x, y>> \in {f} : x = v]
====
