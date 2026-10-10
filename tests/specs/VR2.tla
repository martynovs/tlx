---- MODULE VR2 ----
\* SYMMETRY with actions that are not symmetric (CHOOSE over the symmetry
\* set): which state of an orbit is found first (in BFS order) decides
\* what is reachable.
EXTENDS Naturals, TLC
CONSTANTS P
VARIABLES own, cnt, x
Perms == Permutations(P)
First == CHOOSE p \in P : TRUE
Init == own = First /\ cnt = [p \in P |-> 0] /\ x = 0
Next == \/ \E p \in P : /\ cnt[p] < 3
                        /\ cnt' = [cnt EXCEPT ![p] = @ + 1]
                        /\ own' = IF p = own THEN CHOOSE q \in P \ {own} : TRUE ELSE own
                        /\ x' = (x + cnt[own]) % 9
        \/ own = First /\ x < 8 /\ x' = x + 1 /\ cnt' = [p \in P |-> (cnt[p] + 1) % 4] /\ UNCHANGED own
====
