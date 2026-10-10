---- MODULE LE1 ----
\* A state predicate of a temporal property fails at n = 2, in the
\* middle of the search (which goes on, recording no more edges): liveness
\* reports the evaluation error without exploring the state space again.
EXTENDS Naturals
VARIABLES n
Init == n = 0
Next == n < 4 /\ n' = n + 1
Spec == Init /\ [][Next]_n /\ WF_n(Next)
Prop == <>(10 \div (n - 2) = 99)
====
