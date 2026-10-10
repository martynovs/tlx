---- MODULE LX6 ----
\* TLCSet("exit", TRUE) in the initial predicate: TLC generates
\* all initial states, expands none, and checks the temporal properties on
\* an empty graph (no behavior: no error), not on the whole state space
\* (where the behavior stuttering at n = 0 violates Prop).
EXTENDS Naturals, TLC
VARIABLES n
Init == n \in {0, 1} /\ TLCSet("exit", TRUE)
Next == n < 10 /\ n' = n + 1
Spec == Init /\ [][Next]_n
Prop == <>(n = 10)
====
