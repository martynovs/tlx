---- MODULE ReprMissingPath ----
\* EXCEPT on a key or field outside the domain leaves the function unchanged
\* (TLC warns) and does not evaluate the replacement, also on nested paths and
\* when other updates of the same EXCEPT apply; zero-valued entries are
\* present keys, not absent ones.
EXTENDS Naturals, TLC
CONSTANT M1, M2
VARIABLES f, bag, n
Init == f = [a |-> [b |-> 1]] /\ bag = (M1 :> 0) /\ n = 0
Next == \/ n = 0 /\ f' = [f EXCEPT !.c = 1 \div 0] /\ bag' = bag /\ n' = 1
        \/ n = 0 /\ f' = [f EXCEPT !.a.c = CHOOSE x \in {} : TRUE] /\ bag' = bag /\ n' = 2
        \/ n = 0 /\ f' = [f EXCEPT !["a"]["b"] = @ + 1, !.d = 5] /\ bag' = bag /\ n' = 3
        \/ n = 0 /\ f' = f /\ bag' = [bag EXCEPT ![M2] = 1 \div 0] /\ n' = 4
        \/ n = 0 /\ f' = f /\ bag' = [bag EXCEPT ![M1] = @ + 1] /\ n' = 5
        \/ n \in {1, 2, 4} /\ f' = [f EXCEPT !.a.b = @ * 10] /\ bag' = bag /\ n' = n + 10
        \/ n = 5 /\ f' = f /\ bag' = (IF M2 \in DOMAIN bag THEN bag ELSE bag @@ (M2 :> 0)) /\ n' = 6
Inv == /\ n \in {1, 2, 4} => f = [a |-> [b |-> 1]] /\ bag = (M1 :> 0)
       /\ n = 3 => f = [a |-> [b |-> 2]]
       /\ n = 6 => DOMAIN bag = {M1, M2} /\ bag[M2] = 0
====
