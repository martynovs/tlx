---- MODULE VG2 ----
\* State variables with empty universes (0-word states): an empty function
\* and an always-empty set; membership in the domain and EXCEPT on an absent
\* key read no state words.
EXTENDS Naturals
VARIABLES f, E
Init == f = [x \in {} |-> 0] /\ E = {}
Next == /\ f' = [f EXCEPT ![2] = 5]
        /\ E' = E \cap {1}
Inv == /\ 2 \notin DOMAIN f
       /\ \A k \in DOMAIN f : f[k] = 0
       /\ (1 \in E) => f[1] = 3
====
