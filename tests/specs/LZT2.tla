---- MODULE LZT2 ----
\* As LZT1 with the branches swapped: the invariant is violated at
\* (x = 4, b = 0) before TLC evaluates the failing predicate at
\* (x = 4, b = 1). The atoms the search evaluated show it: no second search.
EXTENDS Naturals
VARIABLES x, b
vars == <<x, b>>
Init == x = 0 /\ b \in {0, 1}
Next == x < 4 /\ x' = x + 1 /\ b' = b
Spec == Init /\ [][Next]_vars
Inv == ~(b = 0 /\ x = 4)
Prop == (x = 1) ~> (IF b = 1 THEN 10 \div (4 - x) > 0 ELSE TRUE)
====
